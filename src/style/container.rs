use std::collections::HashMap;

use crate::{
    css::{
        container,
        media::{MediaEnvironment, MediaQueryList},
        tokenizer::{TokenKind, Tokenizer},
        values::LengthContext,
    },
    dom::{Document, NodeId},
    layout::{BoxKind, LayoutBox, LayoutTree},
};

#[derive(Clone, Debug)]
pub(super) struct ContainerQuery {
    pub source: String,
    name: Option<String>,
    condition: MediaQueryList,
}

impl ContainerQuery {
    pub fn parse(source: &str) -> Self {
        let mut tokenizer = Tokenizer::default();
        tokenizer.feed(source);
        tokenizer.finish();
        let tokens = std::iter::from_fn(|| tokenizer.next_token()).collect::<Vec<_>>();
        let first = tokens
            .into_iter()
            .find(|token| token.kind != TokenKind::Whitespace);
        if !tokenizer.errors().is_empty() || tokenizer.dropped_errors() > 0 {
            return Self {
                source: source.to_owned(),
                name: None,
                condition: MediaQueryList::parse_container(source),
            };
        }
        let (name, condition) = match first {
            Some(token) if matches!(&token.kind, TokenKind::Ident(name) if container::custom_name(name)) =>
            {
                let TokenKind::Ident(name) = token.kind else {
                    unreachable!()
                };
                (Some(name), &source[token.span.end.byte_offset..])
            }
            _ => (None, source),
        };
        Self {
            source: source.to_owned(),
            name,
            condition: MediaQueryList::parse_container(condition),
        }
    }

    pub fn diagnostics(&self) -> &[String] {
        self.condition.diagnostics()
    }

    pub fn valid(&self) -> bool {
        self.condition.valid_condition()
    }

    pub fn matches(
        &self,
        document: &Document,
        node: NodeId,
        pseudo: bool,
        snapshot: &ContainerSnapshot,
        environment: MediaEnvironment,
    ) -> (bool, bool) {
        let (width, height) = self.condition.required_axes();
        snapshot
            .select(document, node, pseudo, self.name.as_deref(), width, height)
            .map(|container| {
                self.condition.container_evaluation(
                    environment,
                    container.size,
                    snapshot.unit_context(document, container.node, false, container.lengths),
                )
            })
            .unwrap_or((false, false))
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Container {
    node: NodeId,
    names: Vec<String>,
    kind: container::ContainerType,
    size: (f32, f32),
    lengths: LengthContext,
}

/// Immutable real content-box geometry from one completed layout. The empty
/// snapshot is the explicit bootstrap phase, not a viewport-sized container.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerSnapshot {
    containers: HashMap<NodeId, Container>,
}

impl ContainerSnapshot {
    pub fn from_layout(tree: &LayoutTree, environment: MediaEnvironment) -> Self {
        let root = tree
            .root()
            .children()
            .first()
            .map(|child| (child.style().font_size, child.style().used_line_height()));
        let mut result = Self::default();
        fn visit(
            layout: &LayoutBox,
            result: &mut ContainerSnapshot,
            environment: MediaEnvironment,
            root: Option<(f32, f32)>,
        ) {
            let style = layout.style();
            if layout.pseudo_element().is_none()
                && layout.kind() == BoxKind::Block
                && !layout.tag_name().eq_ignore_ascii_case("<img>")
                && style.container_type != container::ContainerType::Normal
            {
                let rect = layout.content_rect();
                if rect.width.is_finite()
                    && rect.height.is_finite()
                    && rect.width >= 0.0
                    && rect.height >= 0.0
                {
                    result.containers.insert(
                        layout.dom_node_id(),
                        Container {
                            node: layout.dom_node_id(),
                            names: style.container_names.clone(),
                            kind: style.container_type,
                            size: (rect.width, rect.height),
                            lengths: super::cascade::length_context(style, environment, root),
                        },
                    );
                } else {
                    eprintln!(
                        "CSS container at {:?}: non-finite content geometry; unavailable for queries",
                        layout.dom_node_id()
                    );
                }
            }
            for child in layout.children() {
                visit(child, result, environment, root);
            }
        }
        visit(tree.root(), &mut result, environment, root);
        result
    }

    fn select(
        &self,
        document: &Document,
        node: NodeId,
        pseudo: bool,
        name: Option<&str>,
        width: bool,
        height: bool,
    ) -> Option<&Container> {
        let mut ancestor = if pseudo {
            Some(node)
        } else {
            document.node(node)?.parent()
        };
        while let Some(id) = ancestor {
            if let Some(container) = self.containers.get(&id)
                && container.kind.supports(width, height)
                && name.is_none_or(|name| container.names.iter().any(|n| n == name))
            {
                return Some(container);
            }
            ancestor = document.node(id)?.parent();
        }
        None
    }

    pub(super) fn unit_context(
        &self,
        document: &Document,
        node: NodeId,
        pseudo: bool,
        mut lengths: LengthContext,
    ) -> LengthContext {
        lengths.container_width = self
            .select(document, node, pseudo, None, true, false)
            .map(|c| c.size.0);
        lengths.container_height = self
            .select(document, node, pseudo, None, false, true)
            .map(|c| c.size.1);
        lengths
    }
}
