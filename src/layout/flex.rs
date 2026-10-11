//! CSS Flexbox 9: intrinsic bases, hypothetical line collection, iterative
//! freezing, cross sizing and alignment. Items are formatted by block.rs in
//! local coordinates; only their final placement is installed in this pass.
use crate::{
    css::values::{Edges, Length},
    style::{AlignKeyword as A, Alignment, BoxSizing, Diagnostic, Direction, FlexBasis, FlexWrap},
};

use super::{
    LayoutBox, Rect, block, positioning, sizing,
    tree::{finite_used, resolve_edges, resolve_length, resolve_margins},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StaticPosition {
    pub area: Rect,
    pub row: bool,
    pub main_reverse: bool,
    pub cross_reverse: bool,
    pub main_logical_reverse: bool,
    pub cross_logical_reverse: bool,
    pub justify: Alignment,
    pub align: Alignment,
}

impl StaticPosition {
    pub(super) fn offset(self, width: f32, height: f32) -> (f32, f32) {
        let (main, cross, main_size, cross_size) = if self.row {
            (self.area.width, self.area.height, width, height)
        } else {
            (self.area.height, self.area.width, height, width)
        };
        let main_offset =
            distribution(self.justify, main - main_size, 1, self.main_logical_reverse).0;
        let cross_offset = distribution(
            self.align,
            cross - cross_size,
            1,
            self.cross_logical_reverse,
        )
        .0;
        let main_offset = if self.main_reverse {
            main - main_size - main_offset
        } else {
            main_offset
        };
        let cross_offset = if self.cross_reverse {
            cross - cross_size - cross_offset
        } else {
            cross_offset
        };
        if self.row {
            (main_offset, cross_offset)
        } else {
            (cross_offset, main_offset)
        }
    }
}

struct Item {
    index: usize,
    base: f64,
    hypothetical: f64,
    min: f64,
    max: f64,
    target: f64,
    grow: f64,
    shrink: f64,
    main_decor: f64,
    cross_decor: f32,
    main_before: Option<f32>,
    main_after: Option<f32>,
    cross_before: Option<f32>,
    cross_after: Option<f32>,
    width: f32,
    height: Option<f32>,
    height_definite: bool,
    width_definite: bool,
    cross_min: f32,
    cross_max: f32,
    cross_auto: bool,
    align: Alignment,
    frozen: bool,
    baseline: f32,
    cross_size: f32,
    measured_diagnostics: Vec<Diagnostic>,
}

impl Item {
    fn outer(&self, content: f64) -> f64 {
        content
            + self.main_decor
            + self.main_before.unwrap_or(0.0) as f64
            + self.main_after.unwrap_or(0.0) as f64
    }
    fn outer_cross(&self) -> f64 {
        self.cross_size as f64
            + self.cross_before.unwrap_or(0.0) as f64
            + self.cross_after.unwrap_or(0.0) as f64
    }
}

#[derive(Default)]
struct Line {
    items: Vec<usize>,
    cross: f32,
    baseline: f32,
}

fn report(b: &LayoutBox, message: &str, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        node_id: b.node_id,
        message: message.into(),
    });
}

fn gap(
    b: &LayoutBox,
    length: Option<&Length>,
    reference: Option<f32>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let Some(length) = length else { return 0.0 };
    if length.has_percentage() && reference.is_none() {
        report(
            b,
            "flex gap: cyclic percentage in an indefinite axis contributes 0px",
            diagnostics,
        );
        return 0.0;
    }
    resolve_length(
        length,
        b.style.font_size,
        reference.unwrap_or(0.0),
        b.node_id,
        "gap",
        diagnostics,
    )
    .max(0.0)
}

fn effective_align(child: &LayoutBox, parent: Alignment) -> Alignment {
    let mut align = child.style.align_self;
    if align.keyword == A::Auto {
        align = parent;
    }
    if align.keyword == A::Normal {
        align.keyword = A::Stretch;
    }
    align
}

