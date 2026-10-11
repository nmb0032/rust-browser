use crate::{
    dom::NodeId,
    style::{ComputedStyle, Diagnostic},
};

use super::{
    BoxKind, LayoutBox, Rect, TextFragment,
    tree::{
        finite_rect, finite_used, resolve_edges, resolve_length, resolve_line_height,
        resolve_margins,
    },
};

pub(super) fn layout_inline(
    layout_box: &mut LayoutBox,
    flow: &mut InlineFlow,
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) {
    debug_assert_ne!(layout_box.kind, BoxKind::Block);
    if layout_box.tag_name == "<br>" {
        layout_box.rect = flow.break_line(layout_box.node_id, diagnostics);
        layout_box.content_rect = layout_box.rect;
        return;
    }
    if layout_box.image.is_some() {
        layout_image(layout_box, flow, font, diagnostics);
        return;
    }
    if layout_box.kind == BoxKind::Text {
        let line_height = resolve_line_height(
            &layout_box.style.line_height,
            layout_box.style.font_size,
            layout_box.node_id,
            diagnostics,
        );
        let fragments = flow.add_text(
            layout_box.text.as_deref().unwrap_or_default(),
            &layout_box.style,
            font,
            line_height,
            layout_box.node_id,
            diagnostics,
        );
        layout_box.rect = finite_rect(
            fragments.iter().fold(Rect::default(), |bounds, fragment| {
                bounds.union(Rect {
                    x: fragment.x,
                    y: fragment.y,
                    width: fragment.width,
                    height: fragment.height,
                })
            }),
            layout_box.node_id,
            diagnostics,
        );
        layout_box.content_rect = layout_box.rect;
        layout_box.text_fragments = fragments;
        return;
    }
    let style = &layout_box.style;
    let padding = layout_box.inline_edges(
        resolve_edges(
            &style.padding,
            style.font_size,
            flow.width,
            layout_box.node_id,
            "padding",
            diagnostics,
        )
        .map(|value| value.max(0.0)),
    );
    let margin = layout_box.inline_edges(
        resolve_margins(style, flow.width, layout_box.node_id, diagnostics)
            .map(|value| value.unwrap_or(0.0)),
    );
    let line_height = resolve_line_height(
        &style.line_height,
        style.font_size,
        layout_box.node_id,
        diagnostics,
    );
    let border = layout_box.inline_edges(style.border_width);
    let left = finite_used(
        padding.left as f64 + border.left as f64,
        layout_box.node_id,
        "inline left decorations",
        diagnostics,
    );
    let right = finite_used(
        padding.right as f64 + border.right as f64,
        layout_box.node_id,
        "inline right decorations",
        diagnostics,
    );
    let extra_height = finite_used(
        padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
        layout_box.node_id,
        "inline vertical decorations",
        diagnostics,
    );
    flow.advance(
        margin.left as f64 + left as f64,
        line_height as f64 + extra_height as f64,
        layout_box.node_id,
        diagnostics,
    );
    if left != 0.0
        || right != 0.0
        || extra_height != 0.0
        || margin.left != 0.0
        || margin.right != 0.0
    {
        flow.has_line = true;
    }
    let start = Rect {
        x: flow.current_x,
        y: flow.y,
        width: 0.0,
        height: line_height,
    };
    for child in &mut layout_box.children {
        layout_inline(child, flow, font, diagnostics);
    }
    let bounds = layout_box
        .children
        .iter()
        .fold(Rect::default(), |bounds, child| bounds.union(child.rect));
    let content = if bounds == Rect::default() {
        start
    } else {
        bounds
    };
    layout_box.padding = padding;
    layout_box.margin = margin;
    layout_box.content_rect = finite_rect(content, layout_box.node_id, diagnostics);
    layout_box.rect = Rect {
        x: finite_used(
            content.x as f64 - left as f64,
            layout_box.node_id,
            "inline border x",
            diagnostics,
        ),
        y: finite_used(
            content.y as f64 - padding.top as f64 - border.top as f64,
            layout_box.node_id,
            "inline border y",
            diagnostics,
        ),
        width: finite_used(
            content.width as f64 + left as f64 + right as f64,
            layout_box.node_id,
            "inline border width",
            diagnostics,
        ),
        height: finite_used(
            content.height as f64 + extra_height as f64,
            layout_box.node_id,
            "inline border height",
            diagnostics,
        ),
    };
    flow.advance(
        right as f64 + margin.right as f64,
        line_height as f64 + extra_height as f64,
        layout_box.node_id,
        diagnostics,
    );
}

