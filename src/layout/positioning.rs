use std::collections::HashMap;

use crate::{
    css::values::{Edges, Length, LengthOrAuto},
    style::{Diagnostic, Direction, Position},
};

use super::{
    BoxKind, CoordinateSpace, LayoutBox, Rect, block,
    inline::intrinsic_inline,
    tree::{absolutize_y, finite_used, resolve_edges, resolve_length, resolve_margins},
};

#[derive(Clone, Copy)]
struct ContainingBlock {
    rect: Rect,
    space: CoordinateSpace,
    direction: Direction,
}

/// Translate fragment origins, never the indexed glyphs' baseline-relative
/// positions. This is also used for relative offsets and positioned subtrees.
fn translate_own(b: &mut LayoutBox, dx: f32, dy: f32, diagnostics: &mut Vec<Diagnostic>) {
    let id = b.node_id;
    fn point(
        value: &mut f32,
        delta: f32,
        id: Option<crate::dom::NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        *value = finite_used(
            *value as f64 + delta as f64,
            id,
            "translated coordinate",
            diagnostics,
        );
    }
    fn rect(
        r: &mut Rect,
        dx: f32,
        dy: f32,
        id: Option<crate::dom::NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        point(&mut r.x, dx, id, diagnostics);
        point(&mut r.y, dy, id, diagnostics);
    }
    rect(&mut b.rect, dx, dy, id, diagnostics);
    rect(&mut b.content_rect, dx, dy, id, diagnostics);
    if let Some(table) = &mut b.table_rect {
        rect(table, dx, dy, id, diagnostics);
    }
    for border in &mut b.table_borders {
        rect(&mut border.rect, dx, dy, id, diagnostics);
    }
    if let Some(anchor) = &mut b.flex_static {
        rect(&mut anchor.area, dx, dy, id, diagnostics);
    }
    if let Some((_, area)) = &mut b.grid_area {
        rect(area, dx, dy, id, diagnostics);
    }
    for f in &mut b.inline_fragments {
        rect(&mut f.rect, dx, dy, id, diagnostics);
        rect(&mut f.content_rect, dx, dy, id, diagnostics);
        point(
            &mut f.baseline,
            if b.style.writing_mode.vertical() {
                dx
            } else {
                dy
            },
            id,
            diagnostics,
        );
    }
    for line in &mut b.line_boxes {
        rect(&mut line.rect, dx, dy, id, diagnostics);
        rect(&mut line.content_rect, dx, dy, id, diagnostics);
        point(
            &mut line.baseline,
            if b.style.writing_mode.vertical() {
                dx
            } else {
                dy
            },
            id,
            diagnostics,
        );
    }
    for f in &mut b.text_fragments {
        rect(&mut f.ink_rect, dx, dy, id, diagnostics);
        point(&mut f.x, dx, id, diagnostics);
        point(&mut f.y, dy, id, diagnostics);
        point(&mut f.baseline, dy, id, diagnostics);
    }
    if let Some(marker) = &mut b.list_marker {
        point(&mut marker.x, dx, id, diagnostics);
        point(&mut marker.y, dy, id, diagnostics);
        point(&mut marker.baseline, dy, id, diagnostics);
    }
}

pub(super) fn translate(b: &mut LayoutBox, dx: f32, dy: f32, diagnostics: &mut Vec<Diagnostic>) {
    translate_own(b, dx, dy, diagnostics);
    for child in &mut b.children {
        translate(child, dx, dy, diagnostics);
    }
}

/// A pre-finalization block is a local Y root; its descendants must not receive
/// the same displacement again when the tree is absolutized.
pub(super) fn translate_local_y(b: &mut LayoutBox, dy: f32, diagnostics: &mut Vec<Diagnostic>) {
    if b.local_root() {
        super::block::place_block(b, (b.rect.y + dy) as f64, diagnostics);
    } else {
        translate_own(b, 0.0, dy, diagnostics);
        for child in &mut b.children {
            translate_local_y(child, dy, diagnostics);
        }
    }
}

fn space(b: &mut LayoutBox, coordinate_space: CoordinateSpace) {
    b.coordinate_space = coordinate_space;
    for child in &mut b.children {
        space(child, coordinate_space);
    }
}

fn inset(
    b: &LayoutBox,
    length: &LengthOrAuto,
    reference: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    match length {
        LengthOrAuto::Auto => None,
        LengthOrAuto::Length(length) => Some(resolve_length(
            length,
            b.style.font_size,
            reference,
            b.node_id,
            "inset",
            diagnostics,
        )),
    }
}