/// Positions are measured from the flex axis start. Logical start/end are
/// mapped independently of flex reversal (the physical axis start is known).
fn distribution(align: Alignment, remaining: f32, count: usize, reverse: bool) -> (f32, f32) {
    if count == 0 {
        return (0.0, 0.0);
    }
    let mut keyword = align.keyword;
    if align.safe && remaining < 0.0 {
        keyword = A::Start;
    }
    match keyword {
        A::Start | A::SelfStart if reverse => (remaining, 0.0),
        A::End | A::SelfEnd if !reverse => (remaining, 0.0),
        A::FlexEnd => (remaining, 0.0),
        A::Center => (remaining / 2.0, 0.0),
        A::SpaceBetween if count > 1 && remaining >= 0.0 => (0.0, remaining / (count - 1) as f32),
        A::SpaceAround if remaining >= 0.0 => {
            let space = remaining / count as f32;
            (space / 2.0, space)
        }
        A::SpaceEvenly if remaining >= 0.0 => {
            let space = remaining / (count + 1) as f32;
            (space, space)
        }
        // Distribution fallback is safe center (around/evenly), or start
        // (between). Negative free space must not create negative gutters.
        A::SpaceAround | A::SpaceEvenly => (if reverse { remaining } else { 0.0 }, 0.0),
        _ => (0.0, 0.0),
    }
}

/// CSS Flexbox 9.7. Each iteration freezes at least one additional item; the
/// scaled shrink factor is distinct from the unscaled factor-sum adjustment.
fn resolve_flexible(items: &mut [Item], line: &Line, available: f64) {
    let grow = line
        .items
        .iter()
        .map(|&i| items[i].outer(items[i].hypothetical))
        .sum::<f64>()
        < available;
    for &i in &line.items {
        let item = &mut items[i];
        let factor = if grow { item.grow } else { item.shrink };
        item.frozen = factor == 0.0
            || grow && item.base > item.hypothetical
            || !grow && item.base < item.hypothetical;
        item.target = if item.frozen {
            item.hypothetical
        } else {
            item.base
        };
    }
    let free = |items: &[Item]| {
        available
            - line
                .items
                .iter()
                .map(|&i| {
                    let item = &items[i];
                    item.outer(if item.frozen { item.target } else { item.base })
                })
                .sum::<f64>()
    };
    let initial = free(items);
    loop {
        let unfrozen: Vec<_> = line
            .items
            .iter()
            .copied()
            .filter(|&i| !items[i].frozen)
            .collect();
        if unfrozen.is_empty() {
            break;
        }
        let sum = unfrozen
            .iter()
            .map(|&i| if grow { items[i].grow } else { items[i].shrink })
            .sum::<f64>();
        let mut remaining = free(items);
        if sum < 1.0 && (initial * sum).abs() < remaining.abs() {
            remaining = initial * sum;
        }
        let scaled = unfrozen
            .iter()
            .map(|&i| {
                if grow {
                    items[i].grow
                } else {
                    items[i].shrink * items[i].base.max(0.0)
                }
            })
            .sum::<f64>();
        let mut violations = Vec::new();
        let mut total = 0.0;
        for &i in &unfrozen {
            let item = &mut items[i];
            let factor = if grow {
                item.grow
            } else {
                item.shrink * item.base.max(0.0)
            };
            let raw = item.base
                + if scaled > 0.0 {
                    remaining * factor / scaled
                } else {
                    0.0
                };
            item.target = raw.clamp(item.min, item.max).max(0.0);
            let violation = item.target - raw;
            total += violation;
            violations.push((i, violation));
        }
        let mut froze = false;
        for (i, violation) in &violations {
            if total.abs() < 1e-7
                || total > 0.0 && *violation > 0.0
                || total < 0.0 && *violation < 0.0
            {
                items[*i].frozen = true;
                froze = true;
            }
        }
        if !froze {
            // Rounding cancellation cannot leave an unbounded retry.
            for (i, _) in violations {
                items[i].frozen = true;
            }
        }
    }
}

