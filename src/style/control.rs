use std::{collections::HashMap, sync::Arc};

use crate::{
    css::{
        self,
        control::{self, Component},
        selectors::{self, InteractionState, MatchLimitExceeded, Namespaces, Selector},
        tokenizer::TokenKind,
    },
    dom::{Document, NodeId},
};

use super::{Diagnostic, compat::DeclarationContext, declarations};

#[derive(Clone, Debug, Default)]
pub(super) struct Layers {
    children: Vec<Vec<usize>>,
    names: HashMap<(usize, String), usize>,
}

impl Layers {
    pub fn register(&mut self, parent: usize, path: &[String]) -> usize {
        if self.children.is_empty() {
            self.children.push(Vec::new());
        }
        if path.is_empty() {
            return self.child(parent, None);
        }
        let mut layer = parent;
        for name in path {
            layer = self.child(layer, Some(name));
        }
        layer
    }

    fn child(&mut self, parent: usize, name: Option<&String>) -> usize {
        if let Some(name) = name
            && let Some(&id) = self.names.get(&(parent, name.clone()))
        {
            return id;
        }
        let id = self.children.len();
        self.children.push(Vec::new());
        self.children[parent].push(id);
        if let Some(name) = name {
            self.names.insert((parent, name.clone()), id);
        }
        id
    }

    pub fn ranks(&self) -> Vec<usize> {
        if self.children.is_empty() {
            return vec![0];
        }
        // A layer's direct declarations occupy its last implicit sublayer.
        let mut ranks = vec![0; self.children.len()];
        let mut next = 0;
        let mut work = vec![(0, false)];
        while let Some((id, exit)) = work.pop() {
            if exit {
                ranks[id] = next;
                next += 1;
            } else {
                work.push((id, true));
                work.extend(self.children[id].iter().rev().map(|&child| (child, false)));
            }
        }
        ranks
    }
}

pub(super) fn supports_with_context(
    source: &str,
    namespaces: &Namespaces,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
    context: DeclarationContext,
) -> bool {
    fn evaluate(
        source: &str,
        namespaces: &Namespaces,
        depth: usize,
        diagnostics: &mut Vec<String>,
        context: DeclarationContext,
    ) -> Option<bool> {
        if depth > css::MAX_RULE_NESTING {
            return None;
        }
        let values = control::components(source)?;
        let atom = |value: &Component, diagnostics: &mut Vec<String>| -> Option<bool> {
            match value {
                Component::Group { name: None, source } => {
                    if let Some(declaration) = control::single_declaration(source) {
                        let valid = !declaration.is_important()
                            && declarations::parse_with_context(&declaration, 0, context).is_ok();
                        if !valid {
                            diagnostics
                                .push(format!("unsupported @supports declaration: {source}"));
                        }
                        Some(valid)
                    } else if let Some(value) =
                        evaluate(source, namespaces, depth + 1, diagnostics, context)
                    {
                        Some(value)
                    } else {
                        diagnostics.push(format!("unknown @supports condition: ({source})"));
                        Some(false)
                    }
                }
                Component::Group {
                    name: Some(name),
                    source,
                } if name.eq_ignore_ascii_case("selector") => {
                    let valid = selectors::parse_context(source, namespaces, None).is_ok();
                    if !valid {
                        diagnostics.push(format!("unsupported @supports selector: {source}"));
                    }
                    Some(valid)
                }
                Component::Group { .. } => {
                    diagnostics.push("unknown @supports function".into());
                    Some(false)
                }
                _ => None,
            }
        };
        if values.first().is_some_and(|value| value.ident("not")) {
            return (values.len() == 2)
                .then(|| atom(&values[1], diagnostics).map(|value| !value))
                .flatten();
        }
        if values.is_empty() || values.len() % 2 == 0 {
            return None;
        }
        let and = values.get(1).is_some_and(|value| value.ident("and"));
        let or = values.get(1).is_some_and(|value| value.ident("or"));
        if values.len() > 1 && !and && !or {
            return None;
        }
        let mut result = atom(&values[0], diagnostics)?;
        for pair in values[1..].as_chunks::<2>().0 {
            if !pair[0].ident(if and { "and" } else { "or" }) {
                return None;
            }
            let value = atom(&pair[1], diagnostics)?;
            result = if and {
                result && value
            } else {
                result || value
            };
        }
        Some(result)
    }
    let mut messages = Vec::new();
    let result = evaluate(source, namespaces, 0, &mut messages, context);
    if result.is_none() {
        messages.push(format!("invalid @supports condition: {source}"));
    }
    diagnostics.extend(
        messages
            .into_iter()
            .map(|message| Diagnostic { node_id, message }),
    );
    result.unwrap_or(false)
}

