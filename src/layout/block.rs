use crate::{
    css::container::ContainerType,
    style::{Diagnostic, Direction, Display},
};

use super::{
    BoxKind, LayoutBox, ListMarker, Rect,
    inline::{font_metrics, layout_inline_run},
    tree::{finite_used, resolve_edges, resolve_length, resolve_margins},
};

/// An adjoining set must retain both extrema until all its members are known.
/// Reducing a mixed-sign set to a scalar before merging loses information.
#[derive(Clone, Copy, Debug, Default)]
struct MarginStrut {
    positive: f32,
    negative: f32,
}

impl MarginStrut {
    fn new(margin: f32) -> Self {
        Self {
            positive: margin.max(0.0),
            negative: margin.min(0.0),
        }
    }

    fn merge(self, other: Self) -> Self {
        Self {
            positive: self.positive.max(other.positive),
            negative: self.negative.min(other.negative),
        }
    }

    fn value(self) -> f64 {
        self.positive as f64 + self.negative as f64
    }
}

struct BlockFlow {
    before: MarginStrut,
    after: MarginStrut,
    through: bool,
}

#[derive(Default)]
struct ChildrenFlow {
    height: f64,
    before: MarginStrut,
    after: MarginStrut,
    all_through: bool,
}

#[derive(Clone, Copy)]
enum BlockMode {
    Root(Option<f32>),
    Flow(Option<f32>),
    Positioned {
        width: f32,
        height: Option<f32>,
        definite: (bool, bool),
    },
}

fn layout_block(
    layout_box: &mut LayoutBox,
    x: f32,
    available_width: f32,
    containing_direction: Direction,
    mode: BlockMode,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> BlockFlow {
    layout_box.flow_width = available_width;
    layout_box.definite_width = match mode {
        BlockMode::Positioned { definite, .. } => definite.0,
        _ => true,
    };
    let containing_height = match mode {
        BlockMode::Root(height) | BlockMode::Flow(height) => height,
        BlockMode::Positioned { .. } => None,
    };
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
        .filter(|length| !length.has_percentage() || containing_height.is_some())
        .map(|length| {
            style.box_sizing.content_size(
                resolve_length(
                    length,
                    style.font_size,
                    containing_height.unwrap_or(0.0),
                    layout_box.node_id,
                    "height",
                    diagnostics,
                ),
                vertical,
            )
        });
    let explicit_height = match mode {
        BlockMode::Positioned { height, .. } => height,
        _ => explicit_height,
    };
    let mut replaced_size = layout_box.image.as_ref().map(|image| {
        let metrics = font_metrics(font, style, layout_box.node_id, diagnostics);
        let line_height = metrics.above + metrics.below;
        image.size(
            style,
            match mode {
                BlockMode::Positioned { width, .. } => Some(width),
                _ => explicit_width,
            },
            explicit_height,
            line_height,
            font,
        )
    });
    let (mut width, mut left, mut right) = if let BlockMode::Positioned { width, .. } = mode {
        (
            f64::from(width),
            f64::from(margins.left.unwrap_or(0.0)),
            f64::from(margins.right.unwrap_or(0.0)),
        )
    } else {
        solve_width(
            available_width as f64,
            horizontal as f64,
            replaced_size
                .map(|size| size.0)
                .or(explicit_width)
                .map(f64::from),
            margins.left.map(f64::from),
            margins.right.map(f64::from),
            containing_direction,
        )
    };
    if !matches!(mode, BlockMode::Positioned { .. }) {
        let bounds = super::sizing::bounds(
            layout_box,
            true,
            Some(available_width),
            horizontal,
            0.0,
            diagnostics,
        );
        let constrained = (width as f32).clamp(bounds.0, bounds.1);
        if constrained != width as f32 {
            (width, left, right) = solve_width(
                available_width as f64,
                horizontal as f64,
                Some(constrained as f64),
                margins.left.map(f64::from),
                margins.right.map(f64::from),
                containing_direction,
            );
        }
    }
    let width = finite_used(width, layout_box.node_id, "width equation", diagnostics);
    if let Some(image) = &layout_box.image
        && explicit_height.is_none()
        && replaced_size.is_some_and(|size| size.0 != width)
    {
        let metrics = font_metrics(font, &layout_box.style, layout_box.node_id, diagnostics);
        replaced_size = Some(image.size(
            &layout_box.style,
            Some(width),
            None,
            metrics.above + metrics.below,
            font,
        ));
    }
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
    // Block descendants use coordinates relative to this border origin.
    // Placement changes only this box; tree.rs absolutizes the tree once.
    let border_y = 0.0;
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
    layout_box.definite_height = if matches!(
        mode,
        BlockMode::Positioned {
            definite: (_, false),
            ..
        }
    ) {
        None
    } else {
        explicit_height
    };
    let collapse_children = matches!(mode, BlockMode::Flow(_))
        && !layout_box.style.display.is_flex()
        && !layout_box.is_flex_item
        && layout_box.image.is_none()
        && layout_box.style.container_type == ContainerType::Normal
        && !layout_box.style.overflow_x.scroll_container()
        && !layout_box.style.overflow_y.scroll_container();
    let collapse_top = collapse_children && border.top == 0.0 && padding.top == 0.0;
    let collapse_bottom = collapse_children
        && explicit_height.is_none()
        && border.bottom == 0.0
        && padding.bottom == 0.0;
    let mut children_flow = ChildrenFlow::default();
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
                font,
                layout_box.node_id,
                diagnostics,
            )
        {
            layout_box.text_fragments.push(fragment);
        }
        height as f64
    } else if layout_box.style.display.is_flex() {
        super::flex::layout_flex(
            layout_box,
            (content_x, content_y, width),
            explicit_height,
            layout_box.definite_height,
            font,
            diagnostics,
        ) as f64
    } else {
        children_flow = flow_children(
            layout_box,
            (content_x, content_y, width),
            (collapse_top, collapse_bottom),
            font,
            diagnostics,
        );
        children_flow.height
    };
    // Size containment removes descendant intrinsic block-size contributions.
    // Children still lay out and may overflow this content box.
    let height = explicit_height.map(f64::from).unwrap_or_else(|| {
        if layout_box.image.is_none() && layout_box.style.container_type == ContainerType::Size {
            0.0
        } else {
            natural_height.max(0.0)
        }
    });
    let height = finite_used(height, layout_box.node_id, "height", diagnostics);
    let bounds = super::sizing::bounds(
        layout_box,
        false,
        containing_height,
        vertical,
        0.0,
        diagnostics,
    );
    let height = height.clamp(bounds.0, bounds.1);
    if !matches!(mode, BlockMode::Positioned { .. })
        && containing_height.is_none()
        && layout_box
            .style
            .height
            .as_ref()
            .is_some_and(|length| length.has_percentage())
    {
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
    BlockFlow {
        before: MarginStrut::new(margin.top).merge(children_flow.before),
        after: MarginStrut::new(margin.bottom).merge(children_flow.after),
        through: collapse_children
            && collapse_top
            && border.bottom == 0.0
            && padding.bottom == 0.0
            && explicit_height.is_none_or(|height| height == 0.0)
            && children_flow.all_through,
    }
}