fn relative_offset(
    b: &LayoutBox,
    height: Option<f32>,
    direction: Direction,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    if b.style.position == Position::Relative {
        let left = inset(b, &b.style.inset.left, b.flow_width, diagnostics);
        let right = inset(b, &b.style.inset.right, b.flow_width, diagnostics);
        let vertical = |length: &LengthOrAuto, diagnostics: &mut Vec<Diagnostic>| {
            if matches!(length, LengthOrAuto::Length(l) if l.has_percentage()) && height.is_none() {
                None
            } else {
                inset(b, length, height.unwrap_or(0.0), diagnostics)
            }
        };
        let top = vertical(&b.style.inset.top, diagnostics);
        let bottom = vertical(&b.style.inset.bottom, diagnostics);
        let dx = if direction == Direction::Rtl {
            right.map(|r| -r).or(left)
        } else {
            left.or_else(|| right.map(|r| -r))
        }
        .unwrap_or(0.0);
        let dy = top.or_else(|| bottom.map(|b| -b)).unwrap_or(0.0);
        return (dx, dy);
    }
    (0.0, 0.0)
}

/// Semantic ancestry survives lifted block-in-inline descendants. Move each
/// visual fragment once; anonymous line allocations themselves stay in flow.
fn relative(b: &mut LayoutBox, height: Option<f32>, diagnostics: &mut Vec<Diagnostic>) {
    type Offsets = HashMap<usize, (Option<usize>, (f32, f32))>;
    fn collect(
        b: &LayoutBox,
        height: Option<f32>,
        direction: Direction,
        root: bool,
        map: &mut Offsets,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if b.style.position.out_of_flow() && !root {
            return;
        }
        map.entry(b.box_id).or_insert_with(|| {
            (
                if root { None } else { b.parent_box },
                relative_offset(b, height, direction, diagnostics),
            )
        });
        let height = if b.local_root() {
            b.definite_height
        } else {
            height
        };
        let direction = if b.local_root() {
            b.style.direction
        } else {
            direction
        };
        for child in &b.children {
            collect(child, height, direction, false, map, diagnostics);
        }
    }
    fn apply(b: &mut LayoutBox, root: bool, map: &Offsets, diagnostics: &mut Vec<Diagnostic>) {
        if b.style.position.out_of_flow() && !root {
            // Static-position placeholders inherit their normal ancestors'
            // visual offsets, although they have no allocated fragment yet.
            let mut parent = b.parent_box;
            let mut offset = (0.0f64, 0.0f64);
            while let Some((next, delta)) = parent.and_then(|id| map.get(&id)) {
                offset.0 += delta.0 as f64;
                offset.1 += delta.1 as f64;
                parent = *next;
            }
            let dx = finite_used(offset.0, b.node_id, "relative x", diagnostics);
            let dy = finite_used(offset.1, b.node_id, "relative y", diagnostics);
            translate_own(b, dx, dy, diagnostics);
            return;
        }
        let mut parent = Some(b.box_id);
        let mut offset = (0.0f64, 0.0f64);
        while let Some((next, delta)) = parent.and_then(|id| map.get(&id)) {
            offset.0 += delta.0 as f64;
            offset.1 += delta.1 as f64;
            parent = *next;
        }
        let dx = finite_used(offset.0, b.node_id, "relative x", diagnostics);
        let dy = finite_used(offset.1, b.node_id, "relative y", diagnostics);
        translate_own(b, dx, dy, diagnostics);
        for child in &mut b.children {
            apply(child, false, map, diagnostics);
        }
    }
    let mut offsets = HashMap::new();
    collect(
        b,
        height,
        b.style.direction,
        true,
        &mut offsets,
        diagnostics,
    );
    apply(b, true, &offsets, diagnostics);
}

fn collect_containing(
    b: &LayoutBox,
    map: &mut HashMap<usize, ContainingBlock>,
    include_self: bool,
) {
    if b.style.position.out_of_flow() && !include_self {
        return;
    }
    if b.style.position != Position::Static {
        let border = b.style.border_width;
        let padding_rect = |rect: Rect| Rect {
            x: rect.x + border.left,
            y: rect.y + border.top,
            width: (rect.width - border.left - border.right).max(0.0),
            height: (rect.height - border.top - border.bottom).max(0.0),
        };
        let rect = if b.kind == BoxKind::Inline && b.image.is_none() && !b.atomic_inline() {
            let first = padding_rect(b.inline_fragments.first().map_or(b.rect, |f| f.rect));
            let last = padding_rect(b.inline_fragments.last().map_or(b.rect, |f| f.rect));
            let start = map.get(&b.box_id).map_or(first, |cb| cb.rect);
            Rect {
                x: start.x,
                y: start.y,
                width: (last.x + last.width - start.x).max(0.0),
                height: (last.y + last.height - start.y).max(0.0),
            }
        } else {
            padding_rect(b.rect)
        };
        map.insert(
            b.box_id,
            ContainingBlock {
                rect,
                space: b.coordinate_space,
                direction: b.style.direction,
            },
        );
    }
    for child in &b.children {
        collect_containing(child, map, false);
    }
}

