use crate::style::{Diagnostic, Display};

use super::{
    BoxKind, LayoutBox, ListMarker, Rect,
    inline::{InlineFlow, layout_inline, measure_text},
    tree::{resolve_edges, resolve_length, resolve_line_height},
};

pub(super) fn layout_block(
    layout_box: &mut LayoutBox,
    x: f32,
    y: f32,
    available_width: f32,
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let style = &layout_box.style;
    let margin = resolve_edges(
        &style.margin,
        style.font_size,
        available_width,
        layout_box.node_id,
        "margin",
        diagnostics,
    );
    let padding = resolve_edges(
        &style.padding,
        style.font_size,
        available_width,
        layout_box.node_id,
        "padding",
        diagnostics,
    )
    .map(|value| value.max(0.0));
    let border = style.border_width;
    let horizontal = padding.left + padding.right + border.left + border.right;
    let vertical = padding.top + padding.bottom + border.top + border.bottom;
    let explicit_width = style.width.as_ref().map(|length| {
        style.box_sizing.content_size(
            resolve_length(
                length,
                style.font_size,
                available_width,
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
    let replaced_size = layout_box.image.as_ref().map(|image| {
        let line_height = resolve_line_height(
            &style.line_height,
            style.font_size,
            layout_box.node_id,
            diagnostics,
        );
        image.size(style, explicit_width, explicit_height, line_height, font)
    });
    let width = replaced_size.map_or_else(
        || {
            explicit_width.map_or(
                (available_width - margin.left - margin.right - horizontal).max(0.0),
                |width| width.max(0.0),
            )
        },
        |(width, _)| width,
    );
    let border_x = x + margin.left;
    let border_y = y + margin.top;
    let content_x = border_x + border.left + padding.left;
    let content_y = border_y + border.top + padding.top;
    layout_box.padding = padding;
    let natural_height = if let Some((_, height)) = replaced_size {
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
        layout_children(layout_box, content_x, content_y, width, font, diagnostics)
    };
    // Size containment removes descendant intrinsic block-size contributions.
    // Children still lay out and may overflow this content box.
    let height = explicit_height.unwrap_or_else(|| {
        if layout_box.image.is_none()
            && layout_box.style.container_type == crate::css::container::ContainerType::Size
        {
            0.0
        } else {
            natural_height
        }
    });
    if layout_box
        .style
        .height
        .as_ref()
        .is_some_and(|length| length.has_percentage())
    {
        eprintln!(
            "CSS at {:?}: percentage height has no definite containing block height in block layout; using auto",
            layout_box.node_id
        );
        diagnostics.push(Diagnostic {
            node_id: Some(layout_box.node_id),
            message: "percentage height has no definite containing block height in block layout; using auto".into(),
        });
    }
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
        height: height + vertical,
    };
    margin.top + layout_box.rect.height + margin.bottom
}

pub(super) fn layout_children(
    layout_box: &mut LayoutBox,
    x: f32,
    y: f32,
    width: f32,
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let mut cursor_y = y;
    let line_height = resolve_line_height(
        &layout_box.style.line_height,
        layout_box.style.font_size,
        layout_box.node_id,
        diagnostics,
    );
    let mut flow = InlineFlow::new(x, y, width, line_height);
    let ordered = layout_box.tag_name == "<ol>";
    let mut list_index = 0;
    for child in &mut layout_box.children {
        if child.kind == BoxKind::Block {
            cursor_y += flow.content_height();
            let child_height = layout_block(child, x, cursor_y, width, font, diagnostics);
            if child.style.display == Display::ListItem {
                list_index += 1;
                let text = child.marker_text.clone().unwrap_or_else(|| {
                    if ordered {
                        format!("{list_index}.")
                    } else {
                        "\u{2022}".into()
                    }
                });
                let style = child.marker_style.as_ref().unwrap_or(&child.style).clone();
                child.list_marker = Some(ListMarker {
                    x: child.content_rect.x
                        - measure_text(font, &text, style.font_size)
                        - style.font_size * 0.5,
                    y: child.content_rect.y,
                    text,
                    style,
                });
            }
            cursor_y += child_height;
            flow = InlineFlow::new(x, cursor_y, width, line_height);
        } else {
            layout_inline(child, &mut flow, font, diagnostics);
        }
    }
    cursor_y + flow.content_height() - y
}
