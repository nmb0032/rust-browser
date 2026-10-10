use std::{collections::HashMap, sync::Arc};

use url::Url;

use crate::{
    dom::{Attribute, Document, ElementData, NodeId},
    layout::{self, LayoutTree},
    media::ImageMap,
    style::{self, ComputedStyles, StylesheetCollection},
};

#[derive(Clone, Debug)]
pub struct StylesheetSource {
    pub source: Arc<str>,
    pub url: Url,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub url: Url,
    pub document: Document,
    pub stylesheets: StylesheetCollection,
    pub external_stylesheet_sources: HashMap<NodeId, StylesheetSource>,
    pub styles: ComputedStyles,
    pub images: ImageMap,
    pub error: Option<String>,
    pub parse_errors: Vec<crate::html::tokenizer::ParseError>,
    pub dropped_parse_errors: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentTarget {
    Top,
    Node(NodeId),
}

impl Frame {
    pub fn new(url: Url, document: Document) -> Self {
        let resolution = style::resolve(&document);
        Self {
            url,
            document,
            stylesheets: resolution.stylesheets,
            external_stylesheet_sources: HashMap::new(),
            styles: resolution.styles,
            images: ImageMap::new(),
            error: None,
            parse_errors: Vec::new(),
            dropped_parse_errors: 0,
        }
    }

    pub fn fragment_target(&self, fragment: &str) -> Option<FragmentTarget> {
        if fragment.is_empty() {
            return Some(FragmentTarget::Top);
        }
        let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
        for by_id in [true, false] {
            let target = self.document.preorder().find(|id| {
                self.document
                    .node(*id)
                    .and_then(|node| node.as_element())
                    .is_some_and(|element| {
                        if by_id {
                            element.attribute("id") == Some(decoded.as_ref())
                        } else {
                            element.tag_name().eq_ignore_ascii_case("a")
                                && element.attribute("name") == Some(decoded.as_ref())
                        }
                    })
            });
            if let Some(id) = target {
                return Some(FragmentTarget::Node(id));
            }
        }
        decoded
            .eq_ignore_ascii_case("top")
            .then_some(FragmentTarget::Top)
    }

    pub fn is_same_document(&self, url: &Url) -> bool {
        let mut current = self.url.clone();
        let mut next = url.clone();
        current.set_fragment(None);
        next.set_fragment(None);
        current == next && self.error.is_none()
    }

    pub fn layout_tree(&self) -> LayoutTree {
        for diagnostic in &self.stylesheets.diagnostics {
            eprintln!(
                "CSS/resource at {:?}: {}",
                diagnostic.node_id, diagnostic.message
            );
        }
        layout::build_layout_tree_with_resources(&self.document, &self.styles, &self.images)
    }

    pub(super) fn set_stylesheets(&mut self, stylesheets: StylesheetCollection) {
        let resolution = style::resolve_with_stylesheets(&self.document, stylesheets);
        self.stylesheets = resolution.stylesheets;
        self.styles = resolution.styles;
    }

    pub fn error_page(url: Url, message: String) -> Self {
        let mut document = Document::new();
        let root = document.root_id();
        for (tag, text) in [
            ("h1", "Could not load this page".to_owned()),
            ("p", url.as_str().to_owned()),
            ("p", message.clone()),
            (
                "p",
                "Check the address or connection. Use Back to return to the previous page."
                    .to_owned(),
            ),
        ] {
            let node = document
                .append_element(root, ElementData::new(tag.into(), Vec::new()))
                .expect("new error-page elements have a valid document parent");
            document
                .append_text(node, text)
                .expect("new error-page elements can contain text");
        }
        let retry = document
            .append_element(
                root,
                ElementData::new(
                    "a".into(),
                    vec![Attribute::new("href".into(), url.to_string())],
                ),
            )
            .expect("retry link has a valid parent");
        document
            .append_text(retry, "Try again".into())
            .expect("retry link can contain text");
        let mut frame = Self::new(url, document);
        frame.error = Some(message);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dom::NodeKind, html::parser};

    #[test]
    fn fragments_decode_percent_escapes_prioritize_ids_and_fall_back_to_named_anchors() {
        let frame = Frame::new(Url::parse("https://example.org/").unwrap(),
            parser::parse("<a name='top'></a><p id='top'>target</p><p id='a b'>space</p><a name='legacy'></a>").unwrap());
        let top = frame.fragment_target("top").unwrap();
        assert!(
            matches!(top, FragmentTarget::Node(id) if frame.document.node(id).unwrap().as_element().unwrap().tag_name() == "p")
        );
        assert!(matches!(
            frame.fragment_target("a%20b"),
            Some(FragmentTarget::Node(_))
        ));
        assert!(matches!(
            frame.fragment_target("legacy"),
            Some(FragmentTarget::Node(_))
        ));
        assert_eq!(frame.fragment_target(""), Some(FragmentTarget::Top));
        assert_eq!(frame.fragment_target("TOP"), Some(FragmentTarget::Top));
        assert_eq!(frame.fragment_target("missing"), None);
    }

    #[test]
    fn error_details_are_text_not_markup_and_retry_requires_a_reload() {
        let url = Url::parse("https://example.org/page").unwrap();
        let frame = Frame::error_page(url.clone(), "<script>bad()</script>".into());
        assert!(frame.document.preorder().any(|id| matches!(frame.document.node(id).unwrap().kind(), NodeKind::Text(text) if text == "<script>bad()</script>")));
        assert!(!frame.document.preorder().any(|id| {
            frame
                .document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.tag_name() == "script")
        }));
        assert!(!frame.is_same_document(&url));
        let good = Frame::new(url.clone(), Document::new());
        assert!(good.is_same_document(&url.join("#part").unwrap()));
        assert!(!good.is_same_document(&url.join("?query").unwrap()));
    }
}
