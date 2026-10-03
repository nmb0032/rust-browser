use std::error::Error;

use crate::dom::{Document, NodeKind};

pub(crate) const DEFAULT_VIEWPORT_WIDTH: f32 = 800.0;
pub(crate) const DEFAULT_VIEWPORT_HEIGHT: f32 = 600.0;
const HORIZONTAL_PADDING: f32 = 24.0;
const FONT_SIZE: f32 = 32.0;
pub(crate) const LINE_HEIGHT: f32 = FONT_SIZE * 1.4;
const TEXT_COLOR: u32 = 0x0000_0000;
const BACKGROUND_COLOR: u32 = 0x00FF_FFFF;

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayItem {
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: u32,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: u32,
    },
}

pub(crate) fn load_bundled_font() -> Result<fontdue::Font, Box<dyn Error>> {
    let font_data = include_bytes!("../../assets/fonts/noto-sans/NotoSans.ttf");
    let font = fontdue::Font::from_bytes(font_data.as_slice(), fontdue::FontSettings::default())
        .map_err(|error| std::io::Error::other(format!("failed to load bundled font: {error}")))?;
    Ok(font)
}

pub(crate) fn collect_document_text(document: &Document) -> String {
    let mut raw_text = String::new();
    let mut pending = vec![document.root_id()];

    while let Some(id) = pending.pop() {
        let node = document
            .node(id)
            .expect("document contains an invalid child node ID");

        match node.kind() {
            NodeKind::Text(text) => raw_text.push_str(text),
            NodeKind::Element(element)
                if matches!(element.tag_name(), "head" | "script" | "style") => {}
            NodeKind::Document | NodeKind::Element(_) => {
                pending.extend(node.children().iter().rev().copied());
            }
        }
    }

    raw_text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn build_display_list(
    text: &str,
    font: &fontdue::Font,
    viewport_width: f32,
    viewport_height: f32,
) -> Vec<DisplayItem> {
    let mut display_list = vec![DisplayItem::Rect {
        x: 0.0,
        y: 0.0,
        w: viewport_width,
        h: viewport_height,
        color: BACKGROUND_COLOR,
    }];

    let available_width = (viewport_width - 2.0 * HORIZONTAL_PADDING).max(0.0);
    let space_width = font.metrics(' ', FONT_SIZE).advance_width;
    let mut line = String::new();
    let mut line_width = 0.0;
    let mut line_y = HORIZONTAL_PADDING;

    for word in text.split_whitespace() {
        let word_width = measure_text(font, word, FONT_SIZE);
        let separator_width = if line.is_empty() { 0.0 } else { space_width };

        if !line.is_empty() && line_width + separator_width + word_width > available_width {
            display_list.push(DisplayItem::Text {
                x: HORIZONTAL_PADDING,
                y: line_y,
                text: std::mem::take(&mut line),
                size: FONT_SIZE,
                color: TEXT_COLOR,
            });
            line_y += LINE_HEIGHT;
            line_width = 0.0;
        }

        if !line.is_empty() {
            line.push(' ');
            line_width += space_width;
        }
        line.push_str(word);
        line_width += word_width;
    }

    if !line.is_empty() {
        display_list.push(DisplayItem::Text {
            x: HORIZONTAL_PADDING,
            y: line_y,
            text: line,
            size: FONT_SIZE,
            color: TEXT_COLOR,
        });
    }

    display_list
}

pub(crate) fn max_scroll_offset(display_list: &[DisplayItem], viewport_height: f32) -> f32 {
    let content_bottom = display_list
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { y, .. } => Some(y + LINE_HEIGHT),
            DisplayItem::Rect { .. } => None,
        })
        .fold(0.0, f32::max);

    (content_bottom + HORIZONTAL_PADDING - viewport_height).max(0.0)
}

fn measure_text(font: &fontdue::Font, text: &str, px: f32) -> f32 {
    text.chars()
        .map(|character| font.metrics(character, px).advance_width)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::parser;

    fn font() -> fontdue::Font {
        load_bundled_font().unwrap()
    }

    fn text_items(display_list: &[DisplayItem]) -> Vec<&DisplayItem> {
        display_list
            .iter()
            .filter(|item| matches!(item, DisplayItem::Text { .. }))
            .collect()
    }

    #[test]
    fn collects_text_in_dom_order_and_skips_non_rendered_elements() {
        let document = parser::parse(
            "<head><title>hidden title</title></head> <p>first <em>nested</em></p> \
             <script>hidden script</script> <style>hidden style</style> <p>last</p>",
        )
        .unwrap();

        assert_eq!(collect_document_text(&document), "first nested last");
    }

    #[test]
    fn wraps_words_using_the_font_advance_width() {
        let font = font();
        let text = "one two three four five six";
        let narrow = build_display_list(text, &font, 180.0, 600.0);
        let wide = build_display_list(text, &font, 800.0, 600.0);

        assert!(text_items(&narrow).len() > text_items(&wide).len());
        assert_eq!(
            text_items(&wide)
                .into_iter()
                .map(|item| match item {
                    DisplayItem::Text { text, .. } => text.as_str(),
                    DisplayItem::Rect { .. } => unreachable!(),
                })
                .collect::<Vec<_>>(),
            [text]
        );
    }

    #[test]
    fn max_scroll_offset_is_zero_for_short_content_and_positive_for_long_content() {
        let font = font();
        let short = build_display_list("short", &font, 800.0, 600.0);
        let long = build_display_list(&"word ".repeat(200), &font, 800.0, 600.0);

        assert_eq!(max_scroll_offset(&short, 600.0), 0.0);
        assert!(max_scroll_offset(&long, 600.0) > 0.0);
    }
}