pub(super) fn layout_positioned(
    layout_box: &mut LayoutBox,
    reference_width: f32,
    width: f32,
    height: Option<f32>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    layout_sized(
        layout_box,
        reference_width,
        width,
        height,
        (true, true),
        font,
        diagnostics,
    );
}

pub(super) fn layout_sized(
    layout_box: &mut LayoutBox,
    reference_width: f32,
    width: f32,
    height: Option<f32>,
    definite: (bool, bool),
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    layout_block(
        layout_box,
        0.0,
        reference_width,
        layout_box.style.direction,
        BlockMode::Positioned {
            width,
            height,
            definite,
        },
        font,
        diagnostics,
    );
    if layout_box.style.display == Display::ListItem {
        let text = layout_box
            .marker_text
            .clone()
            .unwrap_or_else(|| "\u{2022}".into());
        assign_marker(layout_box, text, font, diagnostics);
    }
}

fn assign_marker(
    child: &mut LayoutBox,
    text: String,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let style = child.marker_style.as_ref().unwrap_or(&child.style).clone();
    let metrics = font_metrics(font, &style, child.node_id, diagnostics);
    let glyphs = font.shape(&style, &text);
    let baseline = child
        .first_local_baseline()
        .map_or(child.content_rect.y + metrics.above, |baseline| {
            child.rect.y + baseline
        });
    child.list_marker = Some(ListMarker {
        x: child.content_rect.x - glyphs.width - style.font_size * 0.5,
        y: baseline - metrics.ascent,
        baseline,
        text,
        glyphs,
        style,
    });
}