fn positioned_ids(b: &LayoutBox, ids: &mut Vec<usize>) {
    if b.style.position.out_of_flow() {
        ids.push(b.box_id);
    }
    for child in &b.children {
        positioned_ids(child, ids);
    }
}

fn find_mut(b: &mut LayoutBox, id: usize) -> Option<&mut LayoutBox> {
    if b.box_id == id {
        return Some(b);
    }
    b.children.iter_mut().find_map(|child| find_mut(child, id))
}

pub(super) fn position_tree(
    root: &mut LayoutBox,
    viewport: Rect,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    relative(root, Some(viewport.height), diagnostics);
    let direction = super::tree::root_element(root).map_or(Direction::Ltr, |b| b.style.direction);
    let initial = ContainingBlock {
        rect: viewport,
        space: CoordinateSpace::Document,
        direction,
    };
    let mut blocks = HashMap::new();
    collect_containing(root, &mut blocks, true);
    let mut ids = Vec::new();
    positioned_ids(root, &mut ids);
    // Parents are constructed before descendants, including generated boxes.
    // Anonymous wrappers and split inline continuations do not alter this order.
    ids.sort_unstable();
    for id in ids {
        let b = find_mut(root, id).expect("positioned construction identity");
        let mut cb = if b.style.position == Position::Fixed {
            ContainingBlock {
                rect: viewport,
                space: CoordinateSpace::Viewport,
                direction,
            }
        } else {
            b.position_parent
                .and_then(|id| blocks.get(&id).copied())
                .unwrap_or(initial)
        };
        if b.style.position != Position::Fixed
            && let Some((owner, area)) = b.grid_area
            && b.position_parent == Some(owner)
        {
            cb.rect = area;
        }
        position_box(b, cb, font, diagnostics);
        collect_containing(b, &mut blocks, true);
    }
}

#[derive(Clone, Copy)]
struct Axis {
    containing: f64,
    start: Option<f64>,
    end: Option<f64>,
    size: Option<f64>,
    before: Option<f64>,
    after: Option<f64>,
    decorations: f64,
    static_start: f64,
    reverse: bool,
    vertical: bool,
}

impl Axis {
    fn stretched(self) -> Option<f64> {
        if self.size.is_none() {
            Some(
                (self.containing
                    - self.start?
                    - self.end?
                    - self.decorations
                    - self.before.unwrap_or(0.0)
                    - self.after.unwrap_or(0.0))
                .max(0.0),
            )
        } else {
            self.size
        }
    }

    fn available(self) -> f64 {
        let start = if self.start.is_none() && self.end.is_none() {
            self.static_start
        } else {
            self.start.unwrap_or(0.0)
        };
        (self.containing
            - start
            - self.end.unwrap_or(0.0)
            - self.decorations
            - self.before.unwrap_or(0.0)
            - self.after.unwrap_or(0.0))
        .max(0.0)
    }

    /// CSS2 10.3.7 / 10.6.4: auto margins are zero whenever an inset or
    /// dimension is auto. Only the all-definite equation solves auto margins.
    fn solve(self, size: f64) -> (f64, f64, f64) {
        let mut before = self.before.unwrap_or(0.0);
        let mut after = self.after.unwrap_or(0.0);
        let mut start = self.start;
        let end = self.end;
        if start.is_none() && end.is_none() {
            start = Some(self.static_start);
        }
        if let (Some(start), Some(end)) = (start, end) {
            let remaining =
                self.containing - start - end - self.decorations - size - before - after;
            match (
                self.before.is_none() && self.size.is_some(),
                self.after.is_none() && self.size.is_some(),
            ) {
                (true, true) if remaining < 0.0 && !self.vertical && self.reverse => {
                    before = remaining
                }
                (true, true) if remaining < 0.0 && !self.vertical => after = remaining,
                (true, true) => {
                    before = remaining / 2.0;
                    after = remaining / 2.0;
                }
                (true, false) => before = remaining,
                (false, true) => after = remaining,
                _ => {}
            }
            if self.reverse
                && (self.before.is_some() && self.after.is_some() || self.size.is_none())
            {
                return (
                    self.containing - end - after - self.decorations - size,
                    before,
                    after,
                );
            }
        }
        let start = start.unwrap_or_else(|| {
            self.containing - end.unwrap_or(0.0) - before - after - self.decorations - size
        });
        (start + before, before, after)
    }
}