fn layout_image(
    layout_box: &mut LayoutBox,
    flow: &mut InlineFlow,
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let image = layout_box
        .image
        .as_ref()
        .expect("image layout has image content");
    let style = &layout_box.style;
    let line_height = resolve_line_height(
        &style.line_height,
        style.font_size,
        layout_box.node_id,
        diagnostics,
    );
    let padding = resolve_edges(
        &style.padding,
        style.font_size,
        flow.width,
        layout_box.node_id,
        "padding",
        diagnostics,
    )
    .map(|value| value.max(0.0));
    let margin = resolve_margins(style, flow.width, layout_box.node_id, diagnostics)
        .map(|value| value.unwrap_or(0.0));
    let border = style.border_width;
    let horizontal = finite_used(
        padding.left as f64 + padding.right as f64 + border.left as f64 + border.right as f64,
        layout_box.node_id,
        "image horizontal decorations",
        diagnostics,
    );
    let vertical = finite_used(
        padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
        layout_box.node_id,
        "image vertical decorations",
        diagnostics,
    );
    let explicit_width = style.width.as_ref().map(|length| {
        style.box_sizing.content_size(
            resolve_length(
                length,
                style.font_size,
                flow.width,
                layout_box.node_id,
                "width",
                diagnostics,
            ),
            horizontal,
        )
    });
    let explicit_height = style
        .height
        .as_ref()
        .filter(|length| !length.has_percentage())
        .map(|length| {
            style.box_sizing.content_size(
                resolve_length(
                    length,
                    style.font_size,
                    0.0,
                    layout_box.node_id,
                    "height",
                    diagnostics,
                ),
                vertical,
            )
        });
    let (width, height) = image.size(style, explicit_width, explicit_height, line_height, font);
    let box_width = finite_used(
        width as f64 + horizontal as f64,
        layout_box.node_id,
        "image border width",
        diagnostics,
    );
    let box_height = finite_used(
        height as f64 + vertical as f64,
        layout_box.node_id,
        "image border height",
        diagnostics,
    );
    let separator = if flow.pending_space && flow.line_has_content {
        font.metrics(' ', style.font_size).advance_width
    } else {
        0.0
    };
    if flow.line_has_content
        && flow.current_x as f64
            + separator as f64
            + margin.left as f64
            + box_width as f64
            + margin.right as f64
            > flow.start_x as f64 + flow.width as f64
    {
        flow.next_line(layout_box.node_id, diagnostics);
    } else {
        flow.current_x = finite_used(
            flow.current_x as f64 + separator as f64,
            layout_box.node_id,
            "image separator x",
            diagnostics,
        );
    }
    let x = finite_used(
        flow.current_x as f64 + margin.left as f64,
        layout_box.node_id,
        "image border x",
        diagnostics,
    );
    let y = finite_used(
        flow.y as f64 + margin.top as f64,
        layout_box.node_id,
        "image border y",
        diagnostics,
    );
    layout_box.padding = padding;
    layout_box.margin = margin;
    layout_box.rect = Rect {
        x,
        y,
        width: box_width,
        height: box_height,
    };
    layout_box.content_rect = Rect {
        x: finite_used(
            x as f64 + border.left as f64 + padding.left as f64,
            layout_box.node_id,
            "image content x",
            diagnostics,
        ),
        y: finite_used(
            y as f64 + border.top as f64 + padding.top as f64,
            layout_box.node_id,
            "image content y",
            diagnostics,
        ),
        width,
        height,
    };
    if let Some(fragment) = image.alt_fragment(layout_box.content_rect, style) {
        layout_box.text_fragments.push(fragment);
    }
    flow.advance(
        margin.left as f64 + box_width as f64 + margin.right as f64,
        margin.top as f64 + box_height as f64 + margin.bottom as f64,
        layout_box.node_id,
        diagnostics,
    );
    // Even a zero-sized replaced box is content, not a phantom inline.
    flow.has_line = true;
    if box_height > 0.0 {
        flow.line_height = flow.line_height.max(finite_used(
            margin.top as f64 + box_height as f64 + margin.bottom as f64,
            layout_box.node_id,
            "image margin height",
            diagnostics,
        ));
    }
    flow.pending_space = false;
}

