use super::typography;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, OnceLock},
};

use crate::{
    css::{
        self, BlockItem, Rule, Stylesheet,
        media::{MediaEnvironment, MediaQueryList},
        selectors::{
            self, Selector, Specificity,
            matching::{Bloom, element_keys},
        },
        values::{
            Color, Edges, FontStyle, FontWeight, Length, LengthContext, LineHeight, ResolvedColor,
            SpecifiedLength,
        },
    },
    dom::{Document, NodeId, NodeKind},
};

use super::{
    ComputedStyle, ComputedStyles, Diagnostic, Display, InteractionState, PseudoElement,
    StylesheetCollection, collect_stylesheets,
    compat::DeclarationContext,
    container::{ContainerQuery, ContainerSnapshot},
    control::{self, Layers, Scope},
    declarations::{self, DeclarationProperty, Property, StyledDeclaration, Value, WideKeyword},
    matching::{MatchingContext, RuleIndex},
    property::{Registration, Registrations},
    report_css_errors,
    variables::{self, Budget, Failure},
};

const MAX_FONT_SIZE: f32 = 512.0;

#[derive(Clone, Debug)]
struct StyledRule {
    selectors: Vec<StyledSelector>,
    declarations: Vec<StyledDeclaration>,
    order: usize,
    active: bool,
    layer: usize,
    layer_rank: usize,
    layer_count: usize,
    scopes: Vec<Scope>,
    containers: Vec<ContainerQuery>,
}

#[derive(Clone, Debug)]
struct StyledSelector {
    selector: Selector,
    ancestors: Bloom,
}