/// CSS2 10.3.3 after resolving lengths and box-sizing, or 10.3.4 after
/// determining a supported replaced image's width. Physical bounds are applied
/// by the caller before resolving this equation again.
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
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let result = flow_children(layout_box, (x, y, width), (false, false), font, diagnostics);
    finite_used(
        result.height.max(0.0),
        layout_box.node_id,
        "block flow height",
        diagnostics,
    )
}

fn flow_children(
    layout_box: &mut LayoutBox,
    (x, y, width): (f32, f32, f32),
    (collapse_top, collapse_bottom): (bool, bool),
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> ChildrenFlow {
    let mut cursor_y = y as f64;
    let mut pending = MarginStrut::default();
    let mut before = MarginStrut::default();
    let mut at_start = true;
    let mut all_through = true;
    let ordered = layout_box.tag_name == "<ol>";
    let is_document = layout_box.tag_name == "#document";
    let mut list_index = 0;
    let mut index = 0;
    while index < layout_box.children.len() {
        if layout_box.children[index].kind != BoxKind::Block
            || layout_box.children[index].style.position.out_of_flow()
        {
            let end = layout_box.children[index..]
                .iter()
                .position(|child| {
                    child.kind == BoxKind::Block && !child.style.position.out_of_flow()
                })
                .map_or(layout_box.children.len(), |offset| index + offset);
            let inline_y = finite_used(
                cursor_y + pending.value(),
                layout_box.node_id,
                "inline run y",
                diagnostics,
            );
            let (lines, height) = layout_inline_run(
                &mut layout_box.children[index..end],
                &layout_box.style,
                layout_box.node_id,
                (x, inline_y, width, layout_box.definite_height),
                font,
                diagnostics,
            );
            if !lines.is_empty() {
                cursor_y += pending.value() + height;
                pending = MarginStrut::default();
                at_start = false;
                all_through = false;
                layout_box.line_boxes.extend(lines);
            }
            index = end;
            continue;
        }
        let child = &mut layout_box.children[index];
        if child.kind == BoxKind::Block {
            let child_flow = layout_block(
                child,
                x,
                width,
                layout_box.style.direction,
                if is_document && !child.is_anonymous() {
                    BlockMode::Root(layout_box.definite_height)
                } else {
                    BlockMode::Flow(layout_box.definite_height)
                },
                font,
                diagnostics,
            );
            pending = pending.merge(child_flow.before);
            let child_y = if at_start && collapse_top {
                y as f64
            } else {
                cursor_y + pending.value()
            };
            place_block(child, child_y, diagnostics);
            if child_flow.through {
                pending = pending.merge(child_flow.after);
            } else {
                if at_start && collapse_top {
                    before = pending;
                }
                cursor_y = child_y + child.rect.height as f64;
                pending = child_flow.after;
                at_start = false;
                all_through = false;
            }
            if child.style.display == Display::ListItem {
                list_index += 1;
                let text = child.marker_text.clone().unwrap_or_else(|| {
                    if ordered {
                        format!("{list_index}.")
                    } else {
                        "\u{2022}".into()
                    }
                });
                assign_marker(child, text, font, diagnostics);
            }
            // Root margins never adjoin another box's margins.
            if is_document && !child.is_anonymous() {
                cursor_y += pending.value();
                pending = MarginStrut::default();
            }
        }
        index += 1;
    }
    let after = if at_start && collapse_top {
        // The entire empty chain adjoins the parent's top margin, even if a
        // bottom decoration or definite height prevents its own through-collapse.
        before = pending;
        MarginStrut::default()
    } else if collapse_bottom {
        pending
    } else {
        cursor_y += pending.value();
        MarginStrut::default()
    };
    ChildrenFlow {
        height: cursor_y - y as f64,
        before,
        after,
        all_through,
    }
}

pub(super) fn place_block(layout_box: &mut LayoutBox, y: f64, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(marker) = &mut layout_box.list_marker {
        let dy = y as f32 - layout_box.rect.y;
        marker.y += dy;
        marker.baseline += dy;
    }
    layout_box.rect.y = finite_used(y, layout_box.node_id, "border y", diagnostics);
    layout_box.content_rect.y = finite_used(
        y + layout_box.content_rect.y as f64,
        layout_box.node_id,
        "content y",
        diagnostics,
    );
}
