use std::{collections::HashMap, sync::OnceLock};

use crate::{
    css::{
        self, BlockItem, Rule, Stylesheet,
        selectors::{self, Selector, Specificity},
        values::{Color, Length},
    },
    dom::{Document, NodeId, NodeKind},
};

use super::{
    ComputedStyle, ComputedStyles, Diagnostic, Display, StylesheetCollection, collect_stylesheets,
    declarations::{self, Property, StyledDeclaration, Value, WideKeyword},
    report_css_errors,
};

const MAX_FONT_SIZE: f32 = 512.0;

#[derive(Clone, Debug)]
struct StyledRule {
    selectors: Vec<Selector>,
    declarations: Vec<StyledDeclaration>,
    order: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Priority {
    origin: u8,
    inline: bool,
    specificity: Specificity,
    sheet: usize,
    rule: usize,
    declaration: usize,
}

#[derive(Clone, Debug)]
pub struct StyleResolution {
    pub styles: ComputedStyles,
    pub stylesheets: StylesheetCollection,
}

pub fn resolve(document: &Document) -> StyleResolution {
    resolve_with_stylesheets(document, collect_stylesheets(document))
}

pub fn resolve_with_stylesheets(
    document: &Document,
    mut stylesheets: StylesheetCollection,
) -> StyleResolution {
    let author_rules = stylesheets
        .stylesheets
        .iter()
        .map(|sheet| {
            compile(
                &sheet.stylesheet,
                Some(sheet.node_id),
                &mut stylesheets.diagnostics,
            )
        })
        .collect::<Vec<_>>();
    let mut nodes = HashMap::with_capacity(document.node_count());
    let initial = ComputedStyle::default();
    let mut pending = vec![document.root_id()];
    while let Some(node_id) = pending.pop() {
        let Some(node) = document.node(node_id) else {
            continue;
        };
        pending.extend(node.children().iter().rev().copied());
        let parent = node
            .parent()
            .and_then(|id| nodes.get(&id))
            .unwrap_or(&initial);
        let style = match node.kind() {
            NodeKind::Document | NodeKind::DocumentFragment => ComputedStyle {
                display: Display::Block,
                ..initial.clone()
            },
            NodeKind::Text(_) => ComputedStyle::inherit(parent),
            NodeKind::Comment(_)
            | NodeKind::Doctype(_)
            | NodeKind::ProcessingInstruction { .. } => ComputedStyle {
                display: Display::None,
                ..ComputedStyle::inherit(parent)
            },
            NodeKind::Element(element) => {
                let mut winners = HashMap::new();
                apply_rules(
                    document,
                    node_id,
                    user_agent_rules(),
                    false,
                    0,
                    &mut winners,
                );
                if element.tag_name().eq_ignore_ascii_case("img")
                    && let Some(width) = element
                        .attribute("width")
                        .and_then(crate::html::attributes::image_dimension)
                {
                    insert(
                        &mut winners,
                        &StyledDeclaration {
                            property: Property::Width,
                            value: Value::Length(Length::Px(width)),
                            important: false,
                            order: 0,
                        },
                        Priority {
                            origin: 1,
                            inline: false,
                            specificity: Specificity::default(),
                            sheet: 0,
                            rule: 0,
                            declaration: 0,
                        },
                    );
                }
                for (sheet, rules) in author_rules.iter().enumerate() {
                    apply_rules(document, node_id, rules, true, sheet, &mut winners);
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
                        match declarations::parse(declaration, order) {
                            Ok(declarations) => {
                                for declaration in &declarations {
                                    insert(
                                        &mut winners,
                                        declaration,
                                        Priority {
                                            origin: if declaration.important { 2 } else { 1 },
                                            inline: true,
                                            specificity: Specificity::default(),
                                            sheet: 0,
                                            rule: 0,
                                            declaration: order,
                                        },
                                    );
                                }
                            }
                            Err(message) => stylesheets.diagnostics.push(Diagnostic {
                                node_id: Some(node_id),
                                message,
                            }),
                        }
                    }
                }
                compute(parent, &winners, node_id, &mut stylesheets.diagnostics)
            }
        };
        nodes.insert(node_id, style);
    }
    StyleResolution {
        styles: ComputedStyles { nodes },
        stylesheets,
    }
}