#[derive(Clone, Debug, Default)]
struct StyledSheet {
    rules: Vec<StyledRule>,
    index: RuleIndex,
    registrations: Vec<RegisteredRule>,
    fonts: Vec<FontRule>,
    counters: Vec<(css::counter_style::CounterStyle, usize, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FontRule {
    pub descriptor: css::font_face::FontDescriptor,
    pub node_id: Option<NodeId>,
    pub url: Option<url::Url>,
    pub active: bool,
    layer: usize,
    rank: usize,
}

/// Uses the same conditional/import/layer compiler as author declarations.
pub(crate) fn font_rules(
    document: &Document,
    sheets: &StylesheetCollection,
    environment: MediaEnvironment,
) -> Vec<FontRule> {
    let mut layers = Layers::default();
    let mut compiled = Vec::new();
    for sheet in &sheets.stylesheets {
        let mut diagnostics = Vec::new();
        let active = document
            .node(sheet.node_id)
            .and_then(|n| n.as_element())
            .is_none_or(|e| {
                e.attribute("disabled").is_none()
                    && e.attribute("media").is_none_or(|s| {
                        media_matches(s, environment, Some(sheet.node_id), &mut diagnostics, true)
                    })
            });
        let mut compiler = Compiler::<0>::new(
            Some(sheet.node_id),
            None,
            &mut diagnostics,
            environment,
            &mut layers,
            DeclarationContext::for_document(document),
        );
        compiler.url = sheet.url.clone();
        compiled.push(compiler.compile(&sheet.stylesheet, active));
    }
    let mut fonts: Vec<_> = rank_layers(compiled, &layers)
        .into_iter()
        .flat_map(|s| s.fonts)
        .take(256)
        .collect();
    fonts.sort_by_key(|f| f.rank);
    fonts
}

#[derive(Clone, Debug)]
struct RegisteredRule {
    name: String,
    registration: Registration,
    layer: usize,
    rank: usize,
}

#[derive(Clone, Copy, Debug)]
enum Origin {
    UserAgent,
    User,
    Author,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CascadeLevel {
    UserAgent,
    User,
    Presentational,
    Author,
    AuthorImportant,
    UserImportant,
    UserAgentImportant,
}

impl Origin {
    fn level(self, important: bool) -> CascadeLevel {
        match (self, important) {
            (Self::UserAgent, false) => CascadeLevel::UserAgent,
            (Self::User, false) => CascadeLevel::User,
            (Self::Author, false) => CascadeLevel::Author,
            (Self::Author, true) => CascadeLevel::AuthorImportant,
            (Self::User, true) => CascadeLevel::UserImportant,
            (Self::UserAgent, true) => CascadeLevel::UserAgentImportant,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Priority {
    level: CascadeLevel,
    inline: bool,
    layer_rank: usize,
    specificity: Specificity,
    proximity: std::cmp::Reverse<usize>,
    sheet: usize,
    rule: usize,
    declaration: usize,
    layer: usize,
    normal_rank: usize,
}

#[derive(Clone, Debug)]
pub struct StyleResolution {
    pub styles: ComputedStyles,
    pub stylesheets: StylesheetCollection,
}

pub fn resolve(document: &Document) -> StyleResolution {
    resolve_with_environment(document, MediaEnvironment::default())
}

/// Resolve embedded sheets for a CSS-pixel viewport and media environment.
pub fn resolve_with_environment(
    document: &Document,
    environment: MediaEnvironment,
) -> StyleResolution {
    resolve_with_stylesheets_and_environment(document, collect_stylesheets(document), environment)
}

pub fn resolve_with_stylesheets(
    document: &Document,
    stylesheets: StylesheetCollection,
) -> StyleResolution {
    resolve_with_stylesheets_and_environment(document, stylesheets, MediaEnvironment::default())
}

/// Resolve retained author ASTs, including nested media and style/link media
/// attributes, for an explicit environment. Does not fetch or discard sheets.
pub fn resolve_with_stylesheets_and_environment(
    document: &Document,
    stylesheets: StylesheetCollection,
    environment: MediaEnvironment,
) -> StyleResolution {
    resolve_with_user_stylesheets_and_environment(document, stylesheets, &[], environment)
}

/// Resolve collected author styles alongside user-origin CSS sources, in order.
///
/// Normal author declarations override normal user declarations; user
/// `!important` declarations override even important inline author styles.
/// User sources use syntax recovery, with diagnostics carrying no DOM node.
pub fn resolve_with_user_stylesheets(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
) -> StyleResolution {
    resolve_with_user_stylesheets_and_environment(
        document,
        stylesheets,
        user_stylesheets,
        MediaEnvironment::default(),
    )
}

/// The environment-aware variant of [`resolve_with_user_stylesheets`].
pub fn resolve_with_user_stylesheets_and_environment(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    environment: MediaEnvironment,
) -> StyleResolution {
    resolve_internal::<2>(
        document,
        stylesheets,
        user_stylesheets,
        user_agent_rules(),
        environment,
    )
}

/// Resolve a snapshot with explicit media and interaction state. The caller
/// retains collected ASTs and rebuilds layout/paint when either context changes.
pub fn resolve_with_style_environment(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    environment: MediaEnvironment,
    state: &InteractionState,
) -> StyleResolution {
    resolve_internal_state::<2>(
        document,
        stylesheets,
        user_stylesheets,
        user_agent_rules(),
        environment,
        state,
    )
}

/// Second style phase: evaluate container conditions/units against completed
/// ancestor content geometry. Callers must iterate layout until snapshots agree.
/// This API never loads resources; an empty snapshot is the bootstrap phase.
pub fn resolve_with_container_environment(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    environment: MediaEnvironment,
    state: &InteractionState,
    containers: &ContainerSnapshot,
) -> StyleResolution {
    resolve_internal_container_state::<2>(
        document,
        stylesheets,
        user_stylesheets,
        user_agent_rules(),
        environment,
        state,
        containers,
    )
}

/// Reference modes are deliberately opt-in, not part of the styling API.
#[cfg(feature = "style-bench-reference")]
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum BenchmarkMatching {
    Unindexed,
    Indexed,
    IndexedWithBloom,
}

#[cfg(feature = "style-bench-reference")]
#[doc(hidden)]
pub fn resolve_for_benchmark(document: &Document, matching: BenchmarkMatching) -> StyleResolution {
    resolve_for_benchmark_with_environment(document, matching, MediaEnvironment::default())
}

#[cfg(feature = "style-bench-reference")]
#[doc(hidden)]
pub fn resolve_for_benchmark_with_environment(
    document: &Document,
    matching: BenchmarkMatching,
    environment: MediaEnvironment,
) -> StyleResolution {
    resolve_for_benchmark_with_stylesheets(
        document,
        collect_stylesheets(document),
        matching,
        environment,
    )
}

#[cfg(feature = "style-bench-reference")]
#[doc(hidden)]
pub fn resolve_for_benchmark_with_stylesheets(
    document: &Document,
    sheets: StylesheetCollection,
    matching: BenchmarkMatching,
    environment: MediaEnvironment,
) -> StyleResolution {
    match matching {
        BenchmarkMatching::Unindexed => {
            resolve_internal::<0>(document, sheets, &[], user_agent_rules(), environment)
        }
        BenchmarkMatching::Indexed => {
            resolve_internal::<1>(document, sheets, &[], user_agent_rules(), environment)
        }
        BenchmarkMatching::IndexedWithBloom => {
            resolve_internal::<2>(document, sheets, &[], user_agent_rules(), environment)
        }
    }
}

// 0 = unindexed reference, 1 = index only, 2 = production index + Bloom.
#[cfg(feature = "style-bench-reference")]
#[doc(hidden)]
pub fn resolve_for_benchmark_with_containers(
    document: &Document,
    sheets: StylesheetCollection,
    users: &[&str],
    matching: BenchmarkMatching,
    environment: MediaEnvironment,
    state: &InteractionState,
    containers: &ContainerSnapshot,
) -> StyleResolution {
    match matching {
        BenchmarkMatching::Unindexed => resolve_internal_container_state::<0>(
            document,
            sheets,
            users,
            user_agent_rules(),
            environment,
            state,
            containers,
        ),
        BenchmarkMatching::Indexed => resolve_internal_container_state::<1>(
            document,
            sheets,
            users,
            user_agent_rules(),
            environment,
            state,
            containers,
        ),
        BenchmarkMatching::IndexedWithBloom => resolve_internal_container_state::<2>(
            document,
            sheets,
            users,
            user_agent_rules(),
            environment,
            state,
            containers,
        ),
    }
}

// Const modes remove reference branches and ancestry maintenance from mode 0.
fn resolve_internal<const MODE: u8>(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    user_agent: &StyledSheet,
    environment: MediaEnvironment,
) -> StyleResolution {
    resolve_internal_state::<MODE>(
        document,
        stylesheets,
        user_stylesheets,
        user_agent,
        environment,
        &InteractionState::default(),
    )
}

fn resolve_internal_state<const MODE: u8>(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    user_agent: &StyledSheet,
    environment: MediaEnvironment,
    state: &InteractionState,
) -> StyleResolution {
    resolve_internal_container_state::<MODE>(
        document,
        stylesheets,
        user_stylesheets,
        user_agent,
        environment,
        state,
        &ContainerSnapshot::default(),
    )
}

fn resolve_internal_container_state<const MODE: u8>(
    document: &Document,
    stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    user_agent: &StyledSheet,
    environment: MediaEnvironment,
    state: &InteractionState,
    containers: &ContainerSnapshot,
) -> StyleResolution {
    resolve_internal_fonts::<MODE>(
        document,
        stylesheets,
        user_stylesheets,
        user_agent,
        environment,
        state,
        containers,
        &crate::text::FontContext::bundled(),
    )
}

/// Explicit shared font environment for production browsing and injected tests.
pub fn resolve_with_fonts(
    document: &Document,
    stylesheets: StylesheetCollection,
    environment: MediaEnvironment,
    state: &InteractionState,
    containers: &ContainerSnapshot,
    fonts: &crate::text::FontContext,
) -> StyleResolution {
    resolve_internal_fonts::<2>(
        document,
        stylesheets,
        &[],
        user_agent_rules(),
        environment,
        state,
        containers,
        fonts,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_internal_fonts<const MODE: u8>(
    document: &Document,
    mut stylesheets: StylesheetCollection,
    user_stylesheets: &[&str],
    user_agent: &StyledSheet,
    environment: MediaEnvironment,
    state: &InteractionState,
    containers: &ContainerSnapshot,
    fonts: &crate::text::FontContext,
) -> StyleResolution {
    let mut environment = environment;
    let initial_size = if environment.initial_font_size.is_finite() {
        environment.initial_font_size.clamp(1.0, MAX_FONT_SIZE)
    } else {
        16.0
    };
    let initial_face = fonts.select(&ComputedStyle::default());
    let units = initial_face.units(initial_size);
    environment.initial_font_metrics = Some([
        units.ch,
        units.ex,
        units.cap,
        units.ic,
        initial_face
            .horizontal_line_metrics(initial_size)
            .map_or(initial_size * 1.2, |m| m.new_line_size),
    ]);
    let declaration_context = DeclarationContext::for_document(document);
    let mut user_layers = Layers::default();
    let user_rules = user_stylesheets
        .iter()
        .map(|source| {
            let parsed = css::parse_recovering(source);
            let mut diagnostics = Vec::new();
            report_css_errors(&parsed, None, "", &mut diagnostics);
            let rules = Compiler::<MODE>::new(
                None,
                None,
                &mut diagnostics,
                environment,
                &mut user_layers,
                declaration_context,
            )
            .compile(&parsed.value, true);
            append_sheet_diagnostics(
                &mut stylesheets.diagnostics,
                diagnostics,
                "user stylesheet: ",
            );
            rules
        })
        .collect::<Vec<_>>();
    let user_rules = rank_layers(user_rules, &user_layers);
    let mut author_layers = Layers::default();
    let author_rules = stylesheets
        .stylesheets
        .iter()
        .map(|sheet| {
            let mut diagnostics = Vec::new();
            let media = document
                .node(sheet.node_id)
                .and_then(|node| node.as_element())
                .filter(|element| {
                    element.tag_name().eq_ignore_ascii_case("style")
                        || element.tag_name().eq_ignore_ascii_case("link")
                })
                .and_then(|element| element.attribute("media"));
            let active = media.is_none_or(|source| {
                media_matches(
                    source,
                    environment,
                    Some(sheet.node_id),
                    &mut diagnostics,
                    true,
                )
            });
            let mut compiler = Compiler::<MODE>::new(
                Some(sheet.node_id),
                document.node(sheet.node_id).and_then(|node| node.parent()),
                &mut diagnostics,
                environment,
                &mut author_layers,
                declaration_context,
            );
            compiler.url = sheet.url.clone();
            let rules = compiler.compile(&sheet.stylesheet, active);
            let prefix = sheet
                .url
                .as_ref()
                .map(|url| format!("{url}: "))
                .unwrap_or_default();
            append_sheet_diagnostics(&mut stylesheets.diagnostics, diagnostics, &prefix);
            rules
        })
        .collect::<Vec<_>>();
    let author_rules = rank_layers(author_rules, &author_layers);
    let has_pseudos = std::iter::once(user_agent)
        .chain(user_rules.iter())
        .chain(author_rules.iter())
        .any(|sheet| {
            sheet.rules.iter().any(|rule| {
                rule.active
                    && rule
                        .selectors
                        .iter()
                        .any(|styled| styled.selector.pseudo_element().is_some())
            })
        });
    let mut registered_candidates = BTreeMap::new();
    for (origin, sheets) in [(Origin::User, &user_rules), (Origin::Author, &author_rules)] {
        for (sheet_order, sheet) in sheets.iter().enumerate() {
            for (order, rule) in sheet.registrations.iter().enumerate() {
                let priority = (origin.level(false), rule.rank, sheet_order, order);
                if registered_candidates
                    .get(&rule.name)
                    .is_none_or(|(old, _)| priority >= *old)
                {
                    registered_candidates
                        .insert(rule.name.clone(), (priority, rule.registration.clone()));
                }
            }
        }
    }
    let registrations: Registrations = registered_candidates
        .into_iter()
        .map(|(name, (_, registration))| (name, registration))
        .collect();
    let mut nodes = HashMap::with_capacity(document.node_count());
    let mut pseudos = HashMap::new();
    let mut matching = MatchingContext {
        state: state.clone(),
        containers: containers.clone(),
        environment,
        ..MatchingContext::default()
    };
    let mut initial = ComputedNode::default();
    initial.style.font_face = fonts.select(&initial.style);
    initial.style.font_size =
        if environment.initial_font_size.is_finite() && environment.initial_font_size > 0.0 {
            environment.initial_font_size.min(MAX_FONT_SIZE)
        } else {
            16.0
        };
    let mut root_metrics = None;
    let mut pending = vec![(document.root_id(), false)];
    while let Some((node_id, exit)) = pending.pop() {
        let Some(node) = document.node(node_id) else {
            continue;
        };
        if MODE == 2 && exit {
            matching
                .ancestors
                .pop(node.as_element().expect("element exit"));
            continue;
        }
        let parent = node
            .parent()
            .filter(|&id| {
                !matches!(node.kind(), NodeKind::Element(_))
                    || document
                        .node(id)
                        .is_some_and(|node| node.as_element().is_some())
            })
            .and_then(|id| nodes.get(&id))
            .unwrap_or(&initial);
        let style = match node.kind() {
            NodeKind::Document | NodeKind::DocumentFragment => ComputedNode {
                style: ComputedStyle {
                    display: Display::Block,
                    ..initial.style.clone()
                },
                ..ComputedNode::default()
            },
            NodeKind::Text(_) => ComputedNode {
                style: ComputedStyle::inherit(&parent.style),
                ..ComputedNode::default()
            },
            NodeKind::Comment(_)
            | NodeKind::Doctype(_)
            | NodeKind::ProcessingInstruction { .. } => ComputedNode {
                style: ComputedStyle {
                    display: Display::None,
                    ..ComputedStyle::inherit(&parent.style)
                },
                ..ComputedNode::default()
            },
            NodeKind::Element(element) => {
                if MODE != 0 {
                    matching.keys.clear();
                    matching.keys.extend(element_keys(element));
                }
                let mut winners = Winners::default();
                apply_rules::<MODE>(
                    document,
                    node_id,
                    user_agent,
                    Origin::UserAgent,
                    0,
                    &mut winners,
                    &mut matching,
                );
                let direction = match element
                    .attribute("dir")
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                {
                    Some("ltr") => Some(super::Direction::Ltr),
                    Some("rtl") => Some(super::Direction::Rtl),
                    Some("auto") => Some(super::directionality::auto(document, node_id)),
                    _ if element.tag_name() == "bdi" => {
                        Some(super::directionality::auto(document, node_id))
                    }
                    _ => None,
                };
                if let Some(direction) = direction {
                    insert(
                        &mut winners,
                        &StyledDeclaration {
                            property: DeclarationProperty::Longhand(Property::Direction),
                            value: Value::Direction(direction),
                            important: false,
                            order: 0,
                        },
                        Priority {
                            level: CascadeLevel::UserAgent,
                            inline: false,
                            layer_rank: 0,
                            specificity: Specificity::default(),
                            proximity: std::cmp::Reverse(usize::MAX),
                            sheet: usize::MAX,
                            rule: 0,
                            declaration: 0,
                            layer: 0,
                            normal_rank: 0,
                        },
                    );
                }
                for (attribute, property) in
                    [("width", Property::Width), ("height", Property::Height)]
                {
                    if !matches!(element.tag_name(), "img" | "video" | "iframe") {
                        break;
                    }
                    let Some(dimension) = element
                        .attribute(attribute)
                        .and_then(crate::html::attributes::presentation_dimension)
                    else {
                        continue;
                    };
                    insert(
                        &mut winners,
                        &StyledDeclaration {
                            property: DeclarationProperty::Longhand(property),
                            value: Value::Length(dimension),
                            important: false,
                            order: 0,
                        },
                        Priority {
                            level: CascadeLevel::Presentational,
                            inline: false,
                            layer_rank: author_layers.ranks().len() - 1,
                            specificity: Specificity::default(),
                            proximity: std::cmp::Reverse(usize::MAX),
                            sheet: 0,
                            rule: 0,
                            declaration: 0,
                            layer: 0,
                            normal_rank: author_layers.ranks().len() - 1,
                        },
                    );
                }
                for (sheet, rules) in user_rules.iter().enumerate() {
                    apply_rules::<MODE>(
                        document,
                        node_id,
                        rules,
                        Origin::User,
                        sheet,
                        &mut winners,
                        &mut matching,
                    );
                }
                for (sheet, rules) in author_rules.iter().enumerate() {
                    apply_rules::<MODE>(
                        document,
                        node_id,
                        rules,
                        Origin::Author,
                        sheet,
                        &mut winners,
                        &mut matching,
                    );
                }
                if let Some(source) = element.attribute("style") {
                    let parsed = css::parse_declarations_recovering(source);
                    report_css_errors(
                        &parsed,
                        node_id,
                        "inline style: ",
                        &mut stylesheets.diagnostics,
                    );
                    for (order, declaration) in parsed.value.iter().enumerate() {
                        match declarations::parse_with_context(
                            declaration,
                            order,
                            declaration_context,
                        ) {
                            Ok(declarations) => {
                                for declaration in &declarations {
                                    insert(
                                        &mut winners,
                                        declaration,
                                        Priority {
                                            level: Origin::Author.level(declaration.important),
                                            inline: true,
                                            layer_rank: 0,
                                            specificity: Specificity::default(),
                                            proximity: std::cmp::Reverse(usize::MAX),
                                            sheet: 0,
                                            rule: 0,
                                            declaration: order,
                                            layer: 0,
                                            normal_rank: 0,
                                        },
                                    );
                                }
                            }
                            Err(message) => declaration_error(
                                declaration,
                                Some(node_id),
                                message,
                                &mut stylesheets.diagnostics,
                            ),
                        }
                    }
                }
                let context = containers.unit_context(
                    document,
                    node_id,
                    false,
                    length_context(&parent.style, environment, root_metrics),
                );
                let is_root = root_metrics.is_none();
                let result = compute_with_context(
                    parent,
                    &winners,
                    node_id,
                    &mut stylesheets.diagnostics,
                    ComputationContext {
                        lengths: context,
                        is_root,
                        initial_font_size: initial.style.font_size,
                        registrations: &registrations,
                        fonts,
                    },
                );
                let mut result = result;
                if parent.style.display == Display::Contents && !result.style.position.out_of_flow()
                {
                    let mut ancestor = node.parent();
                    while let Some(id) = ancestor {
                        if let Some(computed) = nodes.get(&id)
                            && computed.style.display != Display::Contents
                        {
                            if computed.style.display.is_item_container() {
                                result.style.display = result.style.display.blockified();
                            }
                            break;
                        }
                        ancestor = document.node(id).and_then(|n| n.parent());
                    }
                }
                if let Some(language) = element.attribute("lang").filter(|s| s.len() <= 64) {
                    result.style.font_language = Some(language.to_owned());
                }
                if is_root {
                    root_metrics = Some(RootFontMetrics::new(&result.style));
                }
                // Only visit targets actually present in candidate rules. Index
                // keys still describe the originating element, not generated boxes.
                let mut targets = [false; 8];
                for rules in std::iter::once(user_agent)
                    .chain(user_rules.iter())
                    .chain(author_rules.iter())
                    .filter(|_| has_pseudos)
                {
                    if MODE != 0 {
                        rules.index.collect(
                            &matching.keys,
                            &mut matching.candidates,
                            rules.rules.len(),
                        );
                    }
                    let candidates: Vec<_> = if MODE == 0 {
                        (0..rules.rules.len()).collect()
                    } else {
                        matching.candidates.rules.clone()
                    };
                    for candidate in candidates {
                        let rule = &rules.rules[candidate];
                        if rule.active {
                            for styled in &rule.selectors {
                                if let Some(pseudo) = styled.selector.pseudo_element() {
                                    targets[pseudo as usize] = true;
                                }
                            }
                        }
                    }
                }
                for pseudo in PseudoElement::ALL {
                    if !targets[pseudo as usize] {
                        continue;
                    }
                    matching.pseudo = Some(pseudo);
                    let mut pseudo_winners = Winners::default();
                    apply_rules::<MODE>(
                        document,
                        node_id,
                        user_agent,
                        Origin::UserAgent,
                        0,
                        &mut pseudo_winners,
                        &mut matching,
                    );
                    for (origin, rules) in
                        [(Origin::User, &user_rules), (Origin::Author, &author_rules)]
                    {
                        for (sheet, rules) in rules.iter().enumerate() {
                            apply_rules::<MODE>(
                                document,
                                node_id,
                                rules,
                                origin,
                                sheet,
                                &mut pseudo_winners,
                                &mut matching,
                            );
                        }
                    }
                    if !pseudo_winners.candidates.is_empty()
                        || !pseudo_winners.custom_candidates.is_empty()
                    {
                        let computed = compute_with_context(
                            &result,
                            &pseudo_winners,
                            node_id,
                            &mut stylesheets.diagnostics,
                            ComputationContext {
                                lengths: containers.unit_context(
                                    document,
                                    node_id,
                                    true,
                                    length_context(&result.style, environment, root_metrics),
                                ),
                                is_root: false,
                                initial_font_size: initial.style.font_size,
                                registrations: &registrations,
                                fonts,
                            },
                        );
                        pseudos.insert((node_id, pseudo), computed.style);
                    }
                }
                matching.pseudo = None;
                result
            }
        };
        nodes.insert(node_id, style);
        if MODE == 2
            && !node.children().is_empty()
            && let Some(element) = node.as_element()
        {
            matching.ancestors.push(element);
            pending.push((node_id, true));
        }
        pending.extend(node.children().iter().rev().map(|&id| (id, false)));
    }
    stylesheets.diagnostics.extend(matching.diagnostics.entries);
    let mut counter_styles = css::counter_style::CounterStyles::default();
    for sheets in [&user_rules, &author_rules] {
        let mut candidates = sheets
            .iter()
            .enumerate()
            .flat_map(|(sheet, s)| {
                s.counters
                    .iter()
                    .enumerate()
                    .map(move |(order, (c, _, rank))| ((*rank, sheet, order), c))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(priority, _)| *priority);
        for (_, c) in candidates {
            counter_styles.insert(c.clone());
        }
    }
    StyleResolution {
        styles: ComputedStyles {
            counter_styles: Arc::new(counter_styles),
            pseudos,
            nodes: nodes
                .into_iter()
                .map(|(id, node)| (id, node.style))
                .collect(),
        },
        stylesheets,
    }
}

fn user_agent_rules() -> &'static StyledSheet {
    static RULES: OnceLock<StyledSheet> = OnceLock::new();
    RULES.get_or_init(|| {
        let sheet =
            css::parse(include_str!("user_agent.css")).expect("bundled user-agent CSS must parse");
        let mut diagnostics = Vec::new();
        let rules = compile::<2>(
            &sheet,
            None,
            &mut diagnostics,
            MediaEnvironment::default(),
            true,
        );
        assert!(
            diagnostics.is_empty(),
            "invalid bundled user-agent CSS: {diagnostics:?}"
        );
        rules
    })
}

fn compile<const MODE: u8>(
    sheet: &Stylesheet,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
    environment: MediaEnvironment,
    active: bool,
) -> StyledSheet {
    let mut layers = Layers::default();
    let result = Compiler::<MODE>::new(
        node_id,
        None,
        diagnostics,
        environment,
        &mut layers,
        DeclarationContext::default(),
    )
    .compile(sheet, active);
    rank_layers(vec![result], &layers).pop().unwrap()
}

fn rank_layers(mut sheets: Vec<StyledSheet>, layers: &Layers) -> Vec<StyledSheet> {
    let ranks = layers.ranks();
    for sheet in &mut sheets {
        for rule in &mut sheet.rules {
            rule.layer_rank = ranks[rule.layer];
            rule.layer_count = ranks.len();
        }
        for registration in &mut sheet.registrations {
            registration.rank = ranks[registration.layer];
        }
        for font in &mut sheet.fonts {
            font.rank = ranks[font.layer];
        }
        for (_, layer, rank) in &mut sheet.counters {
            *rank = ranks[*layer];
        }
    }
    sheets
}

struct Compiler<'a, const MODE: u8> {
    node_id: Option<NodeId>,
    implicit_scope: Option<NodeId>,
    diagnostics: &'a mut Vec<Diagnostic>,
    environment: MediaEnvironment,
    layers: &'a mut Layers,
    namespaces: selectors::Namespaces,
    result: StyledSheet,
    containers: Vec<ContainerQuery>,
    declaration_context: DeclarationContext,
    url: Option<url::Url>,
}

impl<'a, const MODE: u8> Compiler<'a, MODE> {
    fn new(
        node_id: Option<NodeId>,
        implicit_scope: Option<NodeId>,
        diagnostics: &'a mut Vec<Diagnostic>,
        environment: MediaEnvironment,
        layers: &'a mut Layers,
        declaration_context: DeclarationContext,
    ) -> Self {
        Self {
            node_id,
            implicit_scope,
            diagnostics,
            environment,
            layers,
            namespaces: selectors::Namespaces::default(),
            result: StyledSheet::default(),
            containers: Vec::new(),
            declaration_context,
            url: None,
        }
    }

    fn compile(mut self, sheet: &Stylesheet, active: bool) -> StyledSheet {
        self.sheet(sheet, active, 0, &[]);
        self.result
    }

    fn diagnostic(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            node_id: self.node_id,
            message: message.into(),
        });
    }

    fn sheet(&mut self, sheet: &Stylesheet, active: bool, layer: usize, scopes: &[Scope]) {
        let mut imports = true;
        let mut namespaces = true;
        let mut preamble_started = false;
        for rule in sheet.rules() {
            if let Rule::At(at) = rule {
                if control::is_statement(at, "charset") {
                    continue;
                }
                if at.name().eq_ignore_ascii_case("import") {
                    if !imports || at.block().is_some() {
                        self.diagnostic("@import must precede style rules and @namespace");
                        continue;
                    }
                    self.import(at, active, layer, scopes);
                    preamble_started = true;
                    continue;
                }
                if at.name().eq_ignore_ascii_case("namespace") {
                    imports = false;
                    preamble_started = true;
                    if !namespaces
                        || at.block().is_some()
                        || !control::namespace_rule(at.prelude(), &mut self.namespaces)
                    {
                        self.diagnostic("invalid or misplaced @namespace");
                    }
                    continue;
                }
                if control::is_statement(at, "layer") {
                    if preamble_started {
                        imports = false;
                        namespaces = false;
                    }
                    self.rule(rule, active, layer, scopes, None);
                    continue;
                }
            }
            imports = false;
            namespaces = false;
            self.rule(rule, active, layer, scopes, None);
        }
    }

    fn import(&mut self, rule: &css::AtRule, active: bool, layer: usize, scopes: &[Scope]) {
        let Some(import) = css::control::import(rule.prelude()) else {
            self.diagnostic("invalid @import prelude");
            return;
        };
        let condition = import.supports.as_ref().is_none_or(|source| {
            control::supports_with_context(
                source,
                &self.namespaces,
                self.node_id,
                self.diagnostics,
                self.declaration_context,
            )
        });
        let media = media_matches(
            &import.media,
            self.environment,
            self.node_id,
            self.diagnostics,
            true,
        );
        let layer = if active {
            import
                .layer
                .as_ref()
                .map_or(layer, |path| self.layers.register(layer, path))
        } else {
            layer
        };
        if !condition {
            return;
        }
        let Some(loaded) = rule.imported() else {
            self.diagnostic(format!("@import {:?} has no loaded stylesheet", import.url));
            return;
        };
        let previous = std::mem::take(&mut self.namespaces);
        let previous_url = self.url.replace(loaded.url.clone());
        let start = self.diagnostics.len();
        self.sheet(
            &loaded.stylesheet,
            active && condition && media,
            layer,
            scopes,
        );
        for diagnostic in &mut self.diagnostics[start..] {
            diagnostic.message = format!("{}: {}", loaded.url, diagnostic.message);
        }
        self.namespaces = previous;
        self.url = previous_url;
    }

    fn rule(
        &mut self,
        rule: &Rule,
        active: bool,
        layer: usize,
        scopes: &[Scope],
        parent: Option<Arc<[Selector]>>,
    ) {
        match rule {
            Rule::Qualified(rule) => {
                let selectors = match selectors::parse_scoped(
                    rule.prelude(),
                    &self.namespaces,
                    parent,
                    !scopes.is_empty(),
                ) {
                    Ok(value) => Arc::<[Selector]>::from(value),
                    Err(error) => {
                        self.diagnostic(format!("{}: {error}", rule.prelude()));
                        return;
                    }
                };
                self.items(rule.items(), active, layer, scopes, Some(selectors));
            }
            Rule::At(rule) => {
                let name = rule.name().to_ascii_lowercase();
                if name == "container" {
                    let query = ContainerQuery::parse(rule.prelude());
                    for message in query.diagnostics() {
                        self.diagnostic(format!(
                            "container query {:?}: {}",
                            rule.prelude(),
                            message.replace("media", "container")
                        ));
                    }
                    let Some(block) = rule.block() else {
                        self.diagnostic("@container requires a rule block");
                        return;
                    };
                    let valid = query.valid();
                    self.containers.push(query);
                    self.items(block, active && valid, layer, scopes, parent);
                    self.containers.pop();
                    return;
                }
                let mut next_layer = layer;
                let mut next_scopes = scopes.to_vec();
                let mut next_active = active;
                match name.as_str() {
                    "counter-style" => {
                        if parent.is_some() || !self.containers.is_empty() || !scopes.is_empty() {
                            self.diagnostic(
                                "@counter-style inside style/scope/container rules is unsupported",
                            );
                            return;
                        }
                        match css::counter_style::CounterStyle::parse(rule) {
                            Ok((style, messages)) => {
                                for message in messages {
                                    self.diagnostic(message);
                                }
                                if active && self.result.counters.len() < 256 {
                                    self.result.counters.push((style, layer, 0));
                                } else if active {
                                    self.diagnostic("@counter-style rule count exceeds 256");
                                }
                            }
                            Err(message) => self.diagnostic(message),
                        }
                        return;
                    }
                    "font-face" => {
                        if parent.is_some() || !self.containers.is_empty() || !scopes.is_empty() {
                            self.diagnostic("@font-face is not supported inside style, scope or container rules");
                            return;
                        }
                        let (descriptor, messages) = css::font_face::parse(rule);
                        for message in messages {
                            self.diagnostic(message);
                        }
                        if let Some(descriptor) = descriptor
                            && self.result.fonts.len() < 256
                        {
                            self.result.fonts.push(FontRule {
                                descriptor,
                                node_id: self.node_id,
                                url: self.url.clone(),
                                active,
                                layer,
                                rank: 0,
                            });
                        }
                        return;
                    }
                    "media" => {
                        next_active &= media_matches(
                            rule.prelude(),
                            self.environment,
                            self.node_id,
                            self.diagnostics,
                            false,
                        )
                    }
                    "supports" => {
                        next_active &= control::supports_with_context(
                            rule.prelude(),
                            &self.namespaces,
                            self.node_id,
                            self.diagnostics,
                            self.declaration_context,
                        )
                    }
                    "layer" => {
                        let paths = if rule.prelude().trim().is_empty() && rule.block().is_some() {
                            Some(vec![Vec::new()])
                        } else {
                            css::control::layer_names(rule.prelude())
                        };
                        let Some(paths) = paths else {
                            self.diagnostic("invalid @layer name or order statement");
                            return;
                        };
                        if rule.block().is_some() && paths.len() != 1 {
                            self.diagnostic("@layer block requires one layer name");
                            return;
                        }
                        for path in paths {
                            if active {
                                next_layer = self.layers.register(layer, &path);
                            }
                        }
                        if rule.block().is_none() {
                            return;
                        }
                    }
                    "scope" => {
                        let Some(scope) = Scope::parse(
                            rule.prelude(),
                            &self.namespaces,
                            self.implicit_scope,
                            !scopes.is_empty(),
                            parent.clone(),
                        ) else {
                            self.diagnostic("invalid or unsupported @scope prelude");
                            return;
                        };
                        next_scopes.push(scope);
                    }
                    "import" | "namespace" => {
                        self.diagnostic(format!("@{name} is not allowed in a nested block"));
                        return;
                    }
                    "property" => {
                        if parent.is_some() {
                            self.diagnostic("@property is not allowed inside a style rule");
                            return;
                        }
                        match Registration::parse(rule) {
                            Ok((name, registration)) if active => {
                                self.result.registrations.push(RegisteredRule {
                                    name,
                                    registration,
                                    layer,
                                    rank: 0,
                                });
                            }
                            Ok(_) => {}
                            Err(message) => self.diagnostic(message),
                        }
                        return;
                    }
                    _ => {
                        self.diagnostic(format!("unsupported at-rule: @{name}"));
                        return;
                    }
                }
                let Some(block) = rule.block() else {
                    self.diagnostic(format!("@{name} requires a rule block"));
                    return;
                };
                self.items(block, next_active, next_layer, &next_scopes, parent);
            }
        }
    }

    fn items(
        &mut self,
        items: &[BlockItem],
        active: bool,
        layer: usize,
        scopes: &[Scope],
        parent: Option<Arc<[Selector]>>,
    ) {
        let mut declarations = Vec::new();
        let declaration_parent = parent.clone().or_else(|| {
            (!scopes.is_empty()).then(|| {
                Arc::<[Selector]>::from(
                    selectors::parse_scoped("&", &self.namespaces, None, true).unwrap(),
                )
            })
        });
        for (order, item) in items.iter().enumerate() {
            match item {
                BlockItem::Declaration(declaration) if declaration_parent.is_some() => {
                    match declarations::parse_with_context(
                        declaration,
                        order,
                        self.declaration_context,
                    ) {
                        Ok(values) => declarations.extend(values),
                        Err(message) => {
                            declaration_error(declaration, self.node_id, message, self.diagnostics)
                        }
                    }
                }
                BlockItem::Declaration(_) => self
                    .diagnostic("declarations are not allowed directly inside a conditional rule"),
                BlockItem::Rule(rule) => {
                    if !declarations.is_empty() {
                        self.emit(
                            declaration_parent.as_ref().unwrap(),
                            std::mem::take(&mut declarations),
                            active,
                            layer,
                            scopes,
                        );
                    }
                    self.rule(rule, active, layer, scopes, parent.clone());
                }
            }
        }
        if !declarations.is_empty() {
            self.emit(
                declaration_parent.as_ref().unwrap(),
                declarations,
                active,
                layer,
                scopes,
            );
        }
    }

    fn emit(
        &mut self,
        selectors: &[Selector],
        mut declarations: Vec<StyledDeclaration>,
        active: bool,
        layer: usize,
        scopes: &[Scope],
    ) {
        let mut pending_bases = HashMap::new();
        for declaration in &mut declarations {
            if let Value::Effect(v) = &mut declaration.value
                && let Some(base) = &self.url
            {
                super::effects::resolve_urls(v, base);
            }
            if let Value::Pending(pending) = &mut declaration.value
                && self.url.is_some()
            {
                let updated = pending_bases
                    .entry(Arc::as_ptr(pending))
                    .or_insert_with(|| {
                        let mut value = pending.as_ref().clone();
                        value.base_url.clone_from(&self.url);
                        Arc::new(value)
                    });
                *pending = updated.clone();
            } else if let Value::Typography(typography::Value::Image(Some(reference))) =
                &mut declaration.value
                && let Some(base) = &self.url
                && let Ok(url) = base.join(reference)
            {
                *reference = url.into();
            }
        }
        for pseudo in PseudoElement::ALL {
            if matches!(
                pseudo,
                PseudoElement::FirstLine
                    | PseudoElement::FirstLetter
                    | PseudoElement::Placeholder
                    | PseudoElement::Selection
                    | PseudoElement::Backdrop
            ) && selectors.iter().any(|s| s.pseudo_element() == Some(pseudo))
            {
                self.diagnostic(format!(
                    "::{} computed style retained; layout/paint effect is not implemented",
                    pseudo.name()
                ));
            }
        }
        if selectors.iter().any(|s| s.pseudo_element() == Some(PseudoElement::Marker))
            && declarations.iter().any(|d| matches!(d.property, DeclarationProperty::Longhand(property) if             !matches!(property, Property::Color | Property::TextFillColor | Property::Content | Property::FontSize | Property::FontFamily | Property::FontWeight | Property::FontStyle | Property::LineHeight | Property::Typography(typography::Property::FontFeatures | typography::Property::FontVariations | typography::Property::FontOpticalSizing | typography::Property::FontKerning | typography::Property::FontLigatures | typography::Property::FontWidth | typography::Property::LetterSpacing | typography::Property::WordSpacing))))
        {
            self.diagnostic("::marker renders only content, color, and the supported font properties; other effects are not implemented");
        }
        let order = self.result.rules.len();
        if MODE != 0 {
            self.result.index.add_rule(order, selectors);
        }
        self.result.rules.push(StyledRule {
            selectors: selectors
                .iter()
                .cloned()
                .map(|selector| StyledSelector {
                    ancestors: if MODE == 2 {
                        selector.ancestor_requirements()
                    } else {
                        Bloom::default()
                    },
                    selector,
                })
                .collect(),
            declarations,
            order,
            active,
            layer,
            layer_rank: 0,
            layer_count: 1,
            scopes: scopes.to_vec(),
            containers: self.containers.clone(),
        });
    }
}

fn media_matches(
    source: &str,
    environment: MediaEnvironment,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
    allow_empty: bool,
) -> bool {
    let query = MediaQueryList::parse(source);
    diagnostics.extend(query.diagnostics().iter().map(|message| Diagnostic {
        node_id,
        message: format!("media query {source:?}: {message}"),
    }));
    diagnostics.extend(
        query
            .evaluation_diagnostics(environment)
            .into_iter()
            .map(|message| Diagnostic {
                node_id,
                message: format!("media query {source:?}: {message}"),
            }),
    );
    if !allow_empty && query.is_empty() {
        diagnostics.push(Diagnostic {
            node_id,
            message: "@media requires a nonempty query".into(),
        });
        false
    } else {
        query.matches(environment)
    }
}

fn declaration_error(
    declaration: &css::Declaration,
    node_id: Option<NodeId>,
    message: String,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if declaration.property().starts_with("--")
        || variables::contains_var(declaration.value_tokens())
    {
        let mut preview: String = message.chars().take(240).collect();
        if message.chars().count() > 240 {
            preview.push('…');
        }
        variables::report_message(
            diagnostics,
            node_id,
            format!("CSS variable declaration: {preview}"),
        );
    } else {
        diagnostics.push(Diagnostic { node_id, message });
    }
}

fn append_sheet_diagnostics(
    destination: &mut Vec<Diagnostic>,
    diagnostics: Vec<Diagnostic>,
    prefix: &str,
) {
    for mut diagnostic in diagnostics {
        let variable = diagnostic.message.starts_with("CSS variable declaration:")
            || diagnostic.message == "additional CSS variable diagnostics omitted";
        diagnostic.message = format!("{prefix}{}", diagnostic.message);
        if variable {
            variables::report_message(destination, diagnostic.node_id, diagnostic.message);
        } else {
            destination.push(diagnostic);
        }
    }
}

type LonghandWinners = HashMap<Property, (Priority, Value)>;

#[derive(Default)]
struct Winners {
    longhands: LonghandWinners,
    custom: BTreeMap<String, (Priority, Value)>,
    candidates: HashMap<Property, Vec<(Priority, Value)>>,
    custom_candidates: BTreeMap<String, Vec<(Priority, Value)>>,
}

fn apply_rules<const MODE: u8>(
    document: &Document,
    node_id: NodeId,
    sheet_rules: &StyledSheet,
    origin: Origin,
    sheet: usize,
    winners: &mut Winners,
    matching: &mut MatchingContext,
) {
    if MODE != 0 {
        sheet_rules.index.collect(
            &matching.keys,
            &mut matching.candidates,
            sheet_rules.rules.len(),
        );
    }
    let count = if MODE == 0 {
        sheet_rules.rules.len()
    } else {
        matching.candidates.rules.len()
    };
    for candidate in 0..count {
        let rule = &sheet_rules.rules[if MODE == 0 {
            candidate
        } else {
            matching.candidates.rules[candidate]
        }];
        if !rule.active {
            continue;
        }
        let mut container_matches = true;
        for query in &rule.containers {
            let (matches, invalid_math) = query.matches(
                document,
                node_id,
                matching.pseudo.is_some(),
                &matching.containers,
                matching.environment,
            );
            if invalid_math {
                let source: String = query.source.chars().take(240).collect();
                matching.diagnostics.query_error(node_id,
                    format!("container query {source:?}: non-finite query calculation; query does not match"));
            }
            container_matches &= matches;
        }
        if !container_matches {
            continue;
        }
        let specificity = |root, matching: &mut MatchingContext| {
            rule.selectors
                .iter()
                .filter(|styled| {
                    (MODE != 2 || matching.ancestors.may_contain(&styled.ancestors))
                        && matching.diagnostics.matched(
                            styled.selector.try_matches_target(
                                document,
                                node_id,
                                root,
                                &matching.state,
                                matching.pseudo,
                            ),
                            node_id,
                        )
                })
                .map(|styled| styled.selector.specificity())
                .max()
        };
        let matched = if rule.scopes.is_empty() {
            specificity(None, matching)
                .map(|specificity| (specificity, std::cmp::Reverse(usize::MAX)))
        } else {
            let mut scopes = vec![(None, usize::MAX)];
            'scopes: for scope in &rule.scopes {
                let mut seen = std::collections::HashSet::new();
                let mut next = Vec::new();
                for (outer, _) in std::mem::take(&mut scopes) {
                    match scope.matches(document, node_id, outer, &matching.state) {
                        Ok(roots) => {
                            next.extend(
                                roots
                                    .into_iter()
                                    .filter(|(root, _)| seen.insert(*root))
                                    .map(|(root, distance)| (Some(root), distance)),
                            );
                        }
                        Err(error) => {
                            matching.diagnostics.report(node_id, error);
                            break 'scopes;
                        }
                    }
                }
                scopes = next;
                if scopes.is_empty() {
                    break;
                }
            }
            scopes
                .into_iter()
                .filter_map(|(root, proximity)| {
                    specificity(root, matching)
                        .map(|specificity| (specificity, std::cmp::Reverse(proximity)))
                })
                .max()
        };
        let Some((specificity, proximity)) = matched else {
            continue;
        };
        for declaration in &rule.declarations {
            insert(
                winners,
                declaration,
                Priority {
                    level: origin.level(declaration.important),
                    inline: false,
                    layer_rank: if declaration.important {
                        rule.layer_count - 1 - rule.layer_rank
                    } else {
                        rule.layer_rank
                    },
                    specificity,
                    proximity,
                    sheet,
                    rule: rule.order,
                    declaration: declaration.order,
                    layer: rule.layer,
                    normal_rank: rule.layer_rank,
                },
            );
        }
    }
}

fn insert(winners: &mut Winners, declaration: &StyledDeclaration, priority: Priority) {
    match &declaration.property {
        DeclarationProperty::Longhand(property) => {
            winners
                .candidates
                .entry(*property)
                .or_default()
                .push((priority, declaration.value.clone()));
            if winners
                .longhands
                .get(property)
                .is_none_or(|(old, _)| priority >= *old)
            {
                winners
                    .longhands
                    .insert(*property, (priority, declaration.value.clone()));
            }
        }
        DeclarationProperty::Custom(name) => {
            winners
                .custom_candidates
                .entry(name.clone())
                .or_default()
                .push((priority, declaration.value.clone()));
            if winners
                .custom
                .get(name)
                .is_none_or(|(old, _)| priority >= *old)
            {
                winners
                    .custom
                    .insert(name.clone(), (priority, declaration.value.clone()));
            }
        }
    }
}

fn cascade_origin(level: CascadeLevel) -> u8 {
    match level {
        CascadeLevel::UserAgent | CascadeLevel::UserAgentImportant => 0,
        CascadeLevel::User | CascadeLevel::UserImportant => 1,
        CascadeLevel::Presentational | CascadeLevel::Author | CascadeLevel::AuthorImportant => 2,
    }
}

fn select_candidate(
    candidates: &[(Priority, Value)],
    mut substitute: impl FnMut(&Value) -> Value,
) -> Option<(Priority, Value)> {
    if let Some((priority, value)) = candidates.iter().max_by_key(|(priority, _)| *priority)
        && !matches!(
            value,
            Value::Wide(WideKeyword::Revert | WideKeyword::RevertLayer)
                | Value::Pending(_)
                | Value::Mapped(..)
        )
    {
        return Some((*priority, value.clone()));
    }
    let mut candidates = candidates.to_vec();
    candidates.sort_by_key(|(priority, _)| *priority);
    while let Some((priority, value)) = candidates.pop() {
        let value = substitute(&value);
        match value {
            Value::Wide(WideKeyword::Revert) => {
                let origin = cascade_origin(priority.level);
                candidates.retain(|(candidate, _)| cascade_origin(candidate.level) < origin);
            }
            Value::Wide(WideKeyword::RevertLayer) => {
                if priority.inline {
                    // Important inline rollback must preserve intervening
                    // important author stylesheet declarations.
                    candidates.retain(|(candidate, _)| {
                        cascade_origin(candidate.level) != cascade_origin(priority.level)
                            || !candidate.inline
                    });
                } else {
                    let normal_level = match cascade_origin(priority.level) {
                        0 => CascadeLevel::UserAgent,
                        1 => CascadeLevel::User,
                        _ => CascadeLevel::Author,
                    };
                    // Roll back the complete interval between this layer's
                    // normal and important levels, not merely its own rules.
                    candidates.retain(|(candidate, _)| {
                        (candidate.level, candidate.inline, candidate.layer_rank)
                            < (normal_level, false, priority.normal_rank)
                    });
                }
            }
            value => return Some((priority, value)),
        }
    }
    None
}

fn substitute_winners(
    winners: &Winners,
    environment: &variables::Environment,
    budget: &mut Budget,
    node_id: NodeId,
    diagnostics: &mut Vec<Diagnostic>,
) -> LonghandWinners {
    let mut pending: HashMap<_, Result<Vec<Value>, Failure>> = HashMap::new();
    let mut result = LonghandWinners::new();
    // Deterministic property traversal also makes resource-budget diagnostics
    // independent of HashMap iteration and matching optimization mode.
    let mut properties: Vec<_> = winners.longhands.keys().copied().collect();
    properties.sort();
    for property in properties {
        let winner = &winners.longhands[&property];
        if !matches!(
            winner.1,
            Value::Wide(WideKeyword::Revert | WideKeyword::RevertLayer)
                | Value::Pending(_)
                | Value::Mapped(..)
        ) {
            result.insert(property, winner.clone());
            continue;
        }
        let fallback = std::slice::from_ref(&winners.longhands[&property]);
        let candidates = winners
            .candidates
            .get(&property)
            .map(Vec::as_slice)
            .unwrap_or(fallback);
        let selected = select_candidate(candidates, |value| {
            let (original, value) = if let Value::Mapped(original, value) = value {
                (*original, value.as_ref())
            } else {
                (property, value)
            };
            let wrap = |value: Value| {
                if original != property
                    && !matches!(
                        value,
                        Value::Wide(WideKeyword::Revert | WideKeyword::RevertLayer)
                    )
                {
                    Value::Mapped(original, Box::new(value))
                } else {
                    value
                }
            };
            let Value::Pending(declaration) = value else {
                return wrap(value.clone());
            };
            let values = pending.entry(Arc::as_ptr(declaration)).or_insert_with(|| {
                let result = declaration
                    .value
                    .substitute(environment, budget)
                    .and_then(|tokens| {
                        declarations::parse_tokens_with_context(
                                &declaration.name,
                                &tokens,
                                declaration.context,
                            )
                            .map(|mut values| {
                                for value in &mut values {
                                    if let Value::Effect(v) = value
                                        && let Some(base) = &declaration.base_url
                                    {
                                        super::effects::resolve_urls(v, base);
                                    }
                                    if let Value::Typography(typography::Value::Image(Some(
                                        reference,
                                    ))) = value
                                        && let Some(base) = &declaration.base_url
                                        && let Ok(url) = base.join(reference)
                                    {
                                        *reference = url.into();
                                    }
                                }
                                values
                            })
                            .ok_or(Failure::Grammar)
                    });
                if let Err(failure) = result {
                    variables::report(diagnostics, node_id, &declaration.name, failure);
                }
                result
            });
            let index = declarations::properties(&declaration.name)
                .unwrap()
                .iter()
                .position(|candidate| *candidate == original)
                .unwrap();
            wrap(
                values
                    .as_ref()
                    .map(|values| values[index].clone())
                    .unwrap_or(Value::Wide(WideKeyword::Unset)),
            )
        });
        if let Some(selected) = selected {
            result.insert(property, selected);
        }
    }
    result
}

#[derive(Default)]
struct ComputedNode {
    style: ComputedStyle,
    colors: ComputedColors,
}

struct ComputedColors {
    background: Color,
    border: Edges<Color>,
    fill: Color,
}

impl Default for ComputedColors {
    fn default() -> Self {
        Self {
            background: Color::Transparent,
            border: Edges::all(Color::CurrentColor),
            fill: Color::CurrentColor,
        }
    }
}

#[cfg(test)]
fn compute(
    parent: &ComputedNode,
    winners: &Winners,
    node_id: NodeId,
    diagnostics: &mut Vec<Diagnostic>,
) -> ComputedNode {
    compute_with_context(
        parent,
        winners,
        node_id,
        diagnostics,
        ComputationContext {
            lengths: length_context(&parent.style, MediaEnvironment::default(), None),
            is_root: true,
            initial_font_size: 16.0,
            registrations: &Registrations::new(),
            fonts: &crate::text::FontContext::bundled(),
        },
    )
}

#[derive(Clone, Copy)]
pub(super) struct RootFontMetrics {
    font_size: f32,
    line_height: f32,
    units: crate::text::FontUnits,
    vertical: bool,
}

impl RootFontMetrics {
    pub(super) fn new(style: &ComputedStyle) -> Self {
        Self {
            font_size: style.font_size,
            line_height: style.used_line_height(),
            units: style.font_face.units_for_style(style),
            vertical: style.writing_mode.vertical(),
        }
    }
}

pub(super) fn length_context(
    style: &ComputedStyle,
    environment: MediaEnvironment,
    root: Option<RootFontMetrics>,
) -> LengthContext {
    let viewport = (environment.viewport_width, environment.viewport_height);
    let root = root.unwrap_or_else(|| {
        RootFontMetrics::new(&ComputedStyle {
            font_size: environment.initial_font_size,
            font_face: style.font_face.clone(),
            ..ComputedStyle::default()
        })
    });
    let mut context = LengthContext {
        vertical: style.writing_mode.vertical(),
        container_vertical: false,
        viewport_vertical: root.vertical,
        font_size: style.font_size,
        root_font_size: root.font_size,
        zero_advance: style.font_size * 0.5,
        x_height: style.font_size * 0.5,
        cap_height: style.font_size,
        ideographic_advance: style.font_size,
        root_zero_advance: root.units.ch,
        root_x_height: root.units.ex,
        root_cap_height: root.units.cap,
        root_ideographic_advance: root.units.ic,
        line_height: style.used_line_height(),
        root_line_height: root.line_height,
        small_viewport: environment.small_viewport.unwrap_or(viewport),
        large_viewport: environment.large_viewport.unwrap_or(viewport),
        dynamic_viewport: environment.dynamic_viewport.unwrap_or(viewport),
        container_width: None,
        container_height: None,
        container_inline: None,
        container_block: None,
    };
    set_selected_metrics(&mut context, style);
    context
}

fn set_selected_metrics(context: &mut LengthContext, style: &ComputedStyle) {
    let units = style.font_face.units_for_style(&ComputedStyle {
        font_size: context.font_size,
        ..style.clone()
    });
    context.zero_advance = units.ch;
    context.x_height = units.ex;
    context.cap_height = units.cap;
    context.ideographic_advance = units.ic;
}

struct ComputationContext<'a> {
    lengths: LengthContext,
    is_root: bool,
    initial_font_size: f32,
    registrations: &'a Registrations,
    fonts: &'a crate::text::FontContext,
}

fn compute_with_context(
    parent: &ComputedNode,
    winners: &Winners,
    node_id: NodeId,
    diagnostics: &mut Vec<Diagnostic>,
    computation: ComputationContext<'_>,
) -> ComputedNode {
    let ComputationContext {
        lengths: mut context,
        is_root,
        initial_font_size,
        registrations,
        fonts,
    } = computation;
    let mut budget = Budget::default();
    let custom = winners
        .custom
        .iter()
        .filter_map(|(name, winner)| {
            let candidates = winners
                .custom_candidates
                .get(name)
                .map(Vec::as_slice)
                .unwrap_or(std::slice::from_ref(winner));
            select_candidate(candidates, Clone::clone).map(|(_, value)| (name.clone(), value))
        })
        .collect();
    let environment = variables::resolve(
        &parent.style.custom_properties,
        &custom,
        &mut budget,
        node_id,
        diagnostics,
        registrations,
    );
    let mut axis_winners = Winners::default();
    for property in [
        Property::Direction,
        Property::WritingMode,
        Property::TextOrientation,
    ] {
        if let Some(winner) = winners.longhands.get(&property) {
            axis_winners.longhands.insert(property, winner.clone());
        }
        if let Some(candidates) = winners.candidates.get(&property) {
            axis_winners.candidates.insert(property, candidates.clone());
        }
    }
    let axes = substitute_winners(
        &axis_winners,
        &environment,
        &mut budget,
        node_id,
        diagnostics,
    );
    let initial = ComputedStyle {
        font_size: initial_font_size,
        ..ComputedStyle::default()
    };
    let mut style = ComputedStyle::inherit(&parent.style);
    for property in [
        Property::Direction,
        Property::WritingMode,
        Property::TextOrientation,
    ] {
        if let Some((_, value)) = axes.get(&property) {
            apply(
                &mut style,
                &parent.style,
                property,
                computed_value(value, property, parent, &initial),
            );
        }
    }
    context.vertical = style.writing_mode.vertical();
    if is_root {
        context.viewport_vertical = context.vertical;
    }
    let mut physical = Winners::default();
    for (&property, candidates) in &winners.candidates {
        for (priority, value) in candidates {
            let mapped = property.physical(&style);
            let value = if mapped != property {
                Value::Mapped(property, Box::new(value.clone()))
            } else {
                value.clone()
            };
            physical
                .candidates
                .entry(mapped)
                .or_default()
                .push((*priority, value.clone()));
            if physical
                .longhands
                .get(&mapped)
                .is_none_or(|(old, _)| priority >= old)
            {
                physical.longhands.insert(mapped, (*priority, value));
            }
        }
    }
    // Some test/benchmark callers construct winners without candidate history.
    for (&property, (priority, value)) in &winners.longhands {
        if !winners.candidates.contains_key(&property) {
            let mapped = property.physical(&style);
            let value = if mapped != property {
                Value::Mapped(property, Box::new(value.clone()))
            } else {
                value.clone()
            };
            physical
                .candidates
                .entry(mapped)
                .or_default()
                .push((*priority, value.clone()));
            if physical
                .longhands
                .get(&mapped)
                .is_none_or(|(old, _)| priority >= old)
            {
                physical.longhands.insert(mapped, (*priority, value));
            }
        }
    }
    let winners = substitute_winners(&physical, &environment, &mut budget, node_id, diagnostics);
    style.custom_properties = environment;
    style.border_width = Edges::all(3.0);
    let mut colors = ComputedColors {
        fill: parent.colors.fill.clone(),
        ..ComputedColors::default()
    };
    for (&property, (_, value)) in &winners {
        let value = if let Value::Mapped(_, inner) = value {
            inner.as_ref()
        } else {
            value
        };
        if matches!(property, Property::Margin(_)) {
            let margin = match value {
                Value::Auto => Some(crate::css::values::LengthOrAuto::Auto),
                Value::Length(length) => Some(length.clone().into()),
                _ => None,
            };
            if let Some(margin) = margin {
                style
                    .specified_margins
                    .insert(format!("{property:?}"), margin);
            }
        }
        if let Value::Color(color) = value {
            style
                .specified_colors
                .insert(format!("{property:?}"), color.clone());
        }
        let length = match value {
            Value::Length(length) | Value::LineHeight(LineHeight::Length(length)) => Some(length),
            _ => None,
        };
        if let Some(length) = length {
            style
                .specified_lengths
                .insert(format!("{property:?}"), SpecifiedLength(length.clone()));
        }
    }
    // Resolve dependent values in CSS order, not hash-table iteration order.
    if let Some((_, value)) = winners.get(&Property::FontSize) {
        let value = computed_value(value, Property::FontSize, parent, &initial);
        if let Some(value) = absolutize(value, Property::FontSize, &context, parent.style.font_size)
        {
            if let Value::Length(length) = &value
                && length.resolve(parent.style.font_size, parent.style.font_size) > MAX_FONT_SIZE
            {
                diagnostics.push(Diagnostic {
                    node_id: Some(node_id),
                    message: format!(
                        "font-size exceeds the {MAX_FONT_SIZE}px rasterization limit; clamped"
                    ),
                });
            }
            apply(&mut style, &parent.style, Property::FontSize, value);
        } else {
            invalid_math(diagnostics, node_id, Property::FontSize);
        }
    }
    context.font_size = style.font_size;
    for property in [
        Property::FontFamily,
        Property::FontWeight,
        Property::FontStyle,
    ] {
        if let Some((_, value)) = winners.get(&property) {
            let value = computed_value(value, property, parent, &initial);
            if let Some(value) = absolutize(value, property, &context, style.font_size) {
                apply(&mut style, &parent.style, property, value);
            } else {
                invalid_math(diagnostics, node_id, property);
            }
        }
    }
    for p in [
        super::typography::Property::FontWidth,
        super::typography::Property::FontFeatures,
        super::typography::Property::FontVariations,
        super::typography::Property::FontOpticalSizing,
        super::typography::Property::FontKerning,
        super::typography::Property::FontLigatures,
    ] {
        let property = Property::Typography(p);
        if let Some((_, value)) = winners.get(&property) {
            apply(
                &mut style,
                &parent.style,
                property,
                computed_value(value, property, parent, &initial),
            );
        }
    }
    style.font_face = fonts.select(&style);
    set_selected_metrics(&mut context, &style);
    if is_root {
        context.root_font_size = style.font_size;
        context.root_zero_advance = context.zero_advance;
        context.root_x_height = context.x_height;
        context.root_cap_height = context.cap_height;
        context.root_ideographic_advance = context.ideographic_advance;
    }
    // lh/rlh in line-height refer to the parent/initial line-height, avoiding
    // cycles; box properties then use this element's completed line-height.
    if let Some((_, value)) = winners.get(&Property::LineHeight) {
        let value = computed_value(value, Property::LineHeight, parent, &initial);
        if let Some(value) = absolutize(value, Property::LineHeight, &context, style.font_size) {
            apply(&mut style, &parent.style, Property::LineHeight, value);
        } else {
            invalid_math(diagnostics, node_id, Property::LineHeight);
        }
    }
    context.line_height = style.used_line_height();
    if is_root {
        context.root_line_height = context.line_height;
    }
    if let Some((_, value)) = winners.get(&Property::Color) {
        let value = computed_value(value, Property::Color, parent, &initial);
        if let Some(value) = compute_color_value(
            value,
            parent.style.text_color,
            node_id,
            Property::Color,
            diagnostics,
        ) {
            apply(&mut style, &parent.style, Property::Color, value);
        }
    }
    style.border_color = Edges::all(style.color);
    style.borders = Edges::all(style.text_color);
    style.text_fill_color = colors
        .fill
        .resolve(style.text_color)
        .expect("inherited fill");
    let mut properties: Vec<_> = winners.keys().copied().collect();
    properties.sort_unstable();
    for property in properties {
        let (_, value) = &winners[&property];
        if matches!(
            property,
            Property::FontSize
                | Property::FontFamily
                | Property::FontWeight
                | Property::FontStyle
                | Property::Color
                | Property::LineHeight
        ) {
            continue;
        }
        let value = computed_value(value, property, parent, &initial);
        let Some(value) = absolutize(value, property, &context, style.font_size) else {
            invalid_math(diagnostics, node_id, property);
            continue;
        };
        let Some(value) =
            compute_color_value(value, style.text_color, node_id, property, diagnostics)
        else {
            continue;
        };
        // Keep currentcolor symbolic for explicit inheritance. Paint still gets
        // colors resolved against this element's color through ComputedStyle.
        match (property, &value) {
            (Property::BackgroundColor, Value::Color(color)) => colors.background = color.clone(),
            (Property::BorderColor(side), Value::Color(color)) => {
                *colors.border.get_mut(side) = color.clone()
            }
            (Property::TextFillColor, Value::Color(color)) => colors.fill = color.clone(),
            _ => {}
        }
        apply(&mut style, &parent.style, property, value);
    }
    for side in [
        crate::css::values::Side::Top,
        crate::css::values::Side::Right,
        crate::css::values::Side::Bottom,
        crate::css::values::Side::Left,
    ] {
        if !style.border_style.get(side).visible() {
            *style.border_width.get_mut(side) = 0.0;
        }
    }
    let (x, y) = (style.overflow_x, style.overflow_y);
    let rtl = style.direction == super::Direction::Rtl;
    style.float = match style.float {
        super::Float::InlineStart => {
            if rtl {
                super::Float::Right
            } else {
                super::Float::Left
            }
        }
        super::Float::InlineEnd => {
            if rtl {
                super::Float::Left
            } else {
                super::Float::Right
            }
        }
        v => v,
    };
    style.clear = match style.clear {
        super::Clear::InlineStart => {
            if rtl {
                super::Clear::Right
            } else {
                super::Clear::Left
            }
        }
        super::Clear::InlineEnd => {
            if rtl {
                super::Clear::Left
            } else {
                super::Clear::Right
            }
        }
        v => v,
    };
    style.overflow_x = x.coupled(y);
    style.overflow_y = y.coupled(x);
    style.positioned_inline = style.position.out_of_flow() && style.display.inline_outer();
    if is_root && style.display == Display::Contents {
        style.display = Display::Block;
    }
    if style.positioned_inline {
        style.display = style.display.blockified();
    }
    if style.position.out_of_flow() {
        style.float = super::Float::None;
        style.display = style.display.blockified();
    } else if style.float != super::Float::None {
        style.display = style.display.blockified();
    }
    if parent.style.display.is_item_container() && !style.position.out_of_flow() {
        style.display = style.display.blockified();
    }
    if matches!(
        style.display,
        Display::InlineBlock
            | Display::InlineFlowRootListItem
            | Display::InlineFlex
            | Display::InlineGrid
            | Display::InlineTable
    ) || style.position.out_of_flow()
        || style.float != super::Float::None
    {
        style.decoration_sources.clear();
    }
    if style.typography.lines() != 0 {
        if style.decoration_sources.len() < 32 {
            style
                .decoration_sources
                .push(super::typography::Decoration::from_style(&style));
        } else {
            diagnostics.push(Diagnostic {
                node_id: Some(node_id),
                message:
                    "text decoration propagation exceeds 32 sources; additional decoration omitted"
                        .into(),
            });
        }
    }
    ComputedNode { style, colors }
}

fn compute_color_value(
    value: Value,
    current: ResolvedColor,
    node_id: NodeId,
    property: Property,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    if let Value::Effect(v) = value {
        return super::effects::compute_colors(v, current).map(Value::Effect);
    }
    if let Value::Typography(typography::Value::Color(color)) = value {
        return compute_color_value(Value::Color(color), current, node_id, property, diagnostics)
            .map(|value| {
                let Value::Color(color) = value else {
                    unreachable!()
                };
                Value::Typography(typography::Value::Color(color))
            });
    }
    let Value::Color(color) = value else {
        return Some(value);
    };
    if let Some(color) = color.computed(current)
        && color.resolve(current).is_some()
    {
        return Some(Value::Color(color));
    }
    diagnostics.push(Diagnostic {
        node_id: Some(node_id),
        message: format!("invalid at computed-value time: {property:?} (non-finite or resource-limited color calculation)"),
    });
    None
}

fn invalid_math(diagnostics: &mut Vec<Diagnostic>, node_id: NodeId, property: Property) {
    diagnostics.push(Diagnostic {
        node_id: Some(node_id),
        message: format!(
            "invalid at computed-value time: {property:?} (non-finite CSS math result)"
        ),
    });
}

fn absolutize(
    value: Value,
    property: Property,
    context: &LengthContext,
    reference: f32,
) -> Option<Value> {
    match value {
        Value::Typography(typography::Value::Length(length)) => Some(Value::Typography(
            typography::Value::Length(length.computed(context)?.0),
        )),
        Value::BorderSpacing(x, y) => Some(Value::BorderSpacing(
            Length::Px(x.used(context, 0.0)?.0.max(0.0)),
            Length::Px(y.used(context, 0.0)?.0.max(0.0)),
        )),
        Value::GridTracks(values) => {
            super::grid::computed_tracks(values, context).map(Value::GridTracks)
        }
        Value::ObjectPosition(mut position) => {
            position.x.offset = position.x.offset.computed(context)?.0;
            position.y.offset = position.y.offset.computed(context)?.0;
            Some(Value::ObjectPosition(position))
        }
        Value::Effect(v) => super::effects::compute(v, context).map(Value::Effect),
        Value::Length(length) => {
            let mut computed = length.computed(context)?.0;
            if matches!(
                property,
                Property::Width
                    | Property::Height
                    | Property::Padding(_)
                    | Property::MinWidth
                    | Property::MinHeight
                    | Property::MaxWidth
                    | Property::MaxHeight
                    | Property::FlexBasis
                    | Property::RowGap
                    | Property::ColumnGap
            ) && let Length::Px(value) = &mut computed
            {
                *value = value.max(0.0);
            }
            if property.computes_absolute_length() {
                Some(Value::Length(Length::Px(
                    computed.used(context, reference)?.0,
                )))
            } else {
                Some(Value::Length(computed))
            }
        }
        Value::LineHeight(LineHeight::Length(length)) => Some(Value::LineHeight(
            LineHeight::Length(Length::Px(length.used(context, reference)?.0.max(0.0))),
        )),
        Value::NumberMath(expression) => {
            let value = expression.computed(context)?.evaluate(context, 0.0)?;
            Some(match property {
                Property::LineHeight => Value::LineHeight(LineHeight::Number(value.max(0.0))),
                Property::FontWeight => Value::Weight(FontWeight(value.clamp(1.0, 1000.0))),
                _ => unreachable!("number math only applies to numeric properties"),
            })
        }
        other => Some(other),
    }
}

fn computed_value(
    value: &Value,
    property: Property,
    parent: &ComputedNode,
    initial: &ComputedStyle,
) -> Value {
    if let Value::Mapped(original, value) = value {
        return computed_value(value, *original, parent, initial);
    }
    if let Value::FontSizeKeyword(keyword) = value {
        return Value::Length(Length::Px(keyword.resolve(initial.font_size)));
    }
    if let Value::RelativeWeight(bolder) = value {
        let weight = parent.style.font_weight.0;
        let weight = if *bolder {
            if weight < 350.0 {
                400.0
            } else if weight < 550.0 {
                700.0
            } else {
                weight.max(900.0)
            }
        } else if weight < 550.0 {
            weight.min(100.0)
        } else if weight < 750.0 {
            400.0
        } else {
            700.0
        };
        return Value::Weight(FontWeight(weight));
    }
    let Value::Wide(keyword) = value else {
        return value.clone();
    };
    let inherit = matches!(keyword, WideKeyword::Inherit)
        || matches!(keyword, WideKeyword::Unset) && property.inherited();
    if !inherit {
        return computed_value(
            &declarations::initial_value(property),
            property,
            parent,
            initial,
        );
    }
    let style = &parent.style;
    use Property as P;
    match property {
        P::Logical(_) => computed_value(value, property.physical(style), parent, initial),
        P::Effect(p) => Value::Effect(style.effects.get(p)),
        P::Typography(p) => Value::Typography(style.typography.get(p)),
        P::Content => Value::Content(style.content.clone()),
        P::Color => Value::Color(Color::Absolute(style.text_color)),
        P::TextFillColor => Value::Color(parent.colors.fill.clone()),
        P::BackgroundColor => Value::Color(parent.colors.background.clone()),
        P::BackgroundImage => Value::BackgroundImage(style.background_image),
        P::Display => Value::Display(style.display),
        P::AspectRatio => Value::AspectRatio(style.aspect_ratio),
        P::ObjectFit => Value::ObjectFit(style.object_fit),
        P::ObjectPosition => Value::ObjectPosition(style.object_position.clone()),
        P::Float => Value::Float(style.float),
        P::Clear => Value::Clear(style.clear),
        P::TableLayout => Value::TableLayout(style.table_layout),
        P::BorderCollapse => Value::BorderCollapse(style.border_collapse),
        P::BorderSpacing => Value::BorderSpacing(
            style.border_spacing.0.clone(),
            style.border_spacing.1.clone(),
        ),
        P::CaptionSide => Value::CaptionSide(style.caption_side),
        P::EmptyCells => Value::EmptyCells(style.empty_cells),
        P::Position => Value::Position(style.position),
        P::ZIndex => Value::ZIndex(style.z_index),
        P::OverflowX => Value::Overflow(style.overflow_x),
        P::OverflowY => Value::Overflow(style.overflow_y),
        P::Inset(side) => match style.inset.get(side) {
            crate::css::values::LengthOrAuto::Length(length) => Value::Length(length.clone()),
            crate::css::values::LengthOrAuto::Auto => Value::Auto,
        },
        P::Direction => Value::Direction(style.direction),
        P::UnicodeBidi => Value::UnicodeBidi(style.unicode_bidi),
        P::WritingMode => Value::WritingMode(style.writing_mode),
        P::TextOrientation => Value::TextOrientation(style.text_orientation),
        P::TextAlignAll => Value::TextAlign(style.text_align),
        P::TextAlignLast => Value::TextAlignLast(style.text_align_last),
        P::WhiteSpaceCollapse => Value::WhiteSpaceCollapse(style.white_space_collapse),
        P::TextWrapMode => Value::TextWrapMode(style.text_wrap_mode),
        P::AlignmentBaseline => Value::VerticalAlign(style.vertical_align),
        P::BaselineShift => Value::BaselineShift(style.baseline_shift),
        P::BaselineSource => Value::BaselineSource(style.baseline_source),
        P::BoxSizing => Value::BoxSizing(style.box_sizing),
        P::FontSize => Value::Length(Length::Px(style.font_size)),
        P::FontFamily => Value::Family(style.font_family.clone()),
        P::FontWeight => Value::Weight(style.font_weight),
        P::FontStyle => Value::FontStyle(style.font_style),
        P::LineHeight => Value::LineHeight(style.line_height.clone()),
        P::Width => style
            .width
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::Auto),
        P::Height => style
            .height
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::Auto),
        P::MinWidth => style
            .min_width
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::Auto),
        P::MinHeight => style
            .min_height
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::Auto),
        P::MaxWidth => style
            .max_width
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::None),
        P::MaxHeight => style
            .max_height
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::None),
        P::FlexDirection => Value::FlexDirection(style.flex_direction),
        P::FlexWrap => Value::FlexWrap(style.flex_wrap),
        P::FlexGrow => Value::Number(style.flex_grow),
        P::FlexShrink => Value::Number(style.flex_shrink),
        P::FlexBasis => match &style.flex_basis {
            super::FlexBasis::Auto => Value::Auto,
            super::FlexBasis::Content => Value::ContentBasis,
            super::FlexBasis::Length(l) => Value::Length(l.clone()),
        },
        P::Order => Value::Integer(style.order),
        P::JustifyContent => Value::Alignment(style.justify_content),
        P::JustifyItems => Value::Alignment(style.justify_items),
        P::JustifySelf => Value::Alignment(style.justify_self),
        P::GridTemplateColumns => Value::GridTracks(style.grid_template_columns.clone()),
        P::GridTemplateRows => Value::GridTracks(style.grid_template_rows.clone()),
        P::GridAutoColumns => Value::GridTracks(style.grid_auto_columns.clone()),
        P::GridAutoRows => Value::GridTracks(style.grid_auto_rows.clone()),
        P::GridTemplateAreas => Value::GridAreas(style.grid_template_areas.clone()),
        P::GridAutoFlow => Value::GridAutoFlow(style.grid_auto_flow),
        P::GridColumnStart => Value::GridLine(style.grid_column_start.clone()),
        P::GridColumnEnd => Value::GridLine(style.grid_column_end.clone()),
        P::GridRowStart => Value::GridLine(style.grid_row_start.clone()),
        P::GridRowEnd => Value::GridLine(style.grid_row_end.clone()),
        P::AlignItems => Value::Alignment(style.align_items),
        P::AlignSelf => Value::Alignment(style.align_self),
        P::AlignContent => Value::Alignment(style.align_content),
        P::RowGap => style
            .row_gap
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::None),
        P::ColumnGap => style
            .column_gap
            .clone()
            .map(Value::Length)
            .unwrap_or(Value::None),
        P::ContainerType => Value::ContainerType(style.container_type),
        P::ContainerName => Value::ContainerNames(style.container_names.clone()),
        P::Margin(side) => match style.margin.get(side) {
            crate::css::values::LengthOrAuto::Length(length) => Value::Length(length.clone()),
            crate::css::values::LengthOrAuto::Auto => Value::Auto,
        },
        P::Padding(side) => Value::Length(style.padding.get(side).clone()),
        P::BorderWidth(side) => Value::Length(Length::Px(*style.border_width.get(side))),
        P::BorderStyle(side) => Value::BorderStyle(*style.border_style.get(side)),
        P::BorderColor(side) => Value::Color(parent.colors.border.get(side).clone()),
    }
}

