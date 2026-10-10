use crate::{
    css::{self, Stylesheet},
    dom::{Document, NodeId, NodeKind},
};

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub node_id: Option<NodeId>,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct AuthorStylesheet {
    pub node_id: NodeId,
    pub stylesheet: Stylesheet,
}

#[derive(Clone, Debug, Default)]
pub struct StylesheetCollection {
    pub stylesheets: Vec<AuthorStylesheet>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn collect_stylesheets(document: &Document) -> StylesheetCollection {
    let mut result = StylesheetCollection::default();
    let mut pending = vec![document.root_id()];
    while let Some(node_id) = pending.pop() {
        let Some(node) = document.node(node_id) else {
            continue;
        };
        pending.extend(node.children().iter().rev().copied());
        let Some(element) = node.as_element() else {
            continue;
        };
        if !element.tag_name().eq_ignore_ascii_case("style")
            || element.attribute("type").is_some_and(|value| {
                !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("text/css")
            })
        {
            continue;
        }
        let mut source = String::new();
        let mut children = node.children().iter().rev().copied().collect::<Vec<_>>();
        while let Some(child) = children.pop() {
            if let Some(node) = document.node(child) {
                match node.kind() {
                    NodeKind::Text(text) => source.push_str(text),
                    _ => children.extend(node.children().iter().rev().copied()),
                }
            }
        }
        match css::parse(&source) {
            Ok(stylesheet) => result.stylesheets.push(AuthorStylesheet {
                node_id,
                stylesheet,
            }),
            Err(error) => result.diagnostics.push(Diagnostic {
                node_id: Some(node_id),
                message: error.to_string(),
            }),
        }
    }
    result
}
