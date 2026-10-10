use std::error::Error;

use crate::layout::{LayoutBox, LayoutTree};

pub(crate) const DEFAULT_VIEWPORT_WIDTH: f32 = 800.0;
pub(crate) const DEFAULT_VIEWPORT_HEIGHT: f32 = 600.0;
pub(crate) const LINE_HEIGHT: f32 = 20.0;
const TEXT_COLOR: u32 = 0x0000_0000;
const BACKGROUND_COLOR: u32 = 0x00FF_FFFF;
const OUTLINE_COLOR: u32 = 0x00FF_0000;

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayItem {
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: u32,
        bold: bool,
        italic: bool,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: u32,
    },
    OutlineRect {
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

pub(crate) fn build_display_list(
    layout: &LayoutTree,
    viewport_width: f32,
    viewport_height: f32,
    show_outlines: bool,
) -> Vec<DisplayItem> {
    let mut display_list = vec![DisplayItem::Rect {
        x: 0.0,
        y: 0.0,
        w: viewport_width,
        h: viewport_height,
        color: BACKGROUND_COLOR,
    }];

    append_box_items(layout.root(), &mut display_list);
    if show_outlines {
        append_box_outlines(layout.root(), &mut display_list);
    }
    display_list
}

fn append_box_items(layout_box: &LayoutBox, display_list: &mut Vec<DisplayItem>) {
    if let Some(marker) = layout_box.list_marker() {
        display_list.push(DisplayItem::Text {
            x: marker.x,
            y: marker.y,
            text: marker.text.clone(),
            size: 16.0,
            color: TEXT_COLOR,
            bold: false,
            italic: false,
        });
    }

    for fragment in layout_box.text_fragments() {
        display_list.push(DisplayItem::Text {
            x: fragment.x,
            y: fragment.y,
            text: fragment.text.clone(),
            size: fragment.font_size,
            color: TEXT_COLOR,
            bold: fragment.bold,
            italic: fragment.italic,
        });
    }

    for child in layout_box.children() {
        append_box_items(child, display_list);
    }
}

fn append_box_outlines(layout_box: &LayoutBox, display_list: &mut Vec<DisplayItem>) {
    let rect = layout_box.rect();
    if rect.width > 0.0 && rect.height > 0.0 {
        display_list.push(DisplayItem::OutlineRect {
            x: rect.x,
            y: rect.y,
            w: rect.width,
            h: rect.height,
            color: OUTLINE_COLOR,
        });
    }

    for child in layout_box.children() {
        append_box_outlines(child, display_list);
    }
}

pub(crate) fn max_scroll_offset(layout: &LayoutTree, viewport_height: f32) -> f32 {
    (layout.root().rect().height - viewport_height).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{html::parser, layout};

    fn layout(source: &str, width: f32) -> LayoutTree {
        let document = parser::parse(source).unwrap();
        let mut tree = layout::build_layout_tree(&document);
        tree.layout(width, &load_bundled_font().unwrap());
        tree
    }

    fn text_items(display_list: &[DisplayItem]) -> Vec<&DisplayItem> {
        display_list
            .iter()
            .filter(|item| matches!(item, DisplayItem::Text { .. }))
            .collect()
    }

    #[test]
    fn display_list_uses_layout_fragments_and_skips_non_rendered_elements() {
        let tree = layout(
            "<head><title>hidden title</title></head><p>first <em>nested</em></p>\
             <script>hidden script</script><style>hidden style</style><p>last</p>",
            800.0,
        );
        let display_list = build_display_list(&tree, 800.0, 600.0, false);
        let rendered_text = text_items(&display_list)
            .into_iter()
            .map(|item| match item {
                DisplayItem::Text { text, .. } => text.as_str(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered_text, ["first", "nested", "last"]);
        assert!(
            !display_list
                .iter()
                .any(|item| matches!(item, DisplayItem::OutlineRect { .. }))
        );
    }

    #[test]
    fn narrower_viewports_wrap_inline_fragments() {
        let source = "<p>one two three four five six</p>";
        let narrow = layout(source, 180.0);
        let wide = layout(source, 800.0);
        let narrow_list = build_display_list(&narrow, 180.0, 600.0, false);
        let wide_list = build_display_list(&wide, 800.0, 600.0, false);
        let line_count = |items: &[DisplayItem]| {
            let mut lines = text_items(items)
                .into_iter()
                .filter_map(|item| match item {
                    DisplayItem::Text { y, .. } => Some(*y),
                    _ => None,
                })
                .collect::<Vec<_>>();
            lines.sort_by(f32::total_cmp);
            lines.dedup_by(|left, right| (*left - *right).abs() < 0.01);
            lines.len()
        };

        assert!(line_count(&narrow_list) > line_count(&wide_list));
    }

    #[test]
    fn list_markers_and_inline_emphasis_reach_the_display_list() {
        let tree = layout(
            "<ul><li><strong>bold</strong> <em>italic</em></li></ul>",
            400.0,
        );
        let display_list = build_display_list(&tree, 400.0, 600.0, true);
        let texts = text_items(&display_list);

        assert!(texts.iter().any(|item| matches!(
            item,
            DisplayItem::Text { text, .. } if text == "\u{2022}"
        )));
        assert!(texts.iter().any(|item| matches!(
            item,
            DisplayItem::Text { text, bold: true, .. } if text == "bold"
        )));
        assert!(texts.iter().any(|item| matches!(
            item,
            DisplayItem::Text { text, italic: true, .. } if text == "italic"
        )));
        assert!(
            display_list
                .iter()
                .any(|item| matches!(item, DisplayItem::OutlineRect { .. }))
        );
    }

    #[test]
    fn max_scroll_offset_tracks_layout_content_height() {
        let short = layout("<p>short</p>", 800.0);
        let long = layout(&format!("<p>{}</p>", "word ".repeat(1000)), 800.0);

        assert_eq!(max_scroll_offset(&short, 600.0), 0.0);
        assert!(max_scroll_offset(&long, 600.0) > 0.0);
    }
}