fn user_agent_rules() -> &'static [StyledRule] {
    static RULES: OnceLock<Vec<StyledRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let sheet =
            css::parse(include_str!("user_agent.css")).expect("bundled user-agent CSS must parse");
        let mut diagnostics = Vec::new();
        let rules = compile(&sheet, None, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "invalid bundled user-agent CSS: {diagnostics:?}"
        );
        rules
    })
}

fn compile(
    sheet: &Stylesheet,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<StyledRule> {
    let mut result = Vec::new();
    for (order, rule) in sheet.rules().iter().enumerate() {
        let Rule::Qualified(rule) = rule else {
            diagnostics.push(Diagnostic {
                node_id,
                message: "at-rules are not supported yet".into(),
            });
            continue;
        };
        let selectors = match selectors::parse(rule.prelude()) {
            Ok(selectors) => selectors,
            Err(error) => {
                diagnostics.push(Diagnostic {
                    node_id,
                    message: format!("{}: {error}", rule.prelude()),
                });
                continue;
            }
        };
        let mut declarations = Vec::new();
        for (order, item) in rule.items().iter().enumerate() {
            match item {
                BlockItem::Declaration(declaration) => {
                    match declarations::parse(declaration, order) {
                        Ok(values) => declarations.extend(values),
                        Err(message) => diagnostics.push(Diagnostic { node_id, message }),
                    }
                }
                BlockItem::Rule(_) => diagnostics.push(Diagnostic {
                    node_id,
                    message: "nested rules are not supported yet".into(),
                }),
            }
        }
        result.push(StyledRule {
            selectors,
            declarations,
            order,
        });
    }
    result
}

type Winners = HashMap<Property, (Priority, Value)>;

fn apply_rules(
    document: &Document,
    node_id: NodeId,
    rules: &[StyledRule],
    author: bool,
    sheet: usize,
    winners: &mut Winners,
) {
    for rule in rules {
        let specificity = rule
            .selectors
            .iter()
            .filter(|selector| selector.matches(document, node_id))
            .map(Selector::specificity)
            .max();
        let Some(specificity) = specificity else {
            continue;
        };
        for declaration in &rule.declarations {
            let origin = match (author, declaration.important) {
                (false, false) => 0,
                (true, false) => 1,
                (true, true) => 2,
                (false, true) => 3,
            };
            insert(
                winners,
                declaration,
                Priority {
                    origin,
                    inline: false,
                    specificity,
                    sheet,
                    rule: rule.order,
                    declaration: declaration.order,
                },
            );
        }
    }
}

fn insert(winners: &mut Winners, declaration: &StyledDeclaration, priority: Priority) {
    if winners
        .get(&declaration.property)
        .is_none_or(|(old, _)| priority >= *old)
    {
        winners.insert(declaration.property, (priority, declaration.value.clone()));
    }
}

fn compute(
    parent: &ComputedStyle,
    winners: &Winners,
    node_id: NodeId,
    diagnostics: &mut Vec<Diagnostic>,
) -> ComputedStyle {
    let initial = ComputedStyle::default();
    let mut style = ComputedStyle::inherit(parent);
    // Resolve dependent values in CSS order, not hash-table iteration order.
    if let Some((_, value)) = winners.get(&Property::FontSize) {
        let value = computed_value(value, Property::FontSize, parent, &initial);
        if let Value::Length(length) = &value
            && length.resolve(parent.font_size, parent.font_size) > MAX_FONT_SIZE
        {
            diagnostics.push(Diagnostic {
                node_id: Some(node_id),
                message: format!(
                    "font-size exceeds the {MAX_FONT_SIZE}px rasterization limit; clamped"
                ),
            });
        }
        apply(&mut style, parent, Property::FontSize, value);
    }
    if let Some((_, value)) = winners.get(&Property::Color) {
        apply(
            &mut style,
            parent,
            Property::Color,
            computed_value(value, Property::Color, parent, &initial),
        );
    }
    style.border_color = crate::css::values::Edges::all(style.color);
    for (&property, (_, value)) in winners {
        if matches!(property, Property::FontSize | Property::Color) {
            continue;
        }
        apply(
            &mut style,
            parent,
            property,
            computed_value(value, property, parent, &initial),
        );
    }
    style
}

fn computed_value(
    value: &Value,
    property: Property,
    parent: &ComputedStyle,
    initial: &ComputedStyle,
) -> Value {
    let Value::Wide(keyword) = value else {
        return value.clone();
    };
    let inherit = matches!(keyword, WideKeyword::Inherit)
        || matches!(keyword, WideKeyword::Unset) && property.inherited();
    let style = if inherit { parent } else { initial };
    use Property as P;
    match property {
        P::Color => Value::Color(rgb(style.color)),
        P::BackgroundColor => Value::Color(rgb(style.background_color)),
        P::Display => Value::Display(style.display),
        P::FontSize => Value::Length(Length::Px(style.font_size)),
        P::FontFamily => Value::Family(style.font_family.clone()),
        P::FontWeight => Value::Bold(style.bold),
        P::FontStyle => Value::Italic(style.italic),
        P::Width => style.width.map(Value::Length).unwrap_or(Value::Auto),
        P::Margin(side) => Value::Length(*style.margin.get(side)),
        P::Padding(side) => Value::Length(*style.padding.get(side)),
        P::BorderWidth(side) => Value::Length(Length::Px(*style.border_width.get(side))),
        P::BorderColor(side) => {
            if inherit {
                Value::Color(rgb(*style.border_color.get(side)))
            } else {
                Value::Color(Color::CurrentColor)
            }
        }
    }
}

fn rgb(color: Option<u32>) -> Color {
    color.map(Color::Rgb).unwrap_or(Color::Transparent)
}

fn resolve_color(color: Color, current: Option<u32>) -> Option<u32> {
    match color {
        Color::Rgb(value) => Some(value),
        Color::Transparent => None,
        Color::CurrentColor => current,
    }
}

fn apply(style: &mut ComputedStyle, parent: &ComputedStyle, property: Property, value: Value) {
    use Property as P;
    match (property, value) {
        (P::Color, Value::Color(color)) => style.color = resolve_color(color, parent.color),
        (P::BackgroundColor, Value::Color(color)) => {
            style.background_color = resolve_color(color, style.color)
        }
        (P::Display, Value::Display(display)) => style.display = display,
        (P::FontSize, Value::Length(length)) => {
            style.font_size = length
                .resolve(parent.font_size, parent.font_size)
                .clamp(0.0, MAX_FONT_SIZE);
        }
        (P::FontFamily, Value::Family(family)) => style.font_family = family,
        (P::FontWeight, Value::Bold(bold)) => style.bold = bold,
        (P::FontStyle, Value::Italic(italic)) => style.italic = italic,
        (P::Width, Value::Length(length)) => style.width = Some(length.compute(style.font_size)),
        (P::Width, Value::Auto) => style.width = None,
        (P::Margin(side), Value::Length(length)) => {
            *style.margin.get_mut(side) = length.compute(style.font_size)
        }
        (P::Padding(side), Value::Length(length)) => {
            *style.padding.get_mut(side) = length.compute(style.font_size)
        }
        (P::BorderWidth(side), Value::Length(length)) => {
            *style.border_width.get_mut(side) = length.resolve(style.font_size, 0.0)
        }
        (P::BorderColor(side), Value::Color(color)) => {
            *style.border_color.get_mut(side) = resolve_color(color, style.color)
        }
        _ => unreachable!("typed declarations must match their property"),
    }
}