fn format_item(
    item: &mut Item,
    child: &mut LayoutBox,
    row: bool,
    reference_width: f32,
    stretch: Option<f32>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target = finite_used(item.target, child.node_id, "flex target", diagnostics).max(0.0);
    let (width, height) = if row {
        (target, stretch.or(item.height))
    } else {
        (stretch.unwrap_or(item.width), Some(target))
    };
    let definite_height = if item.height_definite { height } else { None };
    if child.content_rect.width == width
        && height.is_none_or(|h| h == child.content_rect.height)
        && child.definite_height == definite_height
        && child.definite_width == item.width_definite
        && (!row || (item.cross_min..=item.cross_max).contains(&child.content_rect.height))
    {
        item.cross_size = if row {
            child.rect.height
        } else {
            child.rect.width
        };
        item.baseline = child.first_local_baseline().unwrap_or(child.rect.height);
        diagnostics.extend(item.measured_diagnostics.iter().cloned());
        return;
    }
    child.reset();
    let mut measured_diagnostics = Vec::new();
    block::layout_sized(
        child,
        reference_width,
        width,
        height,
        (item.width_definite, item.height_definite),
        font,
        &mut measured_diagnostics,
    );
    if row {
        let constrained = child
            .content_rect
            .height
            .clamp(item.cross_min, item.cross_max);
        if constrained != child.content_rect.height {
            child.reset();
            measured_diagnostics.clear();
            block::layout_sized(
                child,
                reference_width,
                width,
                Some(constrained),
                (item.width_definite, item.height_definite),
                font,
                &mut measured_diagnostics,
            );
        }
    }
    item.measured_diagnostics = measured_diagnostics;
    diagnostics.extend(item.measured_diagnostics.iter().cloned());
    item.cross_size = if row {
        child.rect.height
    } else {
        child.rect.width
    };
    item.baseline = child.first_local_baseline().unwrap_or(child.rect.height);
}