fn apply(style: &mut ComputedStyle, parent: &ComputedStyle, property: Property, value: Value) {
    use Property as P;
    match (property, value) {
        (P::Effect(p), Value::Effect(v)) => style.effects.set(p, v),
        (P::Typography(p), Value::Typography(v)) => {
            style
                .typography
                .set(p, v, style.font_size, style.text_color)
        }
        (P::Position, Value::Position(position)) => style.position = position,
        (P::ZIndex, Value::ZIndex(index)) => style.z_index = index,
        (P::OverflowX, Value::Overflow(value)) => style.overflow_x = value,
        (P::OverflowY, Value::Overflow(value)) => style.overflow_y = value,
        (P::Inset(side), Value::Length(length)) => {
            *style.inset.get_mut(side) = length.into();
        }
        (P::Inset(side), Value::Auto) => {
            *style.inset.get_mut(side) = crate::css::values::LengthOrAuto::Auto;
        }
        (P::Content, Value::Content(content)) => style.content = content,
        (P::Color, Value::Color(color)) => {
            style.text_color = color.resolve(parent.text_color).expect("validated color");
            style.color = style.text_color.legacy();
        }
        (P::TextFillColor, Value::Color(color)) => {
            style.text_fill_color = color
                .resolve(style.text_color)
                .expect("validated fill color");
        }
        (P::BackgroundColor, Value::Color(color)) => {
            style.background = color.resolve(style.text_color).expect("validated color");
            style.background_color = style.background.legacy();
        }
        (P::BackgroundImage, Value::BackgroundImage(image)) => style.background_image = image,
        (P::Display, Value::Display(display)) => style.display = display,
        (P::AspectRatio, Value::AspectRatio(v)) => style.aspect_ratio = v,
        (P::ObjectFit, Value::ObjectFit(v)) => style.object_fit = v,
        (P::ObjectPosition, Value::ObjectPosition(v)) => style.object_position = v,
        (P::Float, Value::Float(v)) => style.float = v,
        (P::Clear, Value::Clear(v)) => style.clear = v,
        (P::TableLayout, Value::TableLayout(v)) => style.table_layout = v,
        (P::BorderCollapse, Value::BorderCollapse(v)) => style.border_collapse = v,
        (P::BorderSpacing, Value::BorderSpacing(x, y)) => style.border_spacing = (x, y),
        (P::CaptionSide, Value::CaptionSide(v)) => style.caption_side = v,
        (P::EmptyCells, Value::EmptyCells(v)) => style.empty_cells = v,
        (P::Direction, Value::Direction(direction)) => style.direction = direction,
        (P::UnicodeBidi, Value::UnicodeBidi(v)) => style.unicode_bidi = v,
        (P::WritingMode, Value::WritingMode(v)) => style.writing_mode = v,
        (P::TextOrientation, Value::TextOrientation(v)) => style.text_orientation = v,
        (P::TextAlignAll, Value::TextAlign(align)) => style.text_align = align,
        (P::TextAlignLast, Value::TextAlignLast(align)) => style.text_align_last = align,
        (P::WhiteSpaceCollapse, Value::WhiteSpaceCollapse(mode)) => {
            style.white_space_collapse = mode
        }
        (P::TextWrapMode, Value::TextWrapMode(mode)) => style.text_wrap_mode = mode,
        (P::AlignmentBaseline, Value::VerticalAlign(align)) => style.vertical_align = align,
        (P::BaselineShift, Value::BaselineShift(shift)) => style.baseline_shift = shift,
        (P::BaselineSource, Value::BaselineSource(source)) => style.baseline_source = source,
        (P::BoxSizing, Value::BoxSizing(sizing)) => style.box_sizing = sizing,
        (P::FontSize, Value::Length(length)) => {
            style.font_size = length
                .resolve(parent.font_size, parent.font_size)
                .clamp(0.0, MAX_FONT_SIZE);
        }
        (P::FontFamily, Value::Family(family)) => style.font_family = family,
        (P::FontWeight, Value::Weight(weight)) => {
            style.font_weight = weight;
            style.bold = weight.0 >= 600.0;
        }
        (P::FontStyle, Value::FontStyle(font_style)) => {
            style.font_style = font_style;
            style.italic = font_style != FontStyle::Normal;
        }
        (P::LineHeight, Value::LineHeight(line_height)) => {
            style.line_height = match line_height {
                LineHeight::Length(length) => {
                    LineHeight::Length(Length::Px(length.resolve(style.font_size, style.font_size)))
                }
                other => other,
            };
        }
        (P::Width, Value::Length(length)) => style.width = Some(length),
        (P::Width, Value::Auto) => style.width = None,
        (P::Height, Value::Length(length)) => style.height = Some(length),
        (P::Height, Value::Auto) => style.height = None,
        (P::MinWidth, Value::Length(l)) => style.min_width = Some(l),
        (P::MinHeight, Value::Length(l)) => style.min_height = Some(l),
        (P::MaxWidth, Value::Length(l)) => style.max_width = Some(l),
        (P::MaxHeight, Value::Length(l)) => style.max_height = Some(l),
        (P::MinWidth, Value::Auto) => style.min_width = None,
        (P::MinHeight, Value::Auto) => style.min_height = None,
        (P::MaxWidth, Value::None) => style.max_width = None,
        (P::MaxHeight, Value::None) => style.max_height = None,
        (P::FlexDirection, Value::FlexDirection(v)) => style.flex_direction = v,
        (P::FlexWrap, Value::FlexWrap(v)) => style.flex_wrap = v,
        (P::FlexGrow, Value::Number(v)) => style.flex_grow = v,
        (P::FlexShrink, Value::Number(v)) => style.flex_shrink = v,
        (P::FlexBasis, Value::Auto) => style.flex_basis = super::FlexBasis::Auto,
        (P::FlexBasis, Value::ContentBasis) => style.flex_basis = super::FlexBasis::Content,
        (P::FlexBasis, Value::Length(l)) => style.flex_basis = super::FlexBasis::Length(l),
        (P::Order, Value::Integer(v)) => style.order = v,
        (P::JustifyContent, Value::Alignment(v)) => style.justify_content = v,
        (P::JustifyItems, Value::Alignment(v)) => style.justify_items = v,
        (P::JustifySelf, Value::Alignment(v)) => style.justify_self = v,
        (P::GridTemplateColumns, Value::GridTracks(v)) => style.grid_template_columns = v,
        (P::GridTemplateRows, Value::GridTracks(v)) => style.grid_template_rows = v,
        (P::GridAutoColumns, Value::GridTracks(v)) => style.grid_auto_columns = v,
        (P::GridAutoRows, Value::GridTracks(v)) => style.grid_auto_rows = v,
        (P::GridTemplateAreas, Value::GridAreas(v)) => style.grid_template_areas = v,
        (P::GridAutoFlow, Value::GridAutoFlow(v)) => style.grid_auto_flow = v,
        (P::GridColumnStart, Value::GridLine(v)) => style.grid_column_start = v,
        (P::GridColumnEnd, Value::GridLine(v)) => style.grid_column_end = v,
        (P::GridRowStart, Value::GridLine(v)) => style.grid_row_start = v,
        (P::GridRowEnd, Value::GridLine(v)) => style.grid_row_end = v,
        (P::AlignItems, Value::Alignment(v)) => style.align_items = v,
        (P::AlignSelf, Value::Alignment(v)) => style.align_self = v,
        (P::AlignContent, Value::Alignment(v)) => style.align_content = v,
        (P::RowGap, Value::Length(l)) => style.row_gap = Some(l),
        (P::ColumnGap, Value::Length(l)) => style.column_gap = Some(l),
        (P::RowGap, Value::None) => style.row_gap = None,
        (P::ColumnGap, Value::None) => style.column_gap = None,
        (P::ContainerType, Value::ContainerType(kind)) => style.container_type = kind,
        (P::ContainerName, Value::ContainerNames(names)) => style.container_names = names,
        (P::Margin(side), Value::Length(length)) => *style.margin.get_mut(side) = length.into(),
        (P::Margin(side), Value::Auto) => {
            *style.margin.get_mut(side) = crate::css::values::LengthOrAuto::Auto;
        }
        (P::Padding(side), Value::Length(length)) => *style.padding.get_mut(side) = length,
        (P::BorderWidth(side), Value::Length(length)) => {
            *style.border_width.get_mut(side) = length.resolve(style.font_size, 0.0).max(0.0)
        }
        (P::BorderStyle(side), Value::BorderStyle(border_style)) => {
            *style.border_style.get_mut(side) = border_style;
        }
        (P::BorderColor(side), Value::Color(color)) => {
            let color = color.resolve(style.text_color).expect("validated color");
            *style.borders.get_mut(side) = color;
            *style.border_color.get_mut(side) = color.legacy();
        }
        _ => unreachable!("typed declarations must match their property"),
    }
}

