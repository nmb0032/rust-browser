use crate::{
    css::values::Edges,
    dom::{Document, NodeId, NodeKind},
    style::{self, ComputedStyle, ComputedStyles, Display},
};

use super::{Rect, block::layout_children};

// This is a viewport gutter, not an element's CSS padding.
const PAGE_PADDING: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKind {
    Block,
    Inline,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextFragment {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
    pub font_size: f32,
    pub color: Option<u32>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListMarker {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBox {
    pub(super) node_id: NodeId,
    pub(super) tag_name: String,
    pub(super) kind: BoxKind,
    pub(super) text: Option<String>,
    pub(super) style: ComputedStyle,
    pub(super) rect: Rect,
    pub(super) content_rect: Rect,
    pub(super) padding: Edges<f32>,
    pub(super) children: Vec<LayoutBox>,
    pub(super) text_fragments: Vec<TextFragment>,
    pub(super) list_marker: Option<ListMarker>,
}

impl LayoutBox {
    pub fn dom_node_id(&self) -> NodeId {
        self.node_id
    }
    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }
    pub fn kind(&self) -> BoxKind {
        self.kind
    }
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
    /// The border box, excluding margins.
    pub fn rect(&self) -> Rect {
        self.rect
    }
    pub fn content_rect(&self) -> Rect {
        self.content_rect
    }
    pub fn style(&self) -> &ComputedStyle {
        &self.style
    }
    pub fn children(&self) -> &[LayoutBox] {
        &self.children
    }
    pub(crate) fn text_fragments(&self) -> &[TextFragment] {
        &self.text_fragments
    }
    pub(crate) fn list_marker(&self) -> Option<&ListMarker> {
        self.list_marker.as_ref()
    }

    pub(super) fn reset(&mut self) {
        self.rect = Rect::default();
        self.content_rect = Rect::default();
        self.text_fragments.clear();
        self.list_marker = None;
        for child in &mut self.children {
            child.reset();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    pub fn layout(&mut self, viewport_width: f32, font: &fontdue::Font) {
        self.root.reset();
        let viewport_width = viewport_width.max(0.0);
        let content_x = PAGE_PADDING.min(viewport_width);
        let content_width = (viewport_width - 2.0 * PAGE_PADDING).max(0.0);
        let content_height =
            layout_children(&mut self.root, content_x, PAGE_PADDING, content_width, font);
        self.root.content_rect = Rect {
            x: content_x,
            y: PAGE_PADDING,
            width: content_width,
            height: content_height,
        };
        self.root.rect = Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_width,
            height: PAGE_PADDING + content_height + PAGE_PADDING,
        };
    }
}

pub fn build_layout_tree(document: &Document) -> LayoutTree {
    let result = style::resolve(document);
    for diagnostic in &result.stylesheets.diagnostics {
        eprintln!("CSS at {:?}: {}", diagnostic.node_id, diagnostic.message);
    }
    build_layout_tree_with_styles(document, &result.styles)
}

/// `styles` must be the resolution for this exact document snapshot.
pub fn build_layout_tree_with_styles(document: &Document, styles: &ComputedStyles) -> LayoutTree {
    let root = build_box(document, document.root_id(), styles)
        .expect("the document root must have a computed style and layout box");
    LayoutTree { root }
}

fn build_box(document: &Document, node_id: NodeId, styles: &ComputedStyles) -> Option<LayoutBox> {
    let node = document.node(node_id)?;
    let style = styles
        .get(node_id)
        .expect("every node in this document snapshot must have a computed style")
        .clone();
    if style.display == Display::None {
        return None;
    }
    let (tag_name, kind, text) = match node.kind() {
        NodeKind::Document => ("#document".into(), BoxKind::Block, None),
        NodeKind::Element(element) => {
            let kind = match style.display {
                Display::Block | Display::ListItem => BoxKind::Block,
                Display::Inline => BoxKind::Inline,
                Display::None => return None,
            };
            (format!("<{}>", element.tag_name()), kind, None)
        }
        NodeKind::Text(text) => ("#text".into(), BoxKind::Text, Some(text.clone())),
    };
    Some(LayoutBox {
        node_id,
        tag_name,
        kind,
        text,
        style,
        rect: Rect::default(),
        content_rect: Rect::default(),
        padding: Edges::default(),
        children: node
            .children()
            .iter()
            .filter_map(|id| build_box(document, *id, styles))
            .collect(),
        text_fragments: Vec::new(),
        list_marker: None,
    })
}