pub(super) struct InlineFlow {
    pub(super) start_x: f32,
    pub(super) start_y: f32,
    pub(super) y: f32,
    pub(super) current_x: f32,
    pub(super) width: f32,
    pub(super) base_line_height: f32,
    pub(super) line_height: f32,
    pub(super) line_has_content: bool,
    pub(super) has_line: bool,
    pub(super) pending_space: bool,
}

impl InlineFlow {
    pub(super) fn new(x: f32, y: f32, width: f32, line_height: f32) -> Self {
        Self {
            start_x: x,
            start_y: y,
            y,
            current_x: x,
            width,
            base_line_height: line_height,
            line_height,
            line_has_content: false,
            has_line: false,
            pending_space: false,
        }
    }

    fn advance(
        &mut self,
        width: f64,
        height: f64,
        node: Option<NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        self.current_x = finite_used(
            self.current_x as f64 + width,
            node,
            "inline flow x",
            diagnostics,
        );
        if width != 0.0 {
            self.line_has_content = true;
            self.has_line = true;
            self.line_height =
                self.line_height
                    .max(finite_used(height, node, "inline flow height", diagnostics));
        }
    }

    fn add_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        font: &fontdue::Font,
        line_height: f32,
        node: Option<NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Vec<TextFragment> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            self.pending_space |= text.chars().any(char::is_whitespace);
            return Vec::new();
        }
        let mut fragments = Vec::with_capacity(words.len());
        let starts_with_space = text.chars().next().is_some_and(char::is_whitespace);
        let ends_with_space = text.chars().next_back().is_some_and(char::is_whitespace);
        for (index, word) in words.iter().enumerate() {
            if index > 0 || (index == 0 && starts_with_space) {
                self.pending_space = true;
            }
            fragments.push(self.add_word(word, style, font, line_height, node, diagnostics));
            self.pending_space = index + 1 < words.len() || ends_with_space;
        }
        fragments
    }

    fn add_word(
        &mut self,
        word: &str,
        style: &ComputedStyle,
        font: &fontdue::Font,
        line_height: f32,
        node: Option<NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> TextFragment {
        let word_width = measure_text(font, word, style.font_size);
        let separator_width = if self.pending_space && self.line_has_content {
            font.metrics(' ', style.font_size).advance_width
        } else {
            0.0
        };
        if self.line_has_content
            && self.current_x as f64 + separator_width as f64 + word_width as f64
                > self.start_x as f64 + self.width as f64
        {
            self.next_line(node, diagnostics);
        } else {
            self.current_x = finite_used(
                self.current_x as f64 + separator_width as f64,
                node,
                "text separator x",
                diagnostics,
            );
        }
        self.line_height = self.line_height.max(line_height);
        let fragment = TextFragment {
            x: self.current_x,
            y: self.y,
            width: word_width,
            height: line_height,
            text: word.to_string(),
            font_size: style.font_size,
            color: style.text_fill_color.legacy(),
            text_color: style.text_fill_color,
            bold: style.bold,
            italic: style.italic,
        };
        self.current_x = finite_used(
            self.current_x as f64 + word_width as f64,
            node,
            "text flow x",
            diagnostics,
        );
        self.line_has_content = true;
        self.has_line = true;
        self.pending_space = false;
        fragment
    }

    fn break_line(&mut self, node: Option<NodeId>, diagnostics: &mut Vec<Diagnostic>) -> Rect {
        let rect = Rect {
            x: self.current_x,
            y: self.y,
            width: 1.0,
            height: self.line_height,
        };
        self.next_line(node, diagnostics);
        rect
    }

    fn next_line(&mut self, node: Option<NodeId>, diagnostics: &mut Vec<Diagnostic>) {
        self.y = finite_used(
            self.y as f64 + self.line_height as f64,
            node,
            "inline flow y",
            diagnostics,
        );
        self.current_x = self.start_x;
        self.line_height = self.base_line_height;
        self.line_has_content = false;
        self.has_line = true;
        self.pending_space = false;
    }

    pub(super) fn content_height(&self) -> f64 {
        self.y as f64 - self.start_y as f64
            + if self.has_line {
                self.line_height as f64
            } else {
                0.0
            }
    }
}

pub(super) fn measure_text(font: &fontdue::Font, text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|character| font.metrics(character, font_size).advance_width)
        .sum()
}
