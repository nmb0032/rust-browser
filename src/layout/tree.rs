use std::sync::Arc;

use crate::{
    css::values::{Edges, Length, LengthOrAuto, LineHeight, ResolvedColor, UsedLength},
    dom::{Document, NodeId, NodeKind},
    media::{DecodedImage, ImageMap},
    style::{self, ComputedStyle, ComputedStyles, Content, Diagnostic, Display, PseudoElement},
};

use super::{Rect, block::layout_children, replaced::ImageContent};

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
    pub text_color: ResolvedColor,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListMarker {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub style: ComputedStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBox {
    pub(super) node_id: Option<NodeId>,
    pub(super) tag_name: String,
    pub(super) kind: BoxKind,
    pub(super) text: Option<String>,
    pub(super) style: ComputedStyle,
    pub(super) rect: Rect,
    pub(super) content_rect: Rect,
    pub(super) padding: Edges<f32>,
    pub(super) margin: Edges<f32>,
    pub(super) children: Vec<LayoutBox>,
    pub(super) text_fragments: Vec<TextFragment>,
    pub(super) list_marker: Option<ListMarker>,
    pub(super) image: Option<ImageContent>,
    pub(super) pseudo: Option<PseudoElement>,
    pub(super) marker_style: Option<ComputedStyle>,
    pub(super) marker_text: Option<String>,
    pub(super) inline_start: bool,
    pub(super) inline_end: bool,
}

impl LayoutBox {
    /// Used content width after layout (including `auto` and range clamping).
    pub fn used_width(&self) -> UsedLength {
        UsedLength(self.content_rect.width)
    }

    /// Used margins, distinct from computed length-or-auto values.
    pub fn used_margins(&self) -> Edges<UsedLength> {
        self.margin.map(UsedLength)
    }

    pub fn actual_rect(&self, scale: f32) -> Option<super::ActualRect> {
        self.rect.actual(scale)
    }

    /// Real node or originating node of a pseudo; anonymous formatting boxes
    /// have no DOM identity.
    pub fn dom_node_id(&self) -> Option<NodeId> {
        self.node_id
    }
    pub fn is_anonymous(&self) -> bool {
        self.node_id.is_none()
    }
    pub fn pseudo_element(&self) -> Option<PseudoElement> {
        self.pseudo
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

    pub(crate) fn decoded_image(&self) -> Option<&Arc<DecodedImage>> {
        self.image.as_ref()?.data.as_ref()
    }

    /// Block-in-inline continuations have no decorations at the broken edges.
    pub(crate) fn inline_edges<T: Default>(&self, mut edges: Edges<T>) -> Edges<T> {
        if self.kind == BoxKind::Inline {
            if !self.inline_start {
                edges.left = T::default();
            }
            if !self.inline_end {
                edges.right = T::default();
            }
        }
        edges
    }

    pub(super) fn reset(&mut self) {
        self.rect = Rect::default();
        self.content_rect = Rect::default();
        self.margin = Edges::default();
        self.padding = Edges::default();
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
    construction_diagnostics: Vec<Diagnostic>,
    diagnostics: Vec<Diagnostic>,
}

impl LayoutTree {
    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    /// Generated-content failures and used-value failures from the latest
    /// layout, also reported to stderr. Construction failures survive relayout.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// CSS/document coordinates. Text is tested per fragment, not its multi-line bounds.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        fn hit(layout_box: &LayoutBox, x: f32, y: f32) -> Option<NodeId> {
            for child in layout_box.children.iter().rev() {
                if let Some(id) = hit(child, x, y) {
                    return Some(id);
                }
            }
            let node_id = layout_box.node_id?;
            if layout_box.kind == BoxKind::Text {
                return layout_box
                    .text_fragments
                    .iter()
                    .rev()
                    .any(|fragment| {
                        contains(
                            Rect {
                                x: fragment.x,
                                y: fragment.y,
                                width: fragment.width,
                                height: fragment.height,
                            },
                            x,
                            y,
                        )
                    })
                    .then_some(node_id);
            }
            // Undecorated inline wrappers have no single hit region when wrapped.
            if layout_box.kind == BoxKind::Inline
                && layout_box.image.is_none()
                && layout_box.style.background_color.is_none()
                && layout_box.inline_edges(layout_box.style.border_width) == Edges::default()
            {
                return None;
            }
            contains(layout_box.rect, x, y).then_some(node_id)
        }
        fn contains(rect: Rect, x: f32, y: f32) -> bool {
            rect.width > 0.0
                && rect.height > 0.0
                && x >= rect.x
                && x < rect.x + rect.width
                && y >= rect.y
                && y < rect.y + rect.height
        }
        hit(&self.root, x, y)
    }

    /// Bounds of all real boxes for a node, including block-in-inline
    /// continuations but not its generated pseudo content or anonymous boxes.
    pub fn node_rect(&self, node_id: NodeId) -> Option<Rect> {
        fn find(layout_box: &LayoutBox, id: NodeId) -> Option<Rect> {
            let own = (layout_box.node_id == Some(id) && layout_box.pseudo.is_none())
                .then_some(layout_box.rect);
            layout_box
                .children
                .iter()
                .fold(own, |bounds, child| match (bounds, find(child, id)) {
                    (Some(bounds), Some(rect)) => Some(bounds.union(rect)),
                    (bounds, rect) => bounds.or(rect),
                })
        }
        find(&self.root, node_id)
    }

    pub fn layout(&mut self, viewport_width: f32, font: &fontdue::Font) {
        self.root.reset();
        self.diagnostics.clone_from(&self.construction_diagnostics);
        let viewport_width = finite_used(
            viewport_width as f64,
            None,
            "viewport-width",
            &mut self.diagnostics,
        )
        .max(0.0);
        let content_x = PAGE_PADDING.min(viewport_width);
        let content_width = (viewport_width - 2.0 * PAGE_PADDING).max(0.0);
        let content_height = layout_children(
            &mut self.root,
            content_x,
            PAGE_PADDING,
            content_width,
            font,
            &mut self.diagnostics,
        );
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
            height: finite_used(
                PAGE_PADDING as f64 + content_height as f64 + PAGE_PADDING as f64,
                None,
                "viewport content height",
                &mut self.diagnostics,
            ),
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
    build_layout_tree_with_resources(document, styles, &ImageMap::new())
}

pub fn build_layout_tree_with_resources(
    document: &Document,
    styles: &ComputedStyles,
    images: &ImageMap,
) -> LayoutTree {
    let mut diagnostics = Vec::new();
    let root = build_box(
        document,
        document.root_id(),
        styles,
        images,
        &mut diagnostics,
    )
    .expect("the document root must have a computed style and layout box");
    LayoutTree {
        root,
        construction_diagnostics: diagnostics.clone(),
        diagnostics,
    }
}

pub(super) fn resolve_length(
    length: &Length,
    font_size: f32,
    reference: f32,
    node_id: Option<NodeId>,
    property: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    length.resolve_with_diagnostic(font_size, reference, |message| {
        report_used_failure(diagnostics, node_id, property, message);
    })
}

pub(super) fn resolve_edges(
    edges: &Edges<Length>,
    font_size: f32,
    reference: f32,
    node_id: Option<NodeId>,
    property: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Edges<f32> {
    edges.map(|length| {
        resolve_length(
            &length,
            font_size,
            reference,
            node_id,
            property,
            diagnostics,
        )
    })
}

pub(super) fn resolve_margins(
    style: &ComputedStyle,
    reference: f32,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Edges<Option<f32>> {
    style.margin.map(|margin| match margin {
        LengthOrAuto::Auto => None,
        LengthOrAuto::Length(length) => Some(resolve_length(
            &length,
            style.font_size,
            reference,
            node_id,
            "margin",
            diagnostics,
        )),
    })
}

/// Arithmetic is wider than storage so representable inputs cannot silently
/// produce infinite geometry. Unrepresentable results use the length fallback.
pub(super) fn finite_used(
    value: f64,
    node_id: Option<NodeId>,
    property: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let result = value as f32;
    if result.is_finite() {
        result
    } else {
        report_used_failure(
            diagnostics,
            node_id,
            property,
            "invalid at used-value time: non-finite layout result; compatibility fallback is 0px"
                .into(),
        );
        0.0
    }
}

pub(super) fn finite_rect(
    rect: Rect,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Rect {
    Rect {
        x: finite_used(rect.x as f64, node_id, "inline x", diagnostics),
        y: finite_used(rect.y as f64, node_id, "inline y", diagnostics),
        width: finite_used(rect.width as f64, node_id, "inline width", diagnostics),
        height: finite_used(rect.height as f64, node_id, "inline height", diagnostics),
    }
}

pub(super) fn resolve_line_height(
    line_height: &LineHeight,
    font_size: f32,
    node_id: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    line_height.used_with_diagnostic(font_size, |message| {
        report_used_failure(diagnostics, node_id, "line-height", message);
    })
}

fn report_used_failure(
    diagnostics: &mut Vec<Diagnostic>,
    node_id: Option<NodeId>,
    property: &str,
    message: String,
) {
    let diagnostic = Diagnostic {
        node_id,
        message: format!("{property}: {message}"),
    };
    eprintln!("CSS at {:?}: {}", diagnostic.node_id, diagnostic.message);
    diagnostics.push(diagnostic);
}

fn build_box(
    document: &Document,
    node_id: NodeId,
    styles: &ComputedStyles,
    images: &ImageMap,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LayoutBox> {
    let node = document.node(node_id)?;
    let style = styles
        .get(node_id)
        .expect("every node in this document snapshot must have a computed style")
        .clone();
    if style.display == Display::None {
        return None;
    }
    let (tag_name, kind, text) = match node.kind() {
        NodeKind::Document | NodeKind::DocumentFragment => {
            ("#document".into(), BoxKind::Block, None)
        }
        NodeKind::Element(element) => {
            let kind = match style.display {
                Display::Block | Display::ListItem => BoxKind::Block,
                Display::Inline => BoxKind::Inline,
                Display::None => return None,
            };
            (format!("<{}>", element.tag_name()), kind, None)
        }
        NodeKind::Text(text) => ("#text".into(), BoxKind::Text, Some(text.clone())),
        NodeKind::Comment(_) | NodeKind::Doctype(_) | NodeKind::ProcessingInstruction { .. } => {
            return None;
        }
    };
    let mut children: Vec<_> = node
        .children()
        .iter()
        .filter_map(|id| build_box(document, *id, styles, images, diagnostics))
        .collect();
    let mut marker_text = None;
    let mut marker_style = None;
    if let Some(element) = node.as_element() {
        if !matches!(
            element.tag_name().to_ascii_lowercase().as_str(),
            "img"
                | "input"
                | "textarea"
                | "select"
                | "video"
                | "audio"
                | "iframe"
                | "canvas"
                | "object"
                | "embed"
        ) {
            if let Some(before) =
                generated_box(node_id, element, styles, PseudoElement::Before, diagnostics)
            {
                children.insert(0, before);
            }
            if let Some(after) =
                generated_box(node_id, element, styles, PseudoElement::After, diagnostics)
            {
                children.push(after);
            }
        }
        if let Some(marker) = styles.pseudo(node_id, PseudoElement::Marker) {
            marker_style = Some(marker.clone());
            marker_text = match &marker.content {
                Content::Normal => None,
                Content::None => Some(String::new()),
                _ => generated_text(
                    &marker.content,
                    element,
                    node_id,
                    PseudoElement::Marker,
                    diagnostics,
                )
                .or_else(|| Some(String::new())),
            };
        }
    }
    let mut layout_box = LayoutBox {
        node_id: Some(node_id),
        tag_name,
        kind,
        text,
        style,
        rect: Rect::default(),
        content_rect: Rect::default(),
        padding: Edges::default(),
        margin: Edges::default(),
        image: node
            .as_element()
            .filter(|element| element.tag_name().eq_ignore_ascii_case("img"))
            .map(|element| ImageContent::new(element, images.get(&node_id).cloned())),
        children,
        text_fragments: Vec::new(),
        list_marker: None,
        pseudo: None,
        marker_style,
        marker_text,
        inline_start: true,
        inline_end: true,
    };
    if kind == BoxKind::Block && layout_box.image.is_none() {
        normalize_block_children(&mut layout_box);
    }
    Some(layout_box)
}

fn normalize_block_children(parent: &mut LayoutBox) {
    let children: Vec<_> = std::mem::take(&mut parent.children)
        .into_iter()
        .flat_map(split_inline)
        .collect();
    if !children.iter().any(|child| child.kind == BoxKind::Block) {
        parent.children = children;
        return;
    }
    let mut run = Vec::new();
    for child in children {
        if child.kind == BoxKind::Block {
            flush_inline_run(parent, &mut run);
            parent.children.push(child);
        } else {
            run.push(child);
        }
    }
    flush_inline_run(parent, &mut run);
}

fn flush_inline_run(parent: &mut LayoutBox, run: &mut Vec<LayoutBox>) {
    // Discard only collapsible text, not empty real inline/pseudo boxes,
    // decorated inline elements, replaced content, or non-CSS spaces (NBSP).
    if run.iter().all(|child| {
        child.kind == BoxKind::Text
            && child.text.as_ref().is_none_or(|text| {
                text.chars()
                    .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
            })
    }) {
        run.clear();
        return;
    }
    let mut style = ComputedStyle::inherit(&parent.style);
    style.display = Display::Block;
    parent.children.push(LayoutBox {
        node_id: None,
        tag_name: "#anonymous-block".into(),
        kind: BoxKind::Block,
        text: None,
        style,
        rect: Rect::default(),
        content_rect: Rect::default(),
        padding: Edges::default(),
        margin: Edges::default(),
        children: std::mem::take(run),
        text_fragments: Vec::new(),
        list_marker: None,
        image: None,
        pseudo: None,
        marker_style: None,
        marker_text: None,
        inline_start: true,
        inline_end: true,
    });
}

/// Lift in-flow blocks through every inline ancestor to their containing
/// block. Real inline continuations retain identity and their computed style;
/// the lifted blocks retain DOM inheritance rather than inheriting wrappers.
fn split_inline(mut inline: LayoutBox) -> Vec<LayoutBox> {
    if inline.kind != BoxKind::Inline || inline.image.is_some() {
        return vec![inline];
    }
    let children: Vec<_> = std::mem::take(&mut inline.children)
        .into_iter()
        .flat_map(split_inline)
        .collect();
    if !children.iter().any(|child| child.kind == BoxKind::Block) {
        inline.children = children;
        return vec![inline];
    }
    let mut result = Vec::new();
    let mut run = Vec::new();
    for child in children {
        if child.kind == BoxKind::Block {
            // Keep the first and last continuations even when empty, but do
            // not insert empty continuations between consecutive blocks.
            if result.is_empty() || !run.is_empty() {
                let mut fragment = inline.clone();
                fragment.children = std::mem::take(&mut run);
                result.push(fragment);
            }
            result.push(child);
        } else {
            run.push(child);
        }
    }
    inline.children = run;
    result.push(inline);
    let last = result.len() - 1;
    for (index, fragment) in result.iter_mut().enumerate() {
        if fragment.kind == BoxKind::Inline {
            fragment.inline_start = index == 0;
            fragment.inline_end = index == last;
        }
    }
    result
}

fn generated_box(
    node_id: NodeId,
    element: &crate::dom::ElementData,
    styles: &ComputedStyles,
    pseudo: PseudoElement,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LayoutBox> {
    let style = styles.pseudo(node_id, pseudo)?.clone();
    if style.display == Display::None {
        return None;
    }
    let text = generated_text(&style.content, element, node_id, pseudo, diagnostics)?;
    let text_box = LayoutBox {
        node_id: Some(node_id),
        tag_name: "#generated-text".into(),
        kind: BoxKind::Text,
        text: Some(text),
        style: ComputedStyle::inherit(&style),
        rect: Rect::default(),
        content_rect: Rect::default(),
        padding: Edges::default(),
        margin: Edges::default(),
        children: Vec::new(),
        text_fragments: Vec::new(),
        list_marker: None,
        image: None,
        pseudo: Some(pseudo),
        marker_style: None,
        marker_text: None,
        inline_start: true,
        inline_end: true,
    };
    Some(LayoutBox {
        node_id: Some(node_id),
        tag_name: format!("::{}", pseudo.name()),
        kind: if matches!(style.display, Display::Block | Display::ListItem) {
            BoxKind::Block
        } else {
            BoxKind::Inline
        },
        text: None,
        style,
        rect: Rect::default(),
        content_rect: Rect::default(),
        padding: Edges::default(),
        margin: Edges::default(),
        children: vec![text_box],
        text_fragments: Vec::new(),
        list_marker: None,
        image: None,
        pseudo: Some(pseudo),
        marker_style: None,
        marker_text: None,
        inline_start: true,
        inline_end: true,
    })
}

fn generated_text(
    content: &Content,
    element: &crate::dom::ElementData,
    node_id: NodeId,
    pseudo: PseudoElement,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    content.text_with_diagnostic(element, |message| {
        report_used_failure(
            diagnostics,
            Some(node_id),
            &format!("content::{}", pseudo.name()),
            message,
        );
    })
}
