use crate::style::ComputedStyle;

use super::{BoxKind, LayoutBox, Rect, TextFragment, block::layout_block};

pub(super) fn layout_inline(
    layout_box: &mut LayoutBox,
    flow: &mut InlineFlow,
    font: &fontdue::Font,
) {
    if layout_box.kind == BoxKind::Block {
        let y = flow.start_y + flow.content_height();
        let height = layout_block(layout_box, flow.start_x, y, flow.width, font);
        flow.y = y + height;
        flow.current_x = flow.start_x;
        flow.line_height = flow.base_line_height;
        flow.has_line = false;
        flow.line_has_content = false;
        flow.pending_space = false;
        return;
    }
    if layout_box.tag_name == "<br>" {
        layout_box.rect = flow.break_line();
        layout_box.content_rect = layout_box.rect;
        return;
    }
    if layout_box.image.is_some() {
        layout_image(layout_box, flow, font);
        return;
    }
    if layout_box.kind == BoxKind::Text {
        let fragments = flow.add_text(
            layout_box.text.as_deref().unwrap_or_default(),
            &layout_box.style,
            font,
        );
        layout_box.rect = fragments.iter().fold(Rect::default(), |bounds, fragment| {
            bounds.union(Rect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
        });
        layout_box.content_rect = layout_box.rect;
        layout_box.text_fragments = fragments;
        return;
    }
    let style = &layout_box.style;
    let padding = style
        .padding
        .map(|length| length.resolve(style.font_size, flow.width));
    let margin = style
        .margin
        .map(|length| length.resolve(style.font_size, flow.width));
    let border = style.border_width;
    let left = padding.left + border.left;
    let right = padding.right + border.right;
    let extra_height = padding.top + padding.bottom + border.top + border.bottom;
    flow.advance(margin.left + left, style.font_size * 1.25 + extra_height);
    let start = Rect {
        x: flow.current_x,
        y: flow.y,
        width: 0.0,
        height: style.font_size * 1.25,
    };
    for child in &mut layout_box.children {
        layout_inline(child, flow, font);
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
    layout_box.content_rect = content;
    layout_box.rect = Rect {
        x: content.x - left,
        y: content.y - padding.top - border.top,
        width: content.width + left + right,
        height: content.height + extra_height,
    };
    flow.advance(right + margin.right, style.font_size * 1.25 + extra_height);
}

fn layout_image(layout_box: &mut LayoutBox, flow: &mut InlineFlow, font: &fontdue::Font) {
    let image = layout_box
        .image
        .as_ref()
        .expect("image layout has image content");
    let style = &layout_box.style;
    let (width, height) = image.size(style, flow.width, font);
    let padding = style
        .padding
        .map(|length| length.resolve(style.font_size, flow.width));
    let margin = style
        .margin
        .map(|length| length.resolve(style.font_size, flow.width));
    let border = style.border_width;
    let box_width = width + padding.left + padding.right + border.left + border.right;
    let box_height = height + padding.top + padding.bottom + border.top + border.bottom;
    let separator = if flow.pending_space && flow.line_has_content {
        font.metrics(' ', style.font_size).advance_width
    } else {
        0.0
    };
    if flow.line_has_content
        && flow.current_x + separator + margin.left + box_width + margin.right
            > flow.start_x + flow.width
    {
        flow.next_line();
    } else {
        flow.current_x += separator;
    }
    let x = flow.current_x + margin.left;
    let y = flow.y + margin.top;
    layout_box.padding = padding;
    layout_box.rect = Rect {
        x,
        y,
        width: box_width,
        height: box_height,
    };
    layout_box.content_rect = Rect {
        x: x + border.left + padding.left,
        y: y + border.top + padding.top,
        width,
        height,
    };
    if let Some(fragment) = image.alt_fragment(layout_box.content_rect, style) {
        layout_box.text_fragments.push(fragment);
    }
    flow.advance(
        margin.left + box_width + margin.right,
        margin.top + box_height + margin.bottom,
    );
    if box_height > 0.0 {
        flow.line_height = flow
            .line_height
            .max(margin.top + box_height + margin.bottom);
        flow.has_line = true;
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
    pub(super) fn new(x: f32, y: f32, width: f32, font_size: f32) -> Self {
        let line_height = font_size * 1.25;
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

    fn advance(&mut self, width: f32, height: f32) {
        self.current_x += width;
        if width != 0.0 {
            self.line_has_content = true;
            self.has_line = true;
            self.line_height = self.line_height.max(height);
        }
    }

    fn add_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        font: &fontdue::Font,
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
            fragments.push(self.add_word(word, style, font));
            self.pending_space = index + 1 < words.len() || ends_with_space;
        }
        fragments
    }

    fn add_word(
        &mut self,
        word: &str,
        style: &ComputedStyle,
        font: &fontdue::Font,
    ) -> TextFragment {
        let word_width = measure_text(font, word, style.font_size);
        let separator_width = if self.pending_space && self.line_has_content {
            font.metrics(' ', style.font_size).advance_width
        } else {
            0.0
        };
        if self.line_has_content
            && self.current_x + separator_width + word_width > self.start_x + self.width
        {
            self.next_line();
        } else {
            self.current_x += separator_width;
        }
        self.line_height = self.line_height.max(style.font_size * 1.25);
        let fragment = TextFragment {
            x: self.current_x,
            y: self.y,
            width: word_width,
            height: style.font_size * 1.25,
            text: word.to_string(),
            font_size: style.font_size,
            color: style.color,
            bold: style.bold,
            italic: style.italic,
        };
        self.current_x += word_width;
        self.line_has_content = true;
        self.has_line = true;
        self.pending_space = false;
        fragment
    }

    fn break_line(&mut self) -> Rect {
        let rect = Rect {
            x: self.current_x,
            y: self.y,
            width: 1.0,
            height: self.line_height,
        };
        self.next_line();
        rect
    }

    fn next_line(&mut self) {
        self.y += self.line_height;
        self.current_x = self.start_x;
        self.line_height = self.base_line_height;
        self.line_has_content = false;
        self.has_line = true;
        self.pending_space = false;
    }

    pub(super) fn content_height(&self) -> f32 {
        self.y - self.start_y + if self.has_line { self.line_height } else { 0.0 }
    }
}

pub(super) fn measure_text(font: &fontdue::Font, text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|character| font.metrics(character, font_size).advance_width)
        .sum()
}
