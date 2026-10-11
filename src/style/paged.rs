//! Retained @page AST rules and finite page-selector cascade.
use super::{ComputedStyle, Diagnostic, StylesheetCollection};
use crate::{
    css::{
        self, AtRule, BlockItem, Declaration, Rule,
        selectors::Specificity,
        tokenizer::TokenKind,
        values::{Edges, Length},
    },
    dom::{Document, NodeId},
    layout::Rect,
    text::FontContext,
};

#[derive(Clone, Debug)]
struct Selector {
    name: Option<String>,
    first: bool,
    left: bool,
    right: bool,
    blank: bool,
}
impl Selector {
    fn parse(source: &str) -> Option<Self> {
        let source = source.trim();
        let mut parts = source.split(':');
        let name = parts.next()?.trim();
        let name = if name.is_empty() {
            None
        } else {
            let tokens = css::values::math::tokenize(name)?;
            let [TokenKind::Ident(name)] = tokens.as_slice() else {
                return None;
            };
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "auto" | "none" | "initial" | "inherit" | "default"
            ) {
                return None;
            }
            Some(name.clone())
        };
        let mut result = Self {
            name,
            first: false,
            left: false,
            right: false,
            blank: false,
        };
        for part in parts {
            let flag = match part.trim().to_ascii_lowercase().as_str() {
                "first" => &mut result.first,
                "left" => &mut result.left,
                "right" => &mut result.right,
                "blank" => &mut result.blank,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        Some(result)
    }
    fn specificity(&self) -> Specificity {
        Specificity {
            ids: u32::from(self.name.is_some()),
            classes: u32::from(self.first) + u32::from(self.blank),
            types: u32::from(self.left) + u32::from(self.right),
        }
    }
    fn matches(&self, name: Option<&str>, index: usize, blank: bool, rtl: bool) -> bool {
        let right = index.is_multiple_of(2) != rtl;
        self.name.as_deref().is_none_or(|n| name == Some(n))
            && (!self.first || index == 0)
            && (!self.left || !right)
            && (!self.right || right)
            && (!self.blank || blank)
    }
}
#[derive(Clone, Debug)]
struct PageRule {
    selector: Selector,
    declarations: Vec<Declaration>,
    margins: Vec<(String, Vec<Declaration>)>,
    order: usize,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Rules {
    rules: Vec<PageRule>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug)]
pub(crate) struct PageStyle {
    pub style: ComputedStyle,
    pub sheet: Rect,
    pub area: Rect,
    pub margins: Vec<(String, ComputedStyle)>,
}

impl Rules {
    pub fn collect(
        document: &Document,
        sheets: &StylesheetCollection,
        environment: css::media::MediaEnvironment,
    ) -> Self {
        fn visit(
            rules: &[Rule],
            result: &mut Rules,
            environment: css::media::MediaEnvironment,
            node: NodeId,
        ) {
            for rule in rules {
                let Rule::At(at) = rule else { continue };
                let block = at.block().unwrap_or(&[]);
                match at.name().to_ascii_lowercase().as_str() {
                    "page" => result.add(at, node),
                    "media"
                        if css::media::MediaQueryList::parse(at.prelude()).matches(environment) =>
                    {
                        let rules = block
                            .iter()
                            .filter_map(|i| match i {
                                BlockItem::Rule(r) => Some(r.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        visit(&rules, result, environment, node);
                    }
                    "supports"
                        if super::control::supports_with_context(
                            at.prelude(),
                            &css::selectors::Namespaces::default(),
                            Some(node),
                            &mut result.diagnostics,
                            super::compat::DeclarationContext::default(),
                        ) =>
                    {
                        let rules = block
                            .iter()
                            .filter_map(|i| match i {
                                BlockItem::Rule(r) => Some(r.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        visit(&rules, result, environment, node);
                    }
                    "import" => {
                        if let (Some(imported), Some(condition)) =
                            (at.imported(), css::control::import(at.prelude()))
                        {
                            let supports = condition.supports.as_ref().is_none_or(|s| {
                                super::control::supports_with_context(
                                    s,
                                    &css::selectors::Namespaces::default(),
                                    Some(node),
                                    &mut result.diagnostics,
                                    super::compat::DeclarationContext::default(),
                                )
                            });
                            if supports
                                && css::media::MediaQueryList::parse(&condition.media)
                                    .matches(environment)
                            {
                                visit(imported.stylesheet.rules(), result, environment, node);
                            }
                        }
                    }
                    "layer"
                        if block.iter().any(|item| matches!(item, BlockItem::Rule(Rule::At(a)) if a.name() == "page")) =>
                    {
                        result.diagnostics.push(Diagnostic { node_id: Some(node), message: "@page cascade layers are outside the finite print profile".into() });
                    }
                    _ => {}
                }
            }
        }
        let mut result = Self::default();
        for sheet in &sheets.stylesheets {
            if document
                .node(sheet.node_id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| {
                    e.attribute("disabled").is_some()
                        || e.attribute("media").is_some_and(|m| {
                            !css::media::MediaQueryList::parse(m).matches(environment)
                        })
                })
            {
                continue;
            }
            visit(
                sheet.stylesheet.rules(),
                &mut result,
                environment,
                sheet.node_id,
            );
        }
        result
    }
    fn add(&mut self, at: &AtRule, node: NodeId) {
        let selectors = at
            .prelude()
            .split(',')
            .map(Selector::parse)
            .collect::<Option<Vec<_>>>();
        let Some(selectors) = selectors else {
            self.diagnostics.push(Diagnostic {
                node_id: Some(node),
                message: "invalid @page selector".into(),
            });
            return;
        };
        let mut declarations = Vec::new();
        let mut margins = Vec::new();
        for item in at.block().unwrap_or(&[]) {
            match item {
                BlockItem::Declaration(d) => declarations.push(d.clone()),
                BlockItem::Rule(Rule::At(m)) if matches!(m.name(), "top-left" | "top-center" | "top-right" | "bottom-left" | "bottom-center" | "bottom-right" | "left-top" | "left-middle" | "left-bottom" | "right-top" | "right-middle" | "right-bottom" | "top-left-corner" | "top-right-corner" | "bottom-left-corner" | "bottom-right-corner") && m.prelude().trim().is_empty() => {
                    margins.push((m.name().into(), m.block().unwrap_or(&[]).iter().filter_map(|i| match i { BlockItem::Declaration(d) => Some(d.clone()), _ => None }).collect()));
                }
                _ => self.diagnostics.push(Diagnostic { node_id: Some(node), message: "unsupported @page nested rule (running elements/footnotes are not implemented)".into() }),
            }
        }
        for selector in selectors {
            self.rules.push(PageRule {
                selector,
                declarations: declarations.clone(),
                margins: margins.clone(),
                order: self.rules.len(),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &self,
        parent: &ComputedStyle,
        node: NodeId,
        name: Option<&str>,
        index: usize,
        blank: bool,
        rtl: bool,
        default_size: (f32, f32),
        default_margin: f32,
        environment: css::media::MediaEnvironment,
        fonts: &FontContext,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> PageStyle {
        let matching = self
            .rules
            .iter()
            .filter(|r| r.selector.matches(name, index, blank, rtl))
            .collect::<Vec<_>>();
        let allowed = |d: &&Declaration| {
            let p = d.property();
            p == "size"
                || p.starts_with("--")
                || p.starts_with("margin")
                || p.starts_with("padding")
                || p.starts_with("border")
                || p.starts_with("font")
                || matches!(
                    p,
                    "color"
                        | "background"
                        | "background-color"
                        | "line-height"
                        | "direction"
                        | "writing-mode"
                )
        };
        let all = matching
            .iter()
            .flat_map(|r| {
                r.declarations
                    .iter()
                    .map(move |d| (d, r.selector.specificity(), r.order))
            })
            .collect::<Vec<_>>();
        for (d, _, _) in &all {
            if !allowed(d) {
                diagnostics.push(Diagnostic {
                    node_id: Some(node),
                    message: format!("unsupported @page descriptor: {}", d.property()),
                });
            }
        }
        let native = all
            .iter()
            .copied()
            .filter(|(d, _, _)| d.property() != "size" && allowed(d))
            .collect::<Vec<_>>();
        let mut style = super::cascade::compute_page_style(
            parent,
            &native,
            node,
            environment,
            fonts,
            diagnostics,
        );
        for side in [
            css::values::Side::Top,
            css::values::Side::Right,
            css::values::Side::Bottom,
            css::values::Side::Left,
        ] {
            if !native.iter().any(|(d, _, _)| {
                d.property() == "margin"
                    || d.property()
                        == format!(
                            "margin-{}",
                            match side {
                                css::values::Side::Top => "top",
                                css::values::Side::Right => "right",
                                css::values::Side::Bottom => "bottom",
                                css::values::Side::Left => "left",
                            }
                        )
            }) {
                *style.margin.get_mut(side) = Length::Px(default_margin).into();
            }
        }
        let size = all
            .iter()
            .filter(|(d, _, _)| d.property() == "size")
            .filter_map(|(d, specificity, order)| {
                match page_size(d, &style, environment, default_size) {
                    Some(size) => Some(((d.is_important(), *specificity, *order), size)),
                    None => {
                        diagnostics.push(Diagnostic {
                            node_id: Some(node),
                            message: format!("invalid @page size: {}", d.value()),
                        });
                        None
                    }
                }
            })
            .max_by_key(|(priority, _)| *priority)
            .map_or(default_size, |(_, size)| size);
        let sheet = Rect {
            x: 0.0,
            y: 0.0,
            width: size.0,
            height: size.1,
        };
        let resolve_margin = |side: css::values::Side, basis: f32| match style.margin.get(side) {
            css::values::LengthOrAuto::Length(v) => v.resolve(style.font_size, basis),
            css::values::LengthOrAuto::Auto => 0.0,
        };
        let margin = Edges {
            top: resolve_margin(css::values::Side::Top, size.1),
            bottom: resolve_margin(css::values::Side::Bottom, size.1),
            left: resolve_margin(css::values::Side::Left, size.0),
            right: resolve_margin(css::values::Side::Right, size.0),
        };
        let padding = style.padding.map(|v| v.resolve(style.font_size, size.0));
        let inset = Edges {
            top: margin.top + padding.top + style.border_width.top,
            right: margin.right + padding.right + style.border_width.right,
            bottom: margin.bottom + padding.bottom + style.border_width.bottom,
            left: margin.left + padding.left + style.border_width.left,
        };
        let area = Rect {
            x: inset.left,
            y: inset.top,
            width: size.0 - inset.left - inset.right,
            height: size.1 - inset.top - inset.bottom,
        };
        let mut margins = Vec::new();
        for position in [
            "top-left",
            "top-center",
            "top-right",
            "bottom-left",
            "bottom-center",
            "bottom-right",
            "left-top",
            "left-middle",
            "left-bottom",
            "right-top",
            "right-middle",
            "right-bottom",
            "top-left-corner",
            "top-right-corner",
            "bottom-left-corner",
            "bottom-right-corner",
        ] {
            let declarations = matching
                .iter()
                .flat_map(|r| {
                    r.margins
                        .iter()
                        .filter(move |(p, _)| p == position)
                        .flat_map(move |(_, declarations)| {
                            declarations
                                .iter()
                                .map(move |d| (d, r.selector.specificity(), r.order))
                        })
                })
                .collect::<Vec<_>>();
            if !declarations.is_empty() {
                let margin_style = super::cascade::compute_page_style(
                    &style,
                    &declarations,
                    node,
                    environment,
                    fonts,
                    diagnostics,
                );
                margins.push((position.into(), margin_style));
            }
        }
        PageStyle {
            style,
            sheet,
            area,
            margins,
        }
    }
}

fn page_size(
    d: &Declaration,
    style: &ComputedStyle,
    environment: css::media::MediaEnvironment,
    default_size: (f32, f32),
) -> Option<(f32, f32)> {
    let tokens = css::values::components::from_tokens(d.value_tokens())?;
    if tokens.is_empty() || tokens.len() > 2 {
        return None;
    }
    let words = tokens
        .iter()
        .filter_map(|t| t.ident().map(str::to_ascii_lowercase))
        .collect::<Vec<_>>();
    if words.len() == tokens.len() {
        if words == ["auto"] {
            return Some(default_size);
        }
        let mut size = None;
        let mut landscape = None;
        for word in words {
            match word.as_str() {
                "a4" if size.is_none() => size = Some((793.7008, 1122.5197)),
                "letter" if size.is_none() => size = Some((816.0, 1056.0)),
                "portrait" if landscape.is_none() => landscape = Some(false),
                "landscape" if landscape.is_none() => landscape = Some(true),
                _ => return None,
            }
        }
        let (a, b) = size.unwrap_or(default_size);
        return Some(if landscape.unwrap_or(false) {
            (a.max(b), a.min(b))
        } else {
            (a.min(b), a.max(b))
        });
    }
    let mut context = super::cascade::length_context(style, environment, None);
    context.font_size = style.font_size;
    let values = tokens
        .iter()
        .map(|t| {
            let l = t.length()?;
            if l.has_percentage() {
                return None;
            }
            l.used(&context, 0.0)
                .map(|v| v.0)
                .filter(|v| *v > 0.0 && v.is_finite())
        })
        .collect::<Option<Vec<_>>>()?;
    Some((values[0], *values.get(1).unwrap_or(&values[0])))
}
