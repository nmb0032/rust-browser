use std::{collections::HashMap, sync::Arc};

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
    pub stylesheet: Arc<Stylesheet>,
    pub url: Option<url::Url>,
}

#[derive(Clone, Debug, Default)]
pub struct StylesheetCollection {
    pub stylesheets: Vec<AuthorStylesheet>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn collect_stylesheets(document: &Document) -> StylesheetCollection {
    collect_stylesheets_with_external(document, &HashMap::new())
}

pub fn collect_stylesheets_with_external(
    document: &Document,
    external: &HashMap<NodeId, AuthorStylesheet>,
) -> StylesheetCollection {
    let mut result = StylesheetCollection::default();
    for node_id in document.preorder() {
        if let Some(sheet) = external.get(&node_id) {
            result.stylesheets.push(sheet.clone());
        }
        let Some(source) = embedded_stylesheet_source(document, node_id) else {
            continue;
        };
        match css::parse(&source) {
            Ok(stylesheet) => result.stylesheets.push(AuthorStylesheet {
                node_id,
                stylesheet: Arc::new(stylesheet),
                url: None,
            }),
            Err(error) => result.diagnostics.push(Diagnostic {
                node_id: Some(node_id),
                message: error.to_string(),
            }),
        }
    }
    result
}

pub fn embedded_stylesheet_source(document: &Document, node_id: NodeId) -> Option<String> {
    let node = document.node(node_id)?;
    let element = node.as_element()?;
    if !element.tag_name().eq_ignore_ascii_case("style")
        || element.attribute("type").is_some_and(|value| {
            !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("text/css")
        })
    {
        return None;
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
    Some(source)
}
