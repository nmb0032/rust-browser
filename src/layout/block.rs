use crate::style::{Diagnostic, Direction, Display};

use super::{
    BoxKind, LayoutBox, ListMarker, Rect,
    inline::{InlineFlow, layout_inline, measure_text},
    tree::{finite_used, resolve_edges, resolve_length, resolve_line_height, resolve_margins},
};

pub(super) fn layout_block(
    layout_box: &mut LayoutBox,
    x: f32,
    y: f32,
    available_width: f32,
    containing_direction: Direction,
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let style = &layout_box.style;
    let margins = resolve_margins(style, available_width, layout_box.node_id, diagnostics);
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
    let horizontal = finite_used(
        padding.left as f64 + padding.right as f64 + border.left as f64 + border.right as f64,
        layout_box.node_id,
        "horizontal decorations",
        diagnostics,
    );
    let vertical = finite_used(
        padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
        layout_box.node_id,
        "vertical decorations",
        diagnostics,
    );
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
    let (width, left, right) = solve_width(
        available_width as f64,
        horizontal as f64,
        replaced_size
            .map(|size| size.0)
            .or(explicit_width)
            .map(f64::from),
        margins.left.map(f64::from),
        margins.right.map(f64::from),
        containing_direction,
    );
    let width = finite_used(width, layout_box.node_id, "width equation", diagnostics);
    let margin = crate::css::values::Edges {
        left: finite_used(
            left,
            layout_box.node_id,
            "margin-left equation",
            diagnostics,
        ),
        right: finite_used(
            right,
            layout_box.node_id,
            "margin-right equation",
            diagnostics,
        ),
        top: margins.top.unwrap_or(0.0),
        bottom: margins.bottom.unwrap_or(0.0),
    };
    let border_x = finite_used(
        x as f64 + margin.left as f64,
        layout_box.node_id,
        "border x",
        diagnostics,
    );
    let border_y = finite_used(
        y as f64 + margin.top as f64,
        layout_box.node_id,
        "border y",
        diagnostics,
    );
    let content_x = finite_used(
        border_x as f64 + border.left as f64 + padding.left as f64,
        layout_box.node_id,
        "content x",
        diagnostics,
    );
    let content_y = finite_used(
        border_y as f64 + border.top as f64 + padding.top as f64,
        layout_box.node_id,
        "content y",
        diagnostics,
    );
    layout_box.padding = padding;
    layout_box.margin = margin;
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
    let height = finite_used(height as f64, layout_box.node_id, "height", diagnostics);
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
            node_id: layout_box.node_id,
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
        width: finite_used(
            width as f64 + horizontal as f64,
            layout_box.node_id,
            "border width",
            diagnostics,
        ),
        height: finite_used(
            height as f64 + vertical as f64,
            layout_box.node_id,
            "border height",
            diagnostics,
        ),
    };
    finite_used(
        margin.top as f64 + layout_box.rect.height as f64 + margin.bottom as f64,
        layout_box.node_id,
        "margin height",
        diagnostics,
    )
}

/// CSS2 10.3.3 after resolving lengths and box-sizing, or 10.3.4 after
/// determining a supported replaced image's width. No intrinsic/min/max sizing.
fn solve_width(
    containing: f64,
    decorations: f64,
    width: Option<f64>,
    left: Option<f64>,
    right: Option<f64>,
    direction: Direction,
) -> (f64, f64, f64) {
    let mut used_left = left.unwrap_or(0.0);
    let mut used_right = right.unwrap_or(0.0);
    let width = match width {
        None => {
            let remaining = containing - decorations - used_left - used_right;
            if remaining >= 0.0 {
                return (remaining, used_left, used_right);
            }
            0.0
        }
        Some(width) => width.max(0.0),
    };
    let remaining = containing - decorations - width - used_left - used_right;
    // If the fixed terms overflow, auto margins become zero before solving.
    let allow_auto = width + decorations + used_left + used_right <= containing;
    match (left.is_none() && allow_auto, right.is_none() && allow_auto) {
        (true, true) => {
            used_left = remaining / 2.0;
            used_right = remaining / 2.0;
        }
        (true, false) => used_left = remaining,
        (false, true) => used_right = remaining,
        (false, false) => match direction {
            Direction::Ltr => used_right += remaining,
            Direction::Rtl => used_left += remaining,
        },
    }
    (width, used_left, used_right)
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
            cursor_y = finite_used(
                cursor_y as f64 + flow.content_height(),
                layout_box.node_id,
                "block flow y",
                diagnostics,
            );
            let child_height = layout_block(
                child,
                x,
                cursor_y,
                width,
                layout_box.style.direction,
                font,
                diagnostics,
            );
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
            cursor_y = finite_used(
                cursor_y as f64 + child_height as f64,
                layout_box.node_id,
                "block flow y",
                diagnostics,
            );
            flow = InlineFlow::new(x, cursor_y, width, line_height);
        } else {
            layout_inline(child, &mut flow, font, diagnostics);
        }
    }
    finite_used(
        cursor_y as f64 + flow.content_height() - y as f64,
        layout_box.node_id,
        "block flow height",
        diagnostics,
    )
}