fn position_box(
    b: &mut LayoutBox,
    cb: ContainingBlock,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let padding = resolve_edges(
        &b.style.padding,
        b.style.font_size,
        cb.rect.width,
        b.node_id,
        "padding",
        diagnostics,
    )
    .map(|v| v.max(0.0));
    let border = b.style.border_width;
    let horizontal = finite_used(
        padding.left as f64 + padding.right as f64 + border.left as f64 + border.right as f64,
        b.node_id,
        "positioned horizontal decorations",
        diagnostics,
    );
    let vertical = finite_used(
        padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
        b.node_id,
        "positioned vertical decorations",
        diagnostics,
    );
    let margins = resolve_margins(&b.style, cb.rect.width, b.node_id, diagnostics);
    let size =
        |length: &Option<Length>, reference, decorations, diagnostics: &mut Vec<Diagnostic>| {
            length.as_ref().map(|length| {
                b.style.box_sizing.content_size(
                    resolve_length(
                        length,
                        b.style.font_size,
                        reference,
                        b.node_id,
                        "positioned size",
                        diagnostics,
                    ),
                    decorations,
                )
            })
        };
    let width = size(&b.style.width, cb.rect.width, horizontal, diagnostics);
    let height = size(&b.style.height, cb.rect.height, vertical, diagnostics);
    let mut x = Axis {
        containing: cb.rect.width as f64,
        start: inset(b, &b.style.inset.left, cb.rect.width, diagnostics).map(f64::from),
        end: inset(b, &b.style.inset.right, cb.rect.width, diagnostics).map(f64::from),
        size: width.map(f64::from),
        before: margins.left.map(f64::from),
        after: margins.right.map(f64::from),
        decorations: horizontal as f64,
        static_start: b.rect.x as f64 - cb.rect.x as f64,
        reverse: cb.direction == Direction::Rtl,
        vertical: false,
    };
    let mut y = Axis {
        containing: cb.rect.height as f64,
        start: inset(b, &b.style.inset.top, cb.rect.height, diagnostics).map(f64::from),
        end: inset(b, &b.style.inset.bottom, cb.rect.height, diagnostics).map(f64::from),
        size: height.map(f64::from),
        before: margins.top.map(f64::from),
        after: margins.bottom.map(f64::from),
        decorations: vertical as f64,
        static_start: b.rect.y as f64 - cb.rect.y as f64,
        reverse: false,
        vertical: true,
    };
    let intrinsic = if let Some(image) = &b.image {
        let metrics = super::inline::font_metrics(font, &b.style, b.node_id, diagnostics);
        let natural = image.size(&b.style, width, height, metrics.above + metrics.below, font);
        x.size = Some(natural.0 as f64);
        y.size = Some(natural.1 as f64);
        (natural.0, natural.0)
    } else {
        intrinsic_content(b, cb.rect.width, font, diagnostics)
    };
    let width = x.stretched().unwrap_or_else(|| {
        (intrinsic.0 as f64)
            .max(x.available())
            .min(intrinsic.1 as f64)
    });
    let width = finite_used(width, b.node_id, "positioned width equation", diagnostics);
    let bounds = super::sizing::bounds(b, true, Some(cb.rect.width), horizontal, 0.0, diagnostics);
    let width = width.clamp(bounds.0, bounds.1);
    let height = y
        .stretched()
        .map(|height| finite_used(height, b.node_id, "positioned height equation", diagnostics));
    let anchor = b.flex_static;
    b.reset();
    block::layout_sized(
        b,
        cb.rect.width,
        width,
        height,
        (
            x.size.is_some() || x.start.is_some() && x.end.is_some(),
            true,
        ),
        font,
        diagnostics,
    );
    absolutize_y(b, 0.0, diagnostics);
    if let Some(anchor) = anchor {
        let (sx, sy) = anchor.offset(
            b.rect.width + b.margin.left + b.margin.right,
            b.rect.height + b.margin.top + b.margin.bottom,
        );
        if x.start.is_none() && x.end.is_none() {
            x.static_start = (anchor.area.x + sx - cb.rect.x) as f64;
        }
        if y.start.is_none() && y.end.is_none() {
            y.static_start = (anchor.area.y + sy - cb.rect.y) as f64;
        }
    }
    let (x, left, right) = x.solve(b.content_rect.width as f64);
    let (y, top, bottom) = y.solve(b.content_rect.height as f64);
    let dx = finite_used(
        cb.rect.x as f64 + x - b.rect.x as f64,
        b.node_id,
        "positioned x",
        diagnostics,
    );
    let dy = finite_used(
        cb.rect.y as f64 + y - b.rect.y as f64,
        b.node_id,
        "positioned y",
        diagnostics,
    );
    b.margin = Edges {
        left: finite_used(left, b.node_id, "positioned margin-left", diagnostics),
        right: finite_used(right, b.node_id, "positioned margin-right", diagnostics),
        top: finite_used(top, b.node_id, "positioned margin-top", diagnostics),
        bottom: finite_used(bottom, b.node_id, "positioned margin-bottom", diagnostics),
    };
    translate(b, dx, dy, diagnostics);
    space(b, cb.space);
    // The positioned box has already consumed its own insets; only normal
    // descendants need the independent relative visual-offset pass.
    relative(b, b.definite_height, diagnostics);
}