pub(super) fn layout_flex(
    b: &mut LayoutBox,
    (x, y, width): (f32, f32, f32),
    height: Option<f32>,
    definite_height: Option<f32>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let row = b.style.flex_direction.row();
    if b.style.align_content.keyword == A::Baseline {
        report(
            b,
            "align-content baseline uses flex-start fallback in the finite flex line alignment model",
            diagnostics,
        );
    }
    if !row
        && (b.style.align_items.keyword == A::Baseline
            || b.children
                .iter()
                .any(|c| c.is_flex_item && c.style.align_self.keyword == A::Baseline))
    {
        report(
            b,
            "column flex baseline alignment uses flex-start fallback in the horizontal baseline model",
            diagnostics,
        );
    }
    let main_logical_reverse = b.style.flex_direction.reverse();
    let cross_logical_reverse = b.style.flex_wrap == FlexWrap::WrapReverse;
    let main_reverse =
        b.style.flex_direction.reverse() ^ (row && b.style.direction == Direction::Rtl);
    let cross_reverse = (b.style.flex_wrap == FlexWrap::WrapReverse)
        ^ (!row && b.style.direction == Direction::Rtl);
    let definite_width = b.definite_width.then_some(width);
    let main_reference = if row { definite_width } else { definite_height };
    let cross_reference = if row { definite_height } else { definite_width };
    let main_gap = gap(
        b,
        if row {
            b.style.column_gap.as_ref()
        } else {
            b.style.row_gap.as_ref()
        },
        main_reference,
        diagnostics,
    );
    let cross_gap = gap(
        b,
        if row {
            b.style.row_gap.as_ref()
        } else {
            b.style.column_gap.as_ref()
        },
        cross_reference,
        diagnostics,
    );
    let mut order: Vec<_> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_flex_item)
        .map(|(i, c)| (c.style.order, i))
        .collect();
    order.sort_by_key(|&(order, i)| (order, i));
    let mut items = Vec::new();
    for (_, index) in order {
        let child = &mut b.children[index];
        let padding = resolve_edges(
            &child.style.padding,
            child.style.font_size,
            width,
            child.node_id,
            "padding",
            diagnostics,
        )
        .map(|v| v.max(0.0));
        let border = child.style.border_width;
        let horizontal = finite_used(
            padding.left as f64 + padding.right as f64 + border.left as f64 + border.right as f64,
            child.node_id,
            "flex horizontal decorations",
            diagnostics,
        );
        let vertical = finite_used(
            padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
            child.node_id,
            "flex vertical decorations",
            diagnostics,
        );
        let margins = resolve_margins(&child.style, width, child.node_id, diagnostics);
        let mut explicit_width = sizing::definite_size(
            child,
            child.style.width.as_ref(),
            definite_width,
            horizontal,
            diagnostics,
        );
        let mut explicit_height = sizing::definite_size(
            child,
            child.style.height.as_ref(),
            definite_height,
            vertical,
            diagnostics,
        );
        let align = effective_align(child, b.style.align_items);
        let (cross_before, cross_after) = if row {
            (margins.top, margins.bottom)
        } else {
            (margins.left, margins.right)
        };
        let (cross_before, cross_after) = if cross_reverse {
            (cross_after, cross_before)
        } else {
            (cross_before, cross_after)
        };
        let (cross_min, cross_max) = sizing::bounds(
            child,
            !row,
            cross_reference,
            if row { vertical } else { horizontal },
            0.0,
            diagnostics,
        );
        if row {
            explicit_height = explicit_height.map(|h| h.clamp(cross_min, cross_max));
        } else {
            explicit_width = explicit_width.map(|w| w.clamp(cross_min, cross_max));
        }
        let (min_content, max_content) =
            positioning::intrinsic_content(child, width, font, diagnostics);
        let cross_auto = if row {
            explicit_height.is_none()
        } else {
            explicit_width.is_none()
        };
        let can_stretch =
            align.keyword == A::Stretch && cross_before.is_some() && cross_after.is_some();
        let available_cross =
            (width - horizontal - margins.left.unwrap_or(0.0) - margins.right.unwrap_or(0.0))
                .max(0.0);
        let measured_width = explicit_width.unwrap_or_else(|| {
            if !row && can_stretch {
                available_cross
            } else {
                available_cross.max(min_content).min(max_content)
            }
        });
        let measured_width = if row {
            measured_width
        } else {
            measured_width.clamp(cross_min, cross_max)
        };
        child.reset();
        let mut measured_diagnostics = Vec::new();
        block::layout_positioned(
            child,
            width,
            measured_width,
            explicit_height,
            font,
            &mut measured_diagnostics,
        );
        let content_main = if row {
            max_content
        } else {
            child.content_rect.height
        };
        let main_decor = if row { horizontal } else { vertical };
        let specified_main = if row { explicit_width } else { explicit_height };
        let basis = match &child.style.flex_basis {
            FlexBasis::Auto => specified_main.unwrap_or(content_main),
            FlexBasis::Content => content_main,
            FlexBasis::Length(length) if length.has_percentage() && main_reference.is_none() => {
                report(
                    child,
                    "flex-basis: percentage has an indefinite main size; using content",
                    diagnostics,
                );
                content_main
            }
            FlexBasis::Length(length) => {
                let size = resolve_length(
                    length,
                    child.style.font_size,
                    main_reference.unwrap_or(0.0),
                    child.node_id,
                    "flex-basis",
                    diagnostics,
                )
                .max(0.0);
                // The base ignores the content-box zero floor and min/max.
                if child.style.box_sizing == BoxSizing::BorderBox {
                    size - main_decor
                } else {
                    size
                }
            }
        };
        let non_scrollable = if row {
            !child.style.overflow_x.scroll_container()
        } else {
            !child.style.overflow_y.scroll_container()
        };
        let mut automatic_minimum = if non_scrollable {
            if row {
                min_content
            } else {
                child.content_rect.height
            }
        } else {
            0.0
        };
        if let Some(specified) = specified_main {
            automatic_minimum = automatic_minimum.min(specified);
        }
        // Replaced intrinsic ratio supplies the transferred-size suggestion.
        if let Some(image) = &child.image {
            let metrics =
                super::inline::font_metrics(font, &child.style, child.node_id, diagnostics);
            let transferred = image.size(
                &child.style,
                if row { None } else { explicit_width },
                if row { explicit_height } else { None },
                metrics.above + metrics.below,
                font,
            );
            if (row && explicit_height.is_some()) || (!row && explicit_width.is_some()) {
                automatic_minimum =
                    automatic_minimum.min(if row { transferred.0 } else { transferred.1 });
            }
        }
        let (min, max) = sizing::bounds(
            child,
            row,
            main_reference,
            main_decor,
            automatic_minimum,
            diagnostics,
        );
        let max_length = if row {
            child.style.max_width.as_ref()
        } else {
            child.style.max_height.as_ref()
        };
        let capped_auto =
            sizing::definite_size(child, max_length, main_reference, main_decor, diagnostics);
        let min = if (if row {
            &child.style.min_width
        } else {
            &child.style.min_height
        })
        .is_none()
        {
            capped_auto.map_or(min, |max| min.min(max))
        } else {
            min
        };
        let max = capped_auto.unwrap_or(max).max(min);
        let hypothetical = (basis as f64).clamp(min as f64, max as f64);
        let (main_before, main_after) = if row {
            (margins.left, margins.right)
        } else {
            (margins.top, margins.bottom)
        };
        let (main_before, main_after) = if main_reverse {
            (main_after, main_before)
        } else {
            (main_before, main_after)
        };
        items.push(Item {
            index, base: basis as f64, hypothetical, min: min as f64, max: max as f64,
            target: hypothetical, grow: child.style.flex_grow as f64, shrink: child.style.flex_shrink as f64,
            main_decor: main_decor as f64, cross_decor: if row { vertical } else { horizontal },
            main_before, main_after,
            cross_before, cross_after, width: measured_width, height: explicit_height,
            height_definite: if row { explicit_height.is_some() || cross_reference.is_some() } else {
                main_reference.is_some() || specified_main.is_some()
                    || matches!(&child.style.flex_basis, FlexBasis::Length(l) if !l.has_percentage())
            },
            width_definite: if row {
                main_reference.is_some() || specified_main.is_some()
                    || matches!(&child.style.flex_basis, FlexBasis::Length(l) if !l.has_percentage())
            } else {explicit_width.is_some() || cross_reference.is_some()},
            cross_min, cross_max, cross_auto, align, frozen: false, baseline: 0.0, cross_size: 0.0, measured_diagnostics,
        });
    }
    let natural_main = items.iter().map(|i| i.outer(i.hypothetical)).sum::<f64>()
        + main_gap as f64 * items.len().saturating_sub(1) as f64;
    let height_bounds = sizing::bounds(
        b,
        false,
        height,
        b.padding.top + b.padding.bottom + b.style.border_width.top + b.style.border_width.bottom,
        0.0,
        diagnostics,
    );
    let main = if row {
        width
    } else {
        height
            .unwrap_or_else(|| {
                finite_used(
                    natural_main,
                    b.node_id,
                    "flex natural main size",
                    diagnostics,
                )
            })
            .clamp(height_bounds.0, height_bounds.1)
    };
    let mut lines: Vec<Line> = Vec::new();
    let mut occupied = 0.0;
    for (i, item) in items.iter().enumerate() {
        let outer = item.outer(item.hypothetical);
        let last_nonempty = lines.last().is_some_and(|line| !line.items.is_empty());
        if lines.is_empty()
            || b.style.flex_wrap != FlexWrap::Nowrap
                && last_nonempty
                && (row || height.is_some())
                && occupied + main_gap as f64 + outer > main as f64 + 1e-6
        {
            lines.push(Line::default());
            occupied = 0.0;
        }
        if !lines.last().unwrap().items.is_empty() {
            occupied += main_gap as f64;
        }
        occupied += outer;
        lines.last_mut().unwrap().items.push(i);
    }
    for line in &mut lines {
        if row || main_reference.is_some() || main != natural_main as f32 {
            resolve_flexible(
                &mut items,
                line,
                main as f64 - main_gap as f64 * line.items.len().saturating_sub(1) as f64,
            );
        }
        let mut descent: f32 = 0.0;
        for &i in &line.items {
            let item = &mut items[i];
            format_item(
                item,
                &mut b.children[item.index],
                row,
                width,
                None,
                font,
                &mut Vec::new(),
            );
            line.cross = line.cross.max(finite_used(
                item.outer_cross(),
                b.node_id,
                "flex outer cross size",
                diagnostics,
            ));
            if row
                && item.align.keyword == A::Baseline
                && item.cross_before.is_some()
                && item.cross_after.is_some()
            {
                let baseline = if cross_reverse {
                    item.cross_size - item.baseline
                } else {
                    item.baseline
                };
                line.baseline = line
                    .baseline
                    .max(baseline + item.cross_before.unwrap_or(0.0));
                descent = descent.max(item.cross_size - baseline + item.cross_after.unwrap_or(0.0));
            }
        }
        line.cross = line.cross.max(finite_used(
            line.baseline as f64 + descent as f64,
            b.node_id,
            "flex baseline cross size",
            diagnostics,
        ));
    }
    let natural_cross = finite_used(
        lines.iter().map(|l| l.cross as f64).sum::<f64>()
            + cross_gap as f64 * lines.len().saturating_sub(1) as f64,
        b.node_id,
        "flex natural cross size",
        diagnostics,
    );
    let cross = if row {
        height
            .unwrap_or(natural_cross)
            .clamp(height_bounds.0, height_bounds.1)
    } else {
        width
    };
    if b.style.flex_wrap == FlexWrap::Nowrap {
        if let Some(line) = lines.first_mut() {
            line.cross = cross;
        }
    } else if matches!(b.style.align_content.keyword, A::Normal | A::Stretch)
        && cross > natural_cross
        && !lines.is_empty()
    {
        let extra = (cross - natural_cross) / lines.len() as f32;
        for line in &mut lines {
            line.cross = finite_used(
                line.cross as f64 + extra as f64,
                b.node_id,
                "flex stretched line size",
                diagnostics,
            );
        }
    }
    let occupied_cross = finite_used(
        lines.iter().map(|l| l.cross as f64).sum::<f64>()
            + cross_gap as f64 * lines.len().saturating_sub(1) as f64,
        b.node_id,
        "flex occupied cross size",
        diagnostics,
    );
    let (mut line_cursor, line_space) = if b.style.flex_wrap == FlexWrap::Nowrap {
        (0.0, 0.0)
    } else {
        distribution(
            b.style.align_content,
            cross - occupied_cross,
            lines.len(),
            cross_logical_reverse,
        )
    };
    for line in &lines {
        let occupied_main = line
            .items
            .iter()
            .map(|&i| items[i].outer(items[i].target))
            .sum::<f64>()
            + main_gap as f64 * line.items.len().saturating_sub(1) as f64;
        let remaining = finite_used(
            main as f64 - occupied_main,
            b.node_id,
            "flex free space",
            diagnostics,
        );
        let auto_count = line
            .items
            .iter()
            .map(|&i| {
                usize::from(items[i].main_before.is_none())
                    + usize::from(items[i].main_after.is_none())
            })
            .sum::<usize>();
        let auto = if remaining > 0.0 && auto_count > 0 {
            remaining / auto_count as f32
        } else {
            0.0
        };
        let justify = if auto > 0.0 {
            Alignment::new(A::FlexStart)
        } else {
            b.style.justify_content
        };
        let (mut cursor, between) = distribution(
            justify,
            if auto > 0.0 { 0.0 } else { remaining },
            line.items.len(),
            main_logical_reverse,
        );
        for &i in &line.items {
            let item = &mut items[i];
            let child = &mut b.children[item.index];
            let stretch = if item.cross_auto
                && item.align.keyword == A::Stretch
                && item.cross_before.is_some()
                && item.cross_after.is_some()
            {
                Some(
                    (line.cross
                        - item.cross_before.unwrap_or(0.0)
                        - item.cross_after.unwrap_or(0.0)
                        - item.cross_decor)
                        .max(0.0)
                        .clamp(item.cross_min, item.cross_max),
                )
            } else {
                None
            };
            format_item(item, child, row, width, stretch, font, diagnostics);
            let main_before = item.main_before.unwrap_or(auto);
            let main_after = item.main_after.unwrap_or(auto);
            let (mut before, mut after) = (
                item.cross_before.unwrap_or(0.0),
                item.cross_after.unwrap_or(0.0),
            );
            let cross_remaining = finite_used(
                line.cross as f64 - item.cross_size as f64 - before as f64 - after as f64,
                child.node_id,
                "flex cross free space",
                diagnostics,
            );
            let mut cross_offset;
            if item.cross_before.is_none() || item.cross_after.is_none() {
                if cross_remaining >= 0.0 {
                    let count = usize::from(item.cross_before.is_none())
                        + usize::from(item.cross_after.is_none());
                    if item.cross_before.is_none() {
                        before = cross_remaining / count as f32;
                    }
                    if item.cross_after.is_none() {
                        after = cross_remaining / count as f32;
                    }
                } else if item.cross_before.is_none() {
                    before = 0.0;
                    after += cross_remaining;
                } else {
                    after += cross_remaining;
                }
                cross_offset = before;
            } else if row && item.align.keyword == A::Baseline {
                let baseline = if cross_reverse {
                    item.cross_size - item.baseline
                } else {
                    item.baseline
                };
                cross_offset = line.baseline - baseline;
            } else {
                let mut align = item.align;
                if matches!(align.keyword, A::SelfStart | A::SelfEnd)
                    && !row
                    && child.style.direction != b.style.direction
                {
                    align.keyword = if align.keyword == A::SelfStart {
                        A::SelfEnd
                    } else {
                        A::SelfStart
                    };
                }
                cross_offset =
                    before + distribution(align, cross_remaining, 1, cross_logical_reverse).0;
            }
            cursor += main_before;
            let border_main = item.target as f32 + item.main_decor as f32;
            let main_position = if main_reverse {
                main - cursor - border_main
            } else {
                cursor
            };
            cross_offset += line_cursor;
            let cross_position = if cross_reverse {
                cross - cross_offset - item.cross_size
            } else {
                cross_offset
            };
            let (item_x, item_y) = if row {
                (
                    x as f64 + main_position as f64,
                    y as f64 + cross_position as f64,
                )
            } else {
                (
                    x as f64 + cross_position as f64,
                    y as f64 + main_position as f64,
                )
            };
            let item_x = finite_used(item_x, child.node_id, "flex item x", diagnostics);
            let item_y = finite_used(item_y, child.node_id, "flex item y", diagnostics);
            positioning::translate(child, item_x - child.rect.x, 0.0, diagnostics);
            block::place_block(child, item_y as f64, diagnostics);
            child.margin = if row {
                Edges {
                    left: if main_reverse {
                        main_after
                    } else {
                        main_before
                    },
                    right: if main_reverse {
                        main_before
                    } else {
                        main_after
                    },
                    top: if cross_reverse { after } else { before },
                    bottom: if cross_reverse { before } else { after },
                }
            } else {
                Edges {
                    top: if main_reverse {
                        main_after
                    } else {
                        main_before
                    },
                    bottom: if main_reverse {
                        main_before
                    } else {
                        main_after
                    },
                    left: if cross_reverse { after } else { before },
                    right: if cross_reverse { before } else { after },
                }
            };
            cursor += border_main + main_after + main_gap + between;
        }
        line_cursor += line.cross + cross_gap + line_space;
    }
    let area = Rect {
        x,
        y,
        width,
        height: if row { cross } else { main },
    };
    for child in &mut b.children {
        if child.style.position.out_of_flow() {
            let mut align = effective_align(child, b.style.align_items);
            if matches!(align.keyword, A::Stretch | A::Baseline) {
                align.keyword = A::FlexStart;
            }
            child.rect.x = area.x;
            child.rect.y = area.y;
            child.flex_static = Some(StaticPosition {
                area,
                row,
                main_reverse,
                cross_reverse,
                main_logical_reverse,
                cross_logical_reverse,
                justify: b.style.justify_content,
                align,
            });
        }
    }
    area.height
}