#[cfg(test)]
#[path = "matching_tests.rs"]
mod matching_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_six_cascade_levels_dominate_specificity_inline_and_source_order() {
        let origins = [Origin::UserAgent, Origin::User, Origin::Author];
        let mut levels = origins
            .into_iter()
            .flat_map(|origin| [origin.level(false), origin.level(true)])
            .collect::<Vec<_>>();
        levels.sort();
        assert_eq!(
            levels,
            [
                CascadeLevel::UserAgent,
                CascadeLevel::User,
                CascadeLevel::Author,
                CascadeLevel::AuthorImportant,
                CascadeLevel::UserImportant,
                CascadeLevel::UserAgentImportant,
            ]
        );
        let document = crate::html::parser::parse("<p id='p'>text</p>").unwrap();
        let node_id = document
            .preorder()
            .find(|&id| {
                document
                    .node(id)
                    .unwrap()
                    .as_element()
                    .is_some_and(|element| element.attribute("id") == Some("p"))
            })
            .unwrap();
        for (low, high) in levels.iter().zip(&levels[1..]) {
            let mut winners = Winners::default();
            let declaration = StyledDeclaration {
                property: DeclarationProperty::Longhand(Property::Color),
                value: Value::Color(Color::Rgb(0xFF0000)),
                important: false,
                order: 0,
            };
            let selectors = selectors::parse("#p#p#p").unwrap();
            let priority = Priority {
                level: *low,
                inline: true,
                layer_rank: 0,
                specificity: selectors[0].specificity(),
                proximity: std::cmp::Reverse(usize::MAX),
                sheet: usize::MAX,
                rule: usize::MAX,
                declaration: usize::MAX,
                layer: 0,
                normal_rank: 0,
            };
            insert(&mut winners, &declaration, priority);
            let (origin, important) = match high {
                CascadeLevel::User => (Origin::User, false),
                CascadeLevel::Author => (Origin::Author, false),
                CascadeLevel::AuthorImportant => (Origin::Author, true),
                CascadeLevel::UserImportant => (Origin::User, true),
                CascadeLevel::UserAgentImportant => (Origin::UserAgent, true),
                CascadeLevel::UserAgent | CascadeLevel::Presentational => unreachable!(),
            };
            let source = if important {
                "p {color:blue !important}"
            } else {
                "p {color:blue}"
            };
            let mut diagnostics = Vec::new();
            let rules = compile::<2>(
                &css::parse(source).unwrap(),
                None,
                &mut diagnostics,
                MediaEnvironment::default(),
                true,
            );
            assert!(diagnostics.is_empty());
            let mut matching = MatchingContext::default();
            matching.keys.extend(element_keys(
                document.node(node_id).unwrap().as_element().unwrap(),
            ));
            apply_rules::<2>(
                &document,
                node_id,
                &rules,
                origin,
                0,
                &mut winners,
                &mut matching,
            );
            insert(&mut winners, &declaration, priority);
            let computed = compute(
                &ComputedNode::default(),
                &winners,
                node_id,
                &mut diagnostics,
            );
            assert_eq!(computed.style.color, Some(0x0000FF), "{low:?} < {high:?}");
        }
    }
}
