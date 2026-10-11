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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CoordinateSpace {
    #[default]
    Document,
    Viewport,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextFragment {
    /// Source line within its generating inline formatting context.
    pub line_index: usize,
    /// Horizontal advance origin; `width` is advance, not ink width.
    pub x: f32,
    /// Font metric top, not line top. Painting uses `baseline` explicitly.
    pub y: f32,
    pub baseline: f32,
    /// Exact shaped-face bitmap bounds at CSS size, including synthetic flags.
    /// Independent of line-height; contains glyph overflow.
    pub ink_rect: Rect,
    pub width: f32,
    pub height: f32,
    pub text: String,
    pub glyphs: Arc<crate::text::ShapedText>,
    pub font_size: f32,
    pub color: Option<u32>,
    pub text_color: ResolvedColor,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListMarker {
    pub x: f32,
    /// Font metric top; outside markers share the first content baseline.
    pub y: f32,
    pub baseline: f32,
    pub text: String,
    pub glyphs: Arc<crate::text::ShapedText>,
    pub style: ComputedStyle,
}

/// Line extent includes the parent strut and baseline-aligned participants'
/// half-leading, but excludes non-replaced inline vertical decorations.
#[derive(Clone, Debug, PartialEq)]
pub struct LineBox {
    /// Available CSS line box, before text alignment.
    pub rect: Rect,
    /// Aligned advance extent (ink and decorations may overflow it).
    pub content_rect: Rect,
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InlineFragment {
    pub line_index: usize,
    pub rect: Rect,
    pub content_rect: Rect,
    pub baseline: f32,
    pub first: bool,
    pub last: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBox {
    // Construction identity survives inline splitting; it is not DOM identity.
    pub(crate) box_id: usize,
    pub(super) parent_box: Option<usize>,
    pub(super) position_parent: Option<usize>,
    pub(super) coordinate_space: CoordinateSpace,
    pub(super) flow_width: f32,
    pub(super) definite_height: Option<f32>,
    pub(super) definite_width: bool,
    pub(super) inline_line: Option<usize>,
    pub(super) is_flex_item: bool,
    pub(super) flex_static: Option<super::flex::StaticPosition>,
    pub(super) node_id: Option<NodeId>,
    pub(super) tag_name: String,
    pub(super) kind: BoxKind,
    pub(super) text: Option<String>,
    pub(super) style: Box<ComputedStyle>,
    pub(super) rect: Rect,
    pub(super) content_rect: Rect,
    pub(super) padding: Edges<f32>,
    pub(super) margin: Edges<f32>,
    pub(super) children: Vec<LayoutBox>,
    pub(super) text_fragments: Vec<TextFragment>,
    pub(super) inline_fragments: Vec<InlineFragment>,
    pub(super) line_boxes: Vec<LineBox>,
    pub(super) list_marker: Option<ListMarker>,
    pub(super) image: Option<ImageContent>,
    pub(super) pseudo: Option<PseudoElement>,
    pub(super) marker_style: Option<ComputedStyle>,
    pub(super) marker_text: Option<String>,
    pub(super) inline_start: bool,
    pub(super) inline_end: bool,
}

impl LayoutBox {
    pub fn coordinate_space(&self) -> CoordinateSpace {
        self.coordinate_space
    }
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
    pub fn is_flex_item(&self) -> bool {
        self.is_flex_item
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
    pub fn inline_fragments(&self) -> &[InlineFragment] {
        &self.inline_fragments
    }
    pub fn line_boxes(&self) -> &[LineBox] {
        &self.line_boxes
    }
    pub(super) fn atomic_inline(&self) -> bool {
        self.style.display == Display::InlineFlex && !self.style.position.out_of_flow()
    }
    pub(super) fn local_root(&self) -> bool {
        self.kind == BoxKind::Block || self.atomic_inline()
    }

    /// Before finalization, descendants' baselines relative to this border
    /// origin are used to position an outside marker in the parent's flow.
    pub(super) fn first_local_baseline(&self) -> Option<f32> {
        if self.style.display.is_flex() {
            let mut items: Vec<_> = self.children.iter().filter(|c| c.is_flex_item).collect();
            items.sort_by_key(|c| c.style.order);
            let first = items.first()?;
            return Some(first.first_local_baseline().unwrap_or(first.rect.height) + first.rect.y);
        }
        self.line_boxes
            .first()
            .map(|line| line.baseline)
            .or_else(|| {
                self.children.iter().find_map(|child| {
                    child.first_local_baseline().map(|baseline| {
                        baseline
                            + if child.local_root() {
                                child.rect.y
                            } else {
                                0.0
                            }
                    })
                })
            })
    }
    pub(crate) fn list_marker(&self) -> Option<&ListMarker> {
        self.list_marker.as_ref()
    }

    pub(crate) fn decoded_image(&self) -> Option<&Arc<DecodedImage>> {
        self.image.as_ref()?.data.as_ref()
    }
    pub(crate) fn is_replaced_image(&self) -> bool {
        self.image.is_some()
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
        self.coordinate_space = CoordinateSpace::Document;
        self.flow_width = 0.0;
        self.definite_height = None;
        self.definite_width = false;
        self.inline_line = None;
        self.flex_static = None;
        self.rect = Rect::default();
        self.content_rect = Rect::default();
        self.margin = Edges::default();
        self.padding = Edges::default();
        self.text_fragments.clear();
        self.inline_fragments.clear();
        self.line_boxes.clear();
        self.list_marker = None;
        for child in &mut self.children {
            child.reset();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutTree {
    pub(super) root: LayoutBox,
    construction_diagnostics: Vec<Diagnostic>,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) scroll_state: super::ScrollState,
    pub(super) overflow: std::collections::HashMap<usize, super::ScrollContainer>,
    pub(super) viewport_overflow: super::ScrollContainer,
}

impl LayoutTree {
    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    pub(crate) fn root_element(&self) -> Option<&LayoutBox> {
        root_element(&self.root)
    }

    /// Generated-content failures and used-value failures from the latest
    /// layout, also reported to stderr. Construction failures survive relayout.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// CSS/document coordinates at zero scroll. See `hit_test_viewport` for
    /// input while scrolled, including viewport-anchored fixed descendants.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        self.hit_test_point(x, y, 0.0, true)
    }

    pub fn hit_test_viewport(&self, x: f32, y: f32, scroll: f32) -> Option<NodeId> {
        let viewport = self.viewport_overflow.port;
        if x < 0.0 || y < 0.0 || x >= viewport.width || y >= viewport.height {
            return None;
        }
        self.hit_test_point(x, y, scroll, false)
    }

    fn hit_test_point(&self, x: f32, y: f32, scroll: f32, document_x: bool) -> Option<NodeId> {
        if let Some(bar) = self.scrollbar_at_in_space(x, y, scroll, document_x) {
            return Some(bar.node_id());
        }
        let decoration = self.spatial_states(false);
        let content = self.spatial_states(true);
        for (layout_box, phase) in super::paint_order(self).into_iter().rev() {
            let Some(node_id) = layout_box.node_id else {
                continue;
            };
            let state = if matches!(
                phase,
                super::PaintPhase::Content | super::PaintPhase::Text(_)
            ) {
                content[&layout_box.box_id]
            } else {
                decoration[&layout_box.box_id]
            };
            let x = x - if document_x && layout_box.coordinate_space == CoordinateSpace::Document {
                self.viewport_scroll().x
            } else {
                0.0
            };
            if self.phase_hit(layout_box, phase, state, x, y, scroll) {
                return Some(node_id);
            }
        }
        None
    }

    pub(super) fn phase_hit(
        &self,
        layout_box: &LayoutBox,
        phase: super::PaintPhase,
        state: super::SpatialState,
        x: f32,
        y: f32,
        scroll: f32,
    ) -> bool {
        fn contains(rect: Rect, x: f32, y: f32) -> bool {
            rect.width > 0.0
                && rect.height > 0.0
                && x >= rect.x
                && x < rect.x + rect.width
                && y >= rect.y
                && y < rect.y + rect.height
        }
        let y = y + if layout_box.coordinate_space == CoordinateSpace::Document {
            scroll
        } else {
            0.0
        };
        if !x.is_finite() || !y.is_finite() || !state.clip.contains(x, y) {
            return false;
        }
        let x = x - state.offset.x;
        let y = y - state.offset.y;
        let text_hit = |fragment: &TextFragment| {
            contains(fragment.ink_rect, x, y)
                || fragment.text.chars().all(|c| matches!(c, ' ' | '\t'))
                    && contains(
                        Rect {
                            x: fragment.x,
                            y: fragment.y,
                            width: fragment.width,
                            height: fragment.height,
                        },
                        x,
                        y,
                    )
        };
        match phase {
            super::PaintPhase::Scrollbars => false,
            super::PaintPhase::InlineDecoration(index) => {
                contains(layout_box.inline_fragments[index].rect, x, y)
            }
            super::PaintPhase::Text(index) => text_hit(&layout_box.text_fragments[index]),
            super::PaintPhase::Decoration if layout_box.kind == BoxKind::Text => false,
            super::PaintPhase::Decoration
                if layout_box.kind == BoxKind::Inline && layout_box.image.is_none() =>
            {
                layout_box
                    .inline_fragments
                    .iter()
                    .rev()
                    .any(|f| contains(f.rect, x, y))
            }
            super::PaintPhase::Decoration => contains(layout_box.rect, x, y),
            super::PaintPhase::Content => {
                layout_box.image.is_some() && contains(layout_box.content_rect, x, y)
                    || layout_box.text_fragments.iter().rev().any(text_hit)
                    || layout_box.list_marker.as_ref().is_some_and(|marker| {
                        contains(
                            Rect {
                                x: marker.x,
                                y: marker.baseline - marker.glyphs.ascent,
                                width: marker.glyphs.width,
                                height: marker.glyphs.ascent + marker.glyphs.descent,
                            },
                            x,
                            y,
                        )
                    })
            }
        }
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

    pub fn layout(&mut self, viewport_width: f32, font: &crate::text::FontContext) {
        self.layout_viewport(
            viewport_width,
            crate::css::media::DEFAULT_VIEWPORT_HEIGHT,
            font,
        );
    }

    pub fn layout_viewport(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        font: &crate::text::FontContext,
    ) {
        self.root.reset();
        self.diagnostics.clone_from(&self.construction_diagnostics);
        let viewport_width = finite_used(
            viewport_width as f64,
            None,
            "viewport-width",
            &mut self.diagnostics,
        )
        .max(0.0);
        let viewport_height = finite_used(
            viewport_height as f64,
            None,
            "viewport-height",
            &mut self.diagnostics,
        )
        .max(0.0);
        let content_x = PAGE_PADDING.min(viewport_width);
        let content_width = (viewport_width - 2.0 * PAGE_PADDING).max(0.0);
        self.root.definite_height = Some(viewport_height.max(0.0));
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
        absolutize_y(&mut self.root, 0.0, &mut self.diagnostics);
        super::positioning::position_tree(
            &mut self.root,
            Rect {
                x: 0.0,
                y: 0.0,
                width: viewport_width,
                height: viewport_height,
            },
            font,
            &mut self.diagnostics,
        );
        self.finalize_overflow(Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_width,
            height: viewport_height,
        });
    }

    /// Clipped document extent; hidden panel contents never enlarge the canvas.
    pub(crate) fn document_bottom(&self) -> f32 {
        self.viewport_overflow.extent.y + self.viewport_overflow.extent.height
    }
}

pub(super) fn root_element(b: &LayoutBox) -> Option<&LayoutBox> {
    if b.tag_name == "#document" || b.is_anonymous() {
        b.children.iter().find_map(root_element)
    } else {
        b.tag_name.starts_with('<').then_some(b)
    }
}

/// Blocks were laid out around their own zero border origin; inline boxes
/// share their containing block's coordinates. Convert each box exactly once,
/// including paint fragments and markers, without rerunning used-value math.
pub(super) fn absolutize_y(
    layout_box: &mut LayoutBox,
    offset: f64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let child_offset = if layout_box.local_root() {
        offset + layout_box.rect.y as f64
    } else {
        offset
    };
    let fragment_offset = child_offset;
    layout_box.rect.y = finite_used(
        offset + layout_box.rect.y as f64,
        layout_box.node_id,
        "absolute border y",
        diagnostics,
    );
    layout_box.content_rect.y = finite_used(
        offset + layout_box.content_rect.y as f64,
        layout_box.node_id,
        "absolute content y",
        diagnostics,
    );
    if let Some(anchor) = &mut layout_box.flex_static {
        anchor.area.y += offset as f32;
    }
    for fragment in &mut layout_box.text_fragments {
        fragment.y = finite_used(
            fragment_offset + fragment.y as f64,
            layout_box.node_id,
            "absolute text y",
            diagnostics,
        );
        fragment.baseline = finite_used(
            fragment_offset + fragment.baseline as f64,
            layout_box.node_id,
            "absolute text baseline",
            diagnostics,
        );
        fragment.ink_rect.y = finite_used(
            fragment_offset + fragment.ink_rect.y as f64,
            layout_box.node_id,
            "absolute glyph y",
            diagnostics,
        );
    }
    for fragment in &mut layout_box.inline_fragments {
        fragment.rect.y = finite_used(
            offset + fragment.rect.y as f64,
            layout_box.node_id,
            "absolute inline y",
            diagnostics,
        );
        fragment.content_rect.y = finite_used(
            offset + fragment.content_rect.y as f64,
            layout_box.node_id,
            "absolute inline content y",
            diagnostics,
        );
        fragment.baseline = finite_used(
            offset + fragment.baseline as f64,
            layout_box.node_id,
            "absolute inline baseline",
            diagnostics,
        );
    }
    for line in &mut layout_box.line_boxes {
        line.rect.y = finite_used(
            fragment_offset + line.rect.y as f64,
            layout_box.node_id,
            "absolute line y",
            diagnostics,
        );
        line.content_rect.y = finite_used(
            fragment_offset + line.content_rect.y as f64,
            layout_box.node_id,
            "absolute line content y",
            diagnostics,
        );
        line.baseline = finite_used(
            fragment_offset + line.baseline as f64,
            layout_box.node_id,
            "absolute line baseline",
            diagnostics,
        );
    }
    if let Some(marker) = &mut layout_box.list_marker {
        marker.y = finite_used(
            offset + marker.y as f64,
            layout_box.node_id,
            "absolute marker y",
            diagnostics,
        );
        marker.baseline = finite_used(
            offset + marker.baseline as f64,
            layout_box.node_id,
            "absolute marker baseline",
            diagnostics,
        );
    }
    for child in &mut layout_box.children {
        absolutize_y(child, child_offset, diagnostics);
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
    let mut identity = Construction::default();
    let root = build_box(
        document,
        document.root_id(),
        styles,
        images,
        &mut diagnostics,
        &mut identity,
    )
    .expect("the document root must have a computed style and layout box");
    LayoutTree {
        root,
        construction_diagnostics: diagnostics.clone(),
        diagnostics,
        scroll_state: super::ScrollState::default(),
        overflow: std::collections::HashMap::new(),
        viewport_overflow: super::ScrollContainer {
            port: Rect::default(),
            extent: Rect::default(),
            range: super::ScrollRange::default(),
            overflow_x: style::Overflow::Auto,
            overflow_y: style::Overflow::Auto,
        },
    }
}

#[derive(Default)]
struct Construction {
    next: usize,
    parent: Option<usize>,
    positioned: Option<usize>,
    flex_parent: bool,
}

impl Construction {
    fn allocate(&mut self) -> usize {
        let id = self.next;
        self.next += 1;
        id
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
    identity: &mut Construction,
) -> Option<LayoutBox> {
    let node = document.node(node_id)?;
    let mut style = Box::new(
        styles
            .get(node_id)
            .expect("every node in this document snapshot must have a computed style")
            .clone(),
    );
    if identity.flex_parent && node.as_element().is_some() && !style.position.out_of_flow() {
        style.display = style.display.blockified();
    }
    if style.display == Display::None {
        return None;
    }
    let (tag_name, kind, text) = match node.kind() {
        NodeKind::Document | NodeKind::DocumentFragment => {
            ("#document".into(), BoxKind::Block, None)
        }
        NodeKind::Element(element) => {
            let kind = match style.display {
                Display::Block | Display::ListItem | Display::Flex => BoxKind::Block,
                Display::Inline | Display::InlineFlex => BoxKind::Inline,
                Display::None => return None,
            };
            (format!("<{}>", element.tag_name()), kind, None)
        }
        NodeKind::Text(text) => ("#text".into(), BoxKind::Text, Some(text.clone())),
        NodeKind::Comment(_) | NodeKind::Doctype(_) | NodeKind::ProcessingInstruction { .. } => {
            return None;
        }
    };
    let box_id = identity.allocate();
    let parent_box = identity.parent.replace(box_id);
    let flex_parent = std::mem::replace(&mut identity.flex_parent, style.display.is_flex());
    let position_parent = identity.positioned;
    if style.position != style::Position::Static {
        identity.positioned = Some(box_id);
    }
    let mut children: Vec<_> = node
        .children()
        .iter()
        .filter_map(|id| build_box(document, *id, styles, images, diagnostics, identity))
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
            if let Some(before) = generated_box(
                node_id,
                element,
                styles,
                PseudoElement::Before,
                diagnostics,
                identity,
            ) {
                children.insert(0, before);
            }
            if let Some(after) = generated_box(
                node_id,
                element,
                styles,
                PseudoElement::After,
                diagnostics,
                identity,
            ) {
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
    if style.display == Display::ListItem
        && marker_text.is_none()
        && let Some(parent) = node.parent().and_then(|id| document.node(id))
        && parent
            .as_element()
            .is_some_and(|e| e.tag_name().eq_ignore_ascii_case("ol"))
    {
        let ordinal = 1 + parent
            .children()
            .iter()
            .take_while(|&&id| id != node_id)
            .filter(|&&id| {
                styles
                    .get(id)
                    .is_some_and(|s| s.display == Display::ListItem)
            })
            .count();
        marker_text = Some(format!("{ordinal}."));
    }
    let mut layout_box = LayoutBox {
        box_id,
        parent_box,
        position_parent,
        coordinate_space: CoordinateSpace::Document,
        flow_width: 0.0,
        definite_height: None,
        definite_width: false,
        inline_line: None,
        is_flex_item: flex_parent && !style.position.out_of_flow() && kind != BoxKind::Text,
        flex_static: None,
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
        inline_fragments: Vec::new(),
        line_boxes: Vec::new(),
        list_marker: None,
        pseudo: None,
        marker_style,
        marker_text,
        inline_start: true,
        inline_end: true,
    };
    if layout_box.style.display.is_flex() && layout_box.image.is_none() {
        normalize_flex_children(&mut layout_box, identity);
    } else if kind == BoxKind::Block && layout_box.image.is_none() {
        normalize_block_children(&mut layout_box, identity);
    }
    identity.parent = parent_box;
    identity.positioned = position_parent;
    identity.flex_parent = flex_parent;
    Some(layout_box)
}

fn normalize_block_children(parent: &mut LayoutBox, identity: &mut Construction) {
    let children: Vec<_> = std::mem::take(&mut parent.children)
        .into_iter()
        .flat_map(split_inline)
        .collect();
    if !children
        .iter()
        .any(|child| child.kind == BoxKind::Block && !child.style.position.out_of_flow())
    {
        parent.children = children;
        return;
    }
    let mut run = Vec::new();
    for child in children {
        if child.kind == BoxKind::Block && !child.style.position.out_of_flow() {
            flush_inline_run(parent, &mut run, identity);
            parent.children.push(child);
        } else {
            run.push(child);
        }
    }
    flush_inline_run(parent, &mut run, identity);
}

fn normalize_flex_children(parent: &mut LayoutBox, identity: &mut Construction) {
    fn flush(parent: &mut LayoutBox, run: &mut Vec<LayoutBox>, identity: &mut Construction) {
        if run.iter().all(|b| {
            b.text.as_ref().is_none_or(|s| {
                s.chars()
                    .all(|c| matches!(c, ' ' | '\n' | '\r' | '\t' | '\u{c}'))
            })
        }) {
            run.clear();
            return;
        }
        flush_inline_run(parent, run, identity);
        if let Some(item) = parent.children.last_mut() {
            item.tag_name = "#anonymous-flex-item".into();
            item.is_flex_item = true;
        }
    }
    let mut run = Vec::new();
    for child in std::mem::take(&mut parent.children) {
        if child.kind == BoxKind::Text {
            run.push(child);
        } else {
            flush(parent, &mut run, identity);
            parent.children.push(child);
        }
    }
    flush(parent, &mut run, identity);
}

fn flush_inline_run(parent: &mut LayoutBox, run: &mut Vec<LayoutBox>, identity: &mut Construction) {
    // Discard only collapsible text, not empty real inline/pseudo boxes,
    // decorated inline elements, replaced content, or non-CSS spaces (NBSP).
    if run.iter().all(|child| {
        child.kind == BoxKind::Text
            && child.text.as_ref().is_none_or(|text| {
                text.chars().all(|c| match c {
                    ' ' | '\t' | '\r' => !child.style.white_space_collapse.preserves_spaces(),
                    '\n' => !child.style.white_space_collapse.preserves_breaks(),
                    _ => false,
                })
            })
    }) {
        run.clear();
        return;
    }
    let mut style = Box::new(ComputedStyle::inherit(&parent.style));
    style.display = Display::Block;
    parent.children.push(LayoutBox {
        box_id: identity.allocate(),
        parent_box: Some(parent.box_id),
        position_parent: if parent.style.position == style::Position::Static {
            parent.position_parent
        } else {
            Some(parent.box_id)
        },
        coordinate_space: CoordinateSpace::Document,
        flow_width: 0.0,
        definite_height: None,
        definite_width: false,
        inline_line: None,
        is_flex_item: false,
        flex_static: None,
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
        inline_fragments: Vec::new(),
        line_boxes: Vec::new(),
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
    if inline.kind != BoxKind::Inline
        || inline.atomic_inline()
        || inline.image.is_some()
        || inline.style.position.out_of_flow()
    {
        return vec![inline];
    }
    let children: Vec<_> = std::mem::take(&mut inline.children)
        .into_iter()
        .flat_map(split_inline)
        .collect();
    if !children
        .iter()
        .any(|child| child.kind == BoxKind::Block && !child.style.position.out_of_flow())
    {
        inline.children = children;
        return vec![inline];
    }
    let mut result = Vec::new();
    let mut run = Vec::new();
    for child in children {
        if child.kind == BoxKind::Block && !child.style.position.out_of_flow() {
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
    identity: &mut Construction,
) -> Option<LayoutBox> {
    let mut style = Box::new(styles.pseudo(node_id, pseudo)?.clone());
    if identity.flex_parent && !style.position.out_of_flow() {
        style.display = style.display.blockified();
    }
    if style.display == Display::None {
        return None;
    }
    let text = generated_text(&style.content, element, node_id, pseudo, diagnostics)?;
    let box_id = identity.allocate();
    let text_box = LayoutBox {
        box_id: identity.allocate(),
        parent_box: Some(box_id),
        position_parent: if style.position == style::Position::Static {
            identity.positioned
        } else {
            Some(box_id)
        },
        coordinate_space: CoordinateSpace::Document,
        flow_width: 0.0,
        definite_height: None,
        definite_width: false,
        inline_line: None,
        is_flex_item: false,
        flex_static: None,
        node_id: Some(node_id),
        tag_name: "#generated-text".into(),
        kind: BoxKind::Text,
        text: Some(text),
        style: Box::new(ComputedStyle::inherit(&style)),
        rect: Rect::default(),
        content_rect: Rect::default(),
        padding: Edges::default(),
        margin: Edges::default(),
        children: Vec::new(),
        text_fragments: Vec::new(),
        inline_fragments: Vec::new(),
        line_boxes: Vec::new(),
        list_marker: None,
        image: None,
        pseudo: Some(pseudo),
        marker_style: None,
        marker_text: None,
        inline_start: true,
        inline_end: true,
    };
    let mut generated = LayoutBox {
        box_id,
        parent_box: identity.parent,
        position_parent: identity.positioned,
        coordinate_space: CoordinateSpace::Document,
        flow_width: 0.0,
        definite_height: None,
        definite_width: false,
        inline_line: None,
        is_flex_item: identity.flex_parent && !style.position.out_of_flow(),
        flex_static: None,
        node_id: Some(node_id),
        tag_name: format!("::{}", pseudo.name()),
        kind: if matches!(
            style.display,
            Display::Block | Display::ListItem | Display::Flex
        ) {
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
        inline_fragments: Vec::new(),
        line_boxes: Vec::new(),
        list_marker: None,
        image: None,
        pseudo: Some(pseudo),
        marker_style: None,
        marker_text: None,
        inline_start: true,
        inline_end: true,
    };
    if generated.style.display.is_flex() {
        normalize_flex_children(&mut generated, identity);
    }
    Some(generated)
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
