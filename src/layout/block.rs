use crate::style::Display;

use super::{
    BoxKind, LayoutBox, ListMarker, Rect,
    inline::{InlineFlow, layout_inline, measure_text},
};

pub(super) fn layout_block(
    layout_box: &mut LayoutBox,
    x: f32,
    y: f32,
    available_width: f32,
    font: &fontdue::Font,
) -> f32 {
    let style = &layout_box.style;
    let margin = style
        .margin
        .map(|length| length.resolve(style.font_size, available_width));
    let padding = style
        .padding
        .map(|length| length.resolve(style.font_size, available_width));
    let border = style.border_width;
    let horizontal = padding.left + padding.right + border.left + border.right;
    let replaced_size = layout_box
        .image
        .as_ref()
        .map(|image| image.size(style, available_width, font));
    let width = replaced_size.map_or_else(
        || {
            style.width.map_or(
                (available_width - margin.left - margin.right - horizontal).max(0.0),
                |length| length.resolve(style.font_size, available_width).max(0.0),
            )
        },
        |(width, _)| width,
    );
    let border_x = x + margin.left;
    let border_y = y + margin.top;
    let content_x = border_x + border.left + padding.left;
    let content_y = border_y + border.top + padding.top;
    layout_box.padding = padding;
    let height = if let Some((_, height)) = replaced_size {
        if let Some(image) = &layout_box.image
            && let Some(fragment) = image.alt_fragment(
                Rect {
                    x: content_x,
                    y: content_y,
                    width,
                    height,
                },
                &layout_box.style,
            )
        {
            layout_box.text_fragments.push(fragment);
        }
        height
    } else {
        layout_children(layout_box, content_x, content_y, width, font)
    };
    layout_box.content_rect = Rect {
        x: content_x,
        y: content_y,
        width,
        height,
    };
    layout_box.rect = Rect {
        x: border_x,
        y: border_y,
        width: width + horizontal,
        height: height + padding.top + padding.bottom + border.top + border.bottom,
    };
    margin.top + layout_box.rect.height + margin.bottom
}

pub(super) fn layout_children(
    layout_box: &mut LayoutBox,
    x: f32,
    y: f32,
    width: f32,
    font: &fontdue::Font,
) -> f32 {
    let mut cursor_y = y;
    let font_size = layout_box.style.font_size;
    let mut flow = InlineFlow::new(x, y, width, font_size);
    let ordered = layout_box.tag_name == "<ol>";
    let mut list_index = 0;
    for child in &mut layout_box.children {
        if child.kind == BoxKind::Block {
            cursor_y += flow.content_height();
            let child_height = layout_block(child, x, cursor_y, width, font);
            if child.style.display == Display::ListItem {
                list_index += 1;
                let text = if ordered {
                    format!("{list_index}.")
                } else {
                    "\u{2022}".into()
                };
                child.list_marker = Some(ListMarker {
                    x: child.content_rect.x
                        - measure_text(font, &text, child.style.font_size)
                        - child.style.font_size * 0.5,
                    y: child.content_rect.y,
                    text,
                });
            }
            cursor_y += child_height;
            flow = InlineFlow::new(x, cursor_y, width, font_size);
        } else {
            layout_inline(child, &mut flow, font);
        }
    }
    cursor_y + flow.content_height() - y
}