pub(super) fn intrinsic_content(
    b: &LayoutBox,
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    if let Some(m) = b.axis_measurement {
        return (m.x, m.y);
    }
    if b.style.container_type != crate::css::container::ContainerType::Normal {
        return (0.0, 0.0);
    }
    if b.style.display.is_flex() {
        return super::flex::intrinsic_widths(b, reference, font, diagnostics);
    }
    if b.style.display.is_grid() {
        return super::grid::intrinsic_widths(b, reference, font, diagnostics);
    }
    if b.style.display.is_table() {
        return super::table::intrinsic_widths(b, reference, font, diagnostics);
    }
    if let Some(image) = &b.image {
        let metrics = super::inline::font_metrics(font, &b.style, b.node_id, diagnostics);
        let padding = resolve_edges(
            &b.style.padding,
            b.style.font_size,
            reference,
            b.node_id,
            "intrinsic image padding",
            diagnostics,
        );
        let vertical =
            padding.top + padding.bottom + b.style.border_width.top + b.style.border_width.bottom;
        let height = b
            .style
            .height
            .as_ref()
            .filter(|length| !length.has_percentage())
            .map(|length| {
                b.style.box_sizing.content_size(
                    resolve_length(
                        length,
                        b.style.font_size,
                        0.0,
                        b.node_id,
                        "intrinsic image height",
                        diagnostics,
                    ),
                    vertical,
                )
            });
        let height_limits = super::sizing::bounds(b, false, None, vertical, 0.0, diagnostics);
        let height = height.map(|h| h.clamp(height_limits.0, height_limits.1));
        let width = image
            .size(&b.style, None, height, metrics.above + metrics.below, font)
            .0;
        return (width, width);
    }
    let mut result: (f32, f32) = (0.0, 0.0);
    let mut index = 0;
    while index < b.children.len() {
        let child = &b.children[index];
        if child.style.position.out_of_flow() {
            index += 1;
            continue;
        }
        let widths = if child.kind == BoxKind::Block {
            index += 1;
            let padding = resolve_edges(
                &child.style.padding,
                child.style.font_size,
                reference,
                child.node_id,
                "intrinsic padding",
                diagnostics,
            );
            let margins = resolve_margins(&child.style, reference, child.node_id, diagnostics);
            let decorations = padding.left
                + padding.right
                + child.style.border_width.left
                + child.style.border_width.right;
            let margin = margins.left.unwrap_or(0.0) + margins.right.unwrap_or(0.0);
            if let Some(width) = &child.style.width
                && !width.has_percentage()
            {
                let width = child.style.box_sizing.content_size(
                    resolve_length(
                        width,
                        child.style.font_size,
                        reference,
                        child.node_id,
                        "intrinsic width",
                        diagnostics,
                    ),
                    decorations,
                ) + decorations
                    + margin;
                (width, width)
            } else {
                let (min, max) = intrinsic_content(child, reference, font, diagnostics);
                (min + decorations + margin, max + decorations + margin)
            }
        } else {
            let end = b.children[index..]
                .iter()
                .position(|child| {
                    child.kind == BoxKind::Block && !child.style.position.out_of_flow()
                })
                .map_or(b.children.len(), |n| index + n);
            let widths = intrinsic_inline(
                &b.children[index..end],
                &b.style,
                reference,
                font,
                diagnostics,
            );
            index = end;
            widths
        };
        result.0 = result.0.max(widths.0);
        result.1 = result.1.max(widths.1);
    }
    result
}
