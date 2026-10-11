//! Counter evaluation precedes box construction, in DOM/pseudo order. A
//! formatting-context reflow never advances counters or mutates the DOM.
use crate::{
    dom::{Document, NodeId},
    style::{
        ComputedStyle, ComputedStyles, Content, ContentItem, Diagnostic, PseudoElement,
        typography::Property as P,
    },
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Generated {
    pub text: HashMap<(NodeId, PseudoElement), String>,
    pub markers: HashMap<NodeId, String>,
    pub pseudo_markers: HashMap<(NodeId, PseudoElement), String>,
}
struct Counter {
    scope: NodeId,
    value: i32,
}
#[derive(Default)]
struct State {
    values: HashMap<String, Vec<Counter>>,
    work: usize,
    stack_overflow: bool,
    stack_reported: bool,
}
impl State {
    fn reset(&mut self, name: String, value: i32, scope: NodeId) {
        let stack = self.values.entry(name).or_default();
        if stack.last().is_some_and(|c| c.scope == scope) {
            stack.pop();
        }
        if stack.len() < 256 {
            stack.push(Counter { value, scope });
        } else {
            self.stack_overflow = true;
        }
    }
    fn set(&mut self, name: String, value: i32, scope: NodeId) {
        if let Some(c) = self.values.get_mut(&name).and_then(|s| s.last_mut()) {
            c.value = value;
        } else {
            self.reset(name, value, scope);
        }
    }
    fn increment(&mut self, name: String, value: i32, scope: NodeId) {
        if let Some(c) = self.values.get_mut(&name).and_then(|s| s.last_mut()) {
            c.value = c.value.saturating_add(value);
        } else {
            self.reset(name, value, scope);
        }
    }
    fn apply(&mut self, style: &ComputedStyle, scope: NodeId, step: i32, html_value: Option<i32>) {
        for (name, value) in style.typography.counters(P::CounterReset) {
            self.reset(name, value, scope);
        }
        for (name, value) in style.typography.counters(P::CounterIncrement) {
            self.increment(name, value, scope);
        }
        if style.display.is_list_item()
            && !style
                .typography
                .counters(P::CounterIncrement)
                .iter()
                .any(|(name, _)| name == "list-item")
        {
            self.increment("list-item".into(), step, scope);
        }
        if let Some(value) = html_value {
            self.set("list-item".into(), value, scope);
        }
        for (name, value) in style.typography.counters(P::CounterSet) {
            self.set(name, value, scope);
        }
    }
    fn marker(
        &mut self,
        style: &ComputedStyle,
        styles: &ComputedStyles,
        id: NodeId,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> String {
        let value = self
            .values
            .get("list-item")
            .and_then(|s| s.last())
            .map_or(0, |c| c.value);
        let (marker, issue) =
            styles
                .counter_styles
                .format(&style.typography.name(P::ListType), value, true);
        if let Some(message) = issue {
            report(diagnostics, id, message);
        }
        marker
    }
    fn render(
        &mut self,
        content: &Content,
        document: &Document,
        styles: &ComputedStyles,
        id: NodeId,
        pseudo: PseudoElement,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<String> {
        self.report_limits(id, diagnostics);
        let Content::Items(items) = content else {
            return None;
        };
        let element = document.node(id)?.as_element()?;
        let mut text = String::new();
        for item in items {
            let part = match item {
                ContentItem::Text(s) => s.clone(),
                ContentItem::Attribute(name) => element.attribute(name).unwrap_or("").into(),
                ContentItem::Counter {
                    name,
                    style,
                    separator,
                } => {
                    let values = self
                        .values
                        .get(name)
                        .map(|stack| {
                            if separator.is_some() {
                                stack.iter().map(|c| c.value).collect::<Vec<_>>()
                            } else {
                                stack.last().map(|c| vec![c.value]).unwrap_or_default()
                            }
                        })
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| vec![0]);
                    let mut result = Vec::new();
                    for value in values {
                        let (formatted, issue) = styles.counter_styles.format(style, value, false);
                        if let Some(message) = issue {
                            report(diagnostics, id, message);
                        }
                        result.push(formatted);
                    }
                    result.join(separator.as_deref().unwrap_or(""))
                }
            };
            self.work += part.len();
            if text.len() + part.len() > 65_536 || self.work > 1_048_576 {
                report(
                    diagnostics,
                    id,
                    format!(
                        "content::{}: generated text exceeds 65536-byte limit or document work limit; content omitted",
                        pseudo.name()
                    ),
                );
                return None;
            }
            text.push_str(&part);
        }
        Some(text)
    }
    fn report_limits(&mut self, id: NodeId, diagnostics: &mut Vec<Diagnostic>) {
        if self.stack_overflow && !self.stack_reported {
            report(
                diagnostics,
                id,
                "counter scope stack exceeds 256; additional resets omitted".into(),
            );
            self.stack_reported = true;
        }
    }
}
fn report(diagnostics: &mut Vec<Diagnostic>, id: NodeId, message: String) {
    if diagnostics.len() < 256
        && !diagnostics
            .iter()
            .any(|d| d.node_id == Some(id) && d.message == message)
    {
        diagnostics.push(Diagnostic {
            node_id: Some(id),
            message,
        });
    }
}

pub(super) fn evaluate(
    document: &Document,
    styles: &ComputedStyles,
    diagnostics: &mut Vec<Diagnostic>,
) -> Generated {
    let mut generated = Generated::default();
    let mut state = State::default();
    // phase 0 enter; phase 1 after pseudo and expire child-scoped counters.
    let mut pending = vec![(document.root_id(), false, 1)];
    while let Some((id, exit, inherited_step)) = pending.pop() {
        let Some(node) = document.node(id) else {
            continue;
        };
        let Some(style) = styles.get(id) else {
            continue;
        };
        if style.display == crate::style::Display::None {
            continue;
        }
        let scope = node.parent().unwrap_or(document.root_id());
        if exit {
            if let Some(pseudo) = styles.pseudo(id, PseudoElement::After)
                && matches!(pseudo.content, Content::Items(_))
                && pseudo.display != crate::style::Display::None
            {
                state.apply(pseudo, id, inherited_step, None);
                if pseudo.display.is_list_item() {
                    let marker = state.marker(pseudo, styles, id, diagnostics);
                    generated
                        .pseudo_markers
                        .insert((id, PseudoElement::After), marker);
                }
                if let Some(text) = state.render(
                    &pseudo.content,
                    document,
                    styles,
                    id,
                    PseudoElement::After,
                    diagnostics,
                ) {
                    generated.text.insert((id, PseudoElement::After), text);
                }
            }
            for stack in state.values.values_mut() {
                stack.retain(|c| c.scope != id);
            }
            continue;
        }
        if state.work > 1_048_576 || state.values.len() >= 1024 {
            report(
                diagnostics,
                id,
                "counter evaluation exceeds work/name limit; remaining counters omitted".into(),
            );
            break;
        }
        state.work += 1;
        let mut step = inherited_step;
        if let Some(element) = node.as_element() {
            if matches!(element.tag_name(), "ol" | "ul") {
                step = if element.attribute("reversed").is_some() && element.tag_name() == "ol" {
                    -1
                } else {
                    1
                };
                let mut descendants = node.children().to_vec();
                let mut count = 0i32;
                while let Some(child) = descendants.pop() {
                    let Some(child_style) = styles.get(child) else {
                        continue;
                    };
                    if child_style.display.is_list_item() {
                        count = count.saturating_add(1);
                    }
                    if child_style.display == crate::style::Display::Contents {
                        descendants.extend(document.node(child).unwrap().children());
                    }
                }
                let start = element
                    .attribute("start")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(if step == -1 { count } else { 1 });
                if !style.typography.contains(P::CounterReset) {
                    state.reset("list-item".into(), start.saturating_sub(step), id);
                }
            }
            let html_value = style
                .display
                .is_list_item()
                .then(|| {
                    element
                        .attribute("value")
                        .and_then(|s| s.parse::<i32>().ok())
                })
                .flatten();
            state.apply(style, scope, step, html_value);
            state.report_limits(id, diagnostics);
            if style.display.is_list_item() {
                let marker = state.marker(style, styles, id, diagnostics);
                let marker = match styles.pseudo(id, PseudoElement::Marker).map(|s| &s.content) {
                    Some(Content::None) => String::new(),
                    Some(content @ Content::Items(_)) => state
                        .render(
                            content,
                            document,
                            styles,
                            id,
                            PseudoElement::Marker,
                            diagnostics,
                        )
                        .unwrap_or_default(),
                    _ => marker,
                };
                generated.markers.insert(id, marker);
            }
            if let Some(pseudo) = styles.pseudo(id, PseudoElement::Before)
                && matches!(pseudo.content, Content::Items(_))
                && pseudo.display != crate::style::Display::None
            {
                state.apply(pseudo, id, step, None);
                if pseudo.display.is_list_item() {
                    let marker = state.marker(pseudo, styles, id, diagnostics);
                    generated
                        .pseudo_markers
                        .insert((id, PseudoElement::Before), marker);
                }
                if let Some(text) = state.render(
                    &pseudo.content,
                    document,
                    styles,
                    id,
                    PseudoElement::Before,
                    diagnostics,
                ) {
                    generated.text.insert((id, PseudoElement::Before), text);
                }
            }
        }
        pending.push((id, true, step));
        pending.extend(node.children().iter().rev().map(|&id| (id, false, step)));
    }
    diagnostics.sort_by_key(|d| {
        (
            document.preorder().position(|id| Some(id) == d.node_id),
            if d.message.starts_with("content::before:") {
                0
            } else if d.message.starts_with("content::after:") {
                1
            } else {
                2
            },
        )
    });
    generated
}