pub(super) fn intrinsic_widths(
    b: &LayoutBox,
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let row = b.style.flex_direction.row();
    struct Intrinsic {
        min: f64,
        max: f64,
        base: f64,
        extra: f64,
        grow: f64,
        shrink: f64,
        lower: f64,
        upper: f64,
    }
    let mut widths = Vec::new();
    for child in b.children.iter().filter(|c| c.is_flex_item) {
        let padding = resolve_edges(
            &child.style.padding,
            child.style.font_size,
            reference,
            child.node_id,
            "intrinsic flex padding",
            diagnostics,
        );
        let decor = finite_used(
            padding.left as f64
                + padding.right as f64
                + child.style.border_width.left as f64
                + child.style.border_width.right as f64,
            child.node_id,
            "intrinsic flex decorations",
            diagnostics,
        );
        let margins = resolve_margins(&child.style, reference, child.node_id, diagnostics);
        let extra = finite_used(
            decor as f64 + margins.left.unwrap_or(0.0) as f64 + margins.right.unwrap_or(0.0) as f64,
            child.node_id,
            "intrinsic flex outer edges",
            diagnostics,
        );
        let (mut min, mut max) =
            positioning::intrinsic_content(child, reference, font, diagnostics);
        let mut preferred = None;
        if let Some(width) =
            sizing::definite_size(child, child.style.width.as_ref(), None, decor, diagnostics)
        {
            preferred = Some(width);
            min = width;
            max = width;
        }
        let base = if !row {
            preferred.unwrap_or(max)
        } else {
            match &child.style.flex_basis {
                FlexBasis::Length(length) if !length.has_percentage() => {
                    let value = resolve_length(
                        length,
                        child.style.font_size,
                        0.0,
                        child.node_id,
                        "intrinsic flex basis",
                        diagnostics,
                    )
                    .max(0.0);
                    if child.style.box_sizing == BoxSizing::BorderBox {
                        value - decor
                    } else {
                        value
                    }
                }
                FlexBasis::Auto => preferred.unwrap_or(max),
                _ => max,
            }
        };
        let automatic_min = if !row || child.style.overflow_x.scroll_container() {
            0.0
        } else {
            min
        };
        let limits = sizing::bounds(child, true, None, decor, automatic_min, diagnostics);
        widths.push(Intrinsic {
            min: min as f64,
            max: max as f64,
            base: base as f64,
            extra: extra as f64,
            grow: if row {
                child.style.flex_grow as f64
            } else {
                1.0
            },
            shrink: if row {
                child.style.flex_shrink as f64
            } else {
                1.0
            },
            lower: limits.0 as f64,
            upper: limits.1 as f64,
        });
    }
    let gap = gap(b, b.style.column_gap.as_ref(), None, diagnostics);
    let contribution = |item: &Intrinsic, min: bool| {
        let mut value = if min { item.min } else { item.max };
        if item.grow == 0.0 {
            value = value.min(item.base);
        }
        if item.shrink == 0.0 {
            value = value.max(item.base);
        }
        value.clamp(item.lower, item.upper) + item.extra
    };
    let largest = |min| {
        widths
            .iter()
            .map(|item| contribution(item, min))
            .fold(0.0, f64::max)
    };
    // CSS 9.9.1.1's explicitly published ideal intrinsic algorithm. Its
    // fractional-factor behavior is not a claim of browser/WPT compatibility.
    let intrinsic_line = |min| {
        let mut fraction = widths
            .iter()
            .map(|item| {
                let difference = (if min { item.min } else { item.max }) - item.base;
                if difference > 0.0 {
                    if item.grow >= 1.0 {
                        difference / item.grow
                    } else {
                        difference * item.grow
                    }
                } else if difference < 0.0 {
                    let scaled = item.shrink * item.base.max(0.0);
                    if scaled > 0.0 {
                        difference / scaled
                    } else {
                        f64::NEG_INFINITY
                    }
                } else {
                    0.0
                }
            })
            .fold(f64::NEG_INFINITY, f64::max);
        if !fraction.is_finite() {
            fraction = 0.0;
        }
        let grow = widths.iter().map(|i| i.grow).sum::<f64>();
        let shrink = widths.iter().map(|i| i.shrink).sum::<f64>();
        if fraction > 0.0 && grow > 0.0 && grow < 1.0 {
            fraction /= grow;
        }
        if fraction < 0.0 && shrink < 1.0 {
            fraction *= shrink;
        }
        widths
            .iter()
            .map(|i| {
                let factor = if fraction >= 0.0 {
                    i.grow
                } else {
                    i.shrink * i.base.max(0.0)
                };
                (i.base + factor * fraction).clamp(i.lower, i.upper) + i.extra
            })
            .sum::<f64>()
            + gap as f64 * widths.len().saturating_sub(1) as f64
    };
    let (min, max) = if row {
        (
            if b.style.flex_wrap == FlexWrap::Nowrap {
                intrinsic_line(true)
            } else {
                largest(true)
            },
            intrinsic_line(false),
        )
    } else {
        (largest(true), largest(false))
    };
    (
        finite_used(min, b.node_id, "intrinsic flex min-content", diagnostics),
        finite_used(
            max.max(min),
            b.node_id,
            "intrinsic flex max-content",
            diagnostics,
        ),
    )
}