#[derive(Clone, Debug)]
pub(super) struct Scope {
    pub roots: Option<Vec<Selector>>,
    pub limits: Vec<Selector>,
    pub implicit: Option<NodeId>,
}

impl Scope {
    pub fn parse(
        source: &str,
        namespaces: &Namespaces,
        implicit: Option<NodeId>,
        scoped: bool,
        parent: Option<Arc<[Selector]>>,
    ) -> Option<Self> {
        let (root, limit) = control::scope(source)?;
        Some(Self {
            roots: root
                .map(|source| selectors::parse_scoped(&source, namespaces, parent, scoped))
                .transpose()
                .ok()?,
            limits: limit
                .map(|source| selectors::parse_scoped(&source, namespaces, None, true))
                .transpose()
                .ok()?
                .unwrap_or_default(),
            implicit,
        })
    }

    pub fn matches(
        &self,
        document: &Document,
        node_id: NodeId,
        outer: Option<NodeId>,
        state: &InteractionState,
    ) -> Result<Vec<(NodeId, usize)>, MatchLimitExceeded> {
        let mut result = Vec::new();
        let mut path = Vec::new();
        let mut current = Some(node_id);
        while let Some(id) = current {
            let node = document.node(id).expect("valid scope ancestor");
            if node.as_element().is_some() {
                let root = if let Some(roots) = &self.roots {
                    let mut matched = false;
                    for selector in roots {
                        if selector.try_matches_target(document, id, outer, state, None)? {
                            matched = true;
                            break;
                        }
                    }
                    matched
                } else if let Some(implicit) = self.implicit {
                    id == implicit
                } else {
                    node.parent()
                        .and_then(|id| document.node(id))
                        .is_some_and(|node| matches!(node.kind(), crate::dom::NodeKind::Document))
                };
                if root {
                    let mut limited = false;
                    'limits: for &child in &path {
                        for selector in &self.limits {
                            if selector.try_matches_target(
                                document,
                                child,
                                Some(id),
                                state,
                                None,
                            )? {
                                limited = true;
                                break 'limits;
                            }
                        }
                    }
                    if !limited {
                        result.push((id, path.len()));
                    }
                }
                path.push(id);
            }
            if Some(id) == outer {
                break;
            }
            current = node.parent();
        }
        Ok(result)
    }
}

pub(super) fn namespace_rule(source: &str, namespaces: &mut Namespaces) -> bool {
    let Some((prefix, uri)) = control::namespace(source) else {
        return false;
    };
    if let Some(prefix) = prefix {
        namespaces.prefixes.insert(prefix, uri);
    } else {
        namespaces.default = Some(uri);
    }
    true
}

pub(super) fn is_statement(rule: &css::AtRule, name: &str) -> bool {
    rule.name().eq_ignore_ascii_case(name) && rule.block().is_none()
}

pub(super) fn is_ident_token(tokens: &[TokenKind], name: &str) -> bool {
    let significant: Vec<_> = tokens
        .iter()
        .filter(|token| !matches!(token, TokenKind::Whitespace))
        .collect();
    matches!(significant.as_slice(), [TokenKind::Ident(value)] if value.eq_ignore_ascii_case(name))
}
