//! Fragment complete native lines and formatting units, not raster strips.
//! Geometry is shared with ordinary stacking, effects, overflow and hit testing.
use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    style::{Diagnostic, Direction, fragmentation::Break},
    text::FontContext,
};

use super::{LayoutBox, LayoutTree, LineBox, Rect};

pub const MAX_FRAGMENTAINERS: usize = 1024;
static NEXT_FRAGMENT_BOX: AtomicUsize = AtomicUsize::new(1_000_000_000);

#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    /// Stable construction identity, distinct from this fragment's paint ID.
    pub source_box: usize,
    pub fragmentainer: usize,
    pub rect: Rect,
    /// Translation from the source layout. Indexed glyph positions are shared.
    pub offset: (f32, f32),
    pub first: bool,
    pub last: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Boundary {
    pub at: f32,
    pub forced: Break,
}
#[derive(Clone, Debug)]
struct Avoid {
    start: f32,
    end: f32,
    atomic: bool,
}
#[derive(Clone, Debug)]
struct Paragraph {
    starts: Vec<f32>,
    orphans: usize,
    widows: usize,
}
#[derive(Clone, Debug)]
struct Decoration {
    start: f32,
    end: f32,
    top: f32,
    bottom: f32,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Opportunities {
    points: Vec<Boundary>,
    avoid: Vec<Avoid>,
    paragraphs: Vec<Paragraph>,
    decorations: Vec<Decoration>,
    starts: Vec<f32>,
    pub names: Vec<(f32, Option<String>)>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Range {
    pub start: f32,
    pub end: f32,
    pub next: f32,
    pub forced: Break,
}

fn bottom(r: Rect) -> f32 {
    r.y + r.height
}
fn atomic(b: &LayoutBox) -> bool {
    b.image.is_some()
        || b.atomic_inline()
        || b.style.display.is_item_container()
        || b.style.display.is_table()
        || b.style.fragmentation.columns()
        || b.style.overflow_x.scroll_container()
        || b.style.overflow_y.scroll_container()
}

impl Opportunities {
    pub(crate) fn collect(root: &LayoutBox, page: bool) -> Self {
        fn visit(
            b: &LayoutBox,
            page: bool,
            parent_name: Option<String>,
            result: &mut Opportunities,
        ) {
            if b.style.position.out_of_flow() {
                return;
            }
            let start = b.rect.y;
            let end = bottom(b.rect);
            let name = b.style.fragmentation.page().or(parent_name);
            if b.normal_block() && end > start {
                result.starts.push(start);
                result.points.push(Boundary {
                    at: start,
                    forced: b.style.fragmentation.before(),
                });
                result.points.push(Boundary {
                    at: end,
                    forced: b.style.fragmentation.after(),
                });
                if page && !b.is_anonymous() {
                    result.names.push((start, name.clone()));
                }
                if b.style.fragmentation.inside().avoids(page) {
                    result.avoid.push(Avoid {
                        start,
                        end,
                        atomic: false,
                    });
                }
                if b.style.fragmentation.clone_decoration() {
                    let (top, bottom) = if b.style.writing_mode.vertical() {
                        if b.style.writing_mode.block_reverse() {
                            (
                                b.padding.right + b.style.border_width.right,
                                b.padding.left + b.style.border_width.left,
                            )
                        } else {
                            (
                                b.padding.left + b.style.border_width.left,
                                b.padding.right + b.style.border_width.right,
                            )
                        }
                    } else {
                        (
                            b.padding.top + b.style.border_width.top,
                            b.padding.bottom + b.style.border_width.bottom,
                        )
                    };
                    result.decorations.push(Decoration {
                        start,
                        end,
                        top,
                        bottom,
                    });
                }
            }
            if atomic(b) {
                result.avoid.push(Avoid {
                    start,
                    end,
                    atomic: true,
                });
                return;
            }
            let lines = &b.line_boxes;
            if !lines.is_empty() {
                result.paragraphs.push(Paragraph {
                    starts: lines.iter().map(|l| l.rect.y).collect(),
                    orphans: b.style.fragmentation.orphans(),
                    widows: b.style.fragmentation.widows(),
                });
            }
            for line in lines {
                result.starts.push(line.rect.y);
                let at = bottom(line.rect);
                result.points.push(Boundary {
                    at,
                    forced: Break::Auto,
                });
            }
            for child in &b.children {
                visit(child, page, name.clone(), result);
            }
        }
        let mut result = Self::default();
        visit(root, page, None, &mut result);
        result.points.sort_by(|a, b| a.at.total_cmp(&b.at));
        result.starts.sort_by(f32::total_cmp);
        result.names.sort_by(|a, b| a.0.total_cmp(&b.0));
        result
    }

    pub(crate) fn next(&self, start: f32, height: f32, extent: f32, page: bool) -> Range {
        let clone_top = self
            .decorations
            .iter()
            .filter(|d| d.start < start - 0.001 && d.end > start)
            .map(|d| d.top)
            .sum::<f32>();
        let clone_bottom = self
            .decorations
            .iter()
            .filter(|d| d.start < start + height - clone_top && d.end > start + height - clone_top)
            .map(|d| d.bottom)
            .sum::<f32>();
        let capacity = (height - clone_top - clone_bottom).max(0.01);
        let limit = (start + capacity).min(extent);
        let forced = self
            .points
            .iter()
            .find(|p| p.at > start + 0.01 && p.forced.forced(page));
        let hard_valid = |at: f32| {
            !self.avoid.iter().any(|a| {
                a.start < at - 0.001
                    && at < a.end - 0.001
                    && (a.atomic || a.end - a.start <= height)
            })
        };
        let valid = |at: f32| {
            hard_valid(at)
                && self.paragraphs.iter().all(|p| {
                    let end = p.starts.partition_point(|y| *y < at - 0.001);
                    let first = p.starts.partition_point(|y| *y < start - 0.001);
                    end == 0
                        || end == p.starts.len()
                        || end <= first
                        || (end - first >= p.orphans && p.starts.len() - end >= p.widows)
                })
        };
        let mut selected = if forced.is_some_and(|p| p.at <= limit + 0.001) {
            *forced.unwrap()
        } else if limit >= extent - 0.001 {
            Boundary {
                at: extent,
                forced: Break::Auto,
            }
        } else {
            self.points
                .iter()
                .rev()
                .find(|p| p.at > start + 0.01 && p.at <= limit + 0.001 && valid(p.at))
                .copied()
                .or_else(|| {
                    self.points
                        .iter()
                        .rev()
                        .find(|p| p.at > start + 0.01 && p.at <= limit + 0.001 && hard_valid(p.at))
                        .copied()
                })
                .or_else(|| {
                    self.points
                        .iter()
                        .find(|p| p.at > start + 0.01 && valid(p.at))
                        .copied()
                })
                // Unavoidable widow/orphan constraints yield before indivisible
                // units do. This guarantees forward progress without clipping.
                .or_else(|| {
                    self.points
                        .iter()
                        .find(|p| p.at > start + 0.01 && hard_valid(p.at))
                        .copied()
                })
                .unwrap_or(Boundary {
                    at: extent,
                    forced: Break::Auto,
                })
        };
        if let Some(forced) = forced
            && forced.at < selected.at
        {
            selected = *forced;
        }
        let end = selected.at.max(start + 0.01).min(extent);
        let next = self
            .starts
            .iter()
            .copied()
            .find(|&p| p >= end - 0.001)
            .unwrap_or(end)
            .max(end)
            .min(extent);
        Range {
            start,
            end,
            next,
            forced: selected.forced,
        }
    }

    fn plan(&self, height: f32, extent: f32, page: bool) -> Vec<Range> {
        let mut ranges = Vec::new();
        let mut start = 0.0;
        while start < extent - 0.001 && ranges.len() < MAX_FRAGMENTAINERS {
            let range = self.next(start, height, extent, page);
            start = range.next;
            ranges.push(range);
        }
        ranges
    }

    fn balanced_height(&self, extent: f32, count: usize, cap: Option<f32>) -> f32 {
        let (mut lo, mut hi) = (0.0, extent.max(0.01));
        // The feasibility oracle uses actual legal line/block boundaries. No
        // words are forced into columns, and no shaped advances are scaled.
        for _ in 0..28 {
            let mid = (lo + hi) * 0.5;
            let plan = self.plan(mid, extent, false);
            if plan.len() <= count && plan.iter().all(|r| self.used_height(*r) <= mid + 0.001) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        cap.map_or(hi, |cap| hi.min(cap)).max(0.01)
    }
    fn used_height(&self, r: Range) -> f32 {
        r.end - r.start
            + self
                .decorations
                .iter()
                .filter(|d| d.start < r.end && d.end > r.start)
                .map(|d| {
                    (if d.start < r.start - 0.001 {
                        d.top
                    } else {
                        0.0
                    }) + (if d.end > r.end + 0.001 { d.bottom } else { 0.0 })
                })
                .sum::<f32>()
    }
}

fn remap_ids(b: &mut LayoutBox, map: &mut HashMap<usize, usize>) {
    let old = b.box_id;
    let id = *map
        .entry(old)
        .or_insert_with(|| NEXT_FRAGMENT_BOX.fetch_add(1, Ordering::Relaxed));
    b.box_id = id;
    for child in &mut b.children {
        remap_ids(child, map);
    }
}
fn remap_parents(b: &mut LayoutBox, map: &HashMap<usize, usize>) {
    b.parent_box = b.parent_box.map(|id| map.get(&id).copied().unwrap_or(id));
    b.position_parent = b
        .position_parent
        .map(|id| map.get(&id).copied().unwrap_or(id));
    for child in &mut b.children {
        remap_parents(child, map);
    }
}
fn intersects(r: Rect, range: Range) -> bool {
    r.y < range.end - 0.001 && bottom(r) > range.start + 0.001
}
#[derive(Clone, Copy, Default)]
enum Axis {
    #[default]
    Horizontal,
    Vertical {
        origin: f32,
        inline: f32,
        extent: f32,
        reverse: bool,
    },
}
impl Axis {
    fn measure(self, r: Rect) -> Rect {
        match self {
            Self::Horizontal => r,
            Self::Vertical {
                origin,
                extent,
                reverse,
                ..
            } => Rect {
                x: r.y,
                y: if reverse {
                    origin + extent - r.x - r.width
                } else {
                    r.x - origin
                },
                width: r.height,
                height: r.width,
            },
        }
    }
    fn crop(self, r: Rect, range: Range) -> Rect {
        let m = self.measure(r);
        let start = m.y.max(range.start);
        let end = bottom(m).min(range.end).max(start);
        match self {
            Self::Horizontal => Rect {
                y: start,
                height: end - start,
                ..r
            },
            Self::Vertical {
                origin,
                extent,
                reverse,
                ..
            } => Rect {
                x: if reverse {
                    origin + extent - end
                } else {
                    origin + start
                },
                width: end - start,
                ..r
            },
        }
    }
    fn offset(self, port: Rect, range: Range, fixed: bool) -> (f32, f32) {
        match self {
            Self::Horizontal => (port.x, port.y - if fixed { 0.0 } else { range.start }),
            Self::Vertical {
                origin,
                inline,
                extent,
                reverse,
            } => (
                if reverse {
                    port.x + port.width - origin - extent + range.start
                } else {
                    port.x - origin - range.start
                },
                port.y - inline,
            ),
        }
    }
}

fn piece(
    source: &LayoutBox,
    range: Range,
    port: Rect,
    index: usize,
    lines: &[LineBox],
    repeat_fixed: bool,
) -> Option<LayoutBox> {
    piece_axis(
        source,
        range,
        port,
        index,
        lines,
        repeat_fixed,
        Axis::Horizontal,
    )
}
#[allow(clippy::too_many_arguments)]
fn piece_axis(
    source: &LayoutBox,
    range: Range,
    port: Rect,
    index: usize,
    lines: &[LineBox],
    repeat_fixed: bool,
    axis: Axis,
) -> Option<LayoutBox> {
    let fixed = source.coordinate_space == super::CoordinateSpace::Viewport;
    if fixed && !repeat_fixed {
        return None;
    }
    let own_lines = if source.line_boxes.is_empty() {
        lines
    } else {
        &source.line_boxes
    };
    let first = axis.measure(source.rect).y >= range.start - 0.001;
    let last = bottom(axis.measure(source.rect)) <= range.end + 0.001;
    let clone = source.style.fragmentation.clone_decoration() && source.normal_block();
    let clone_top = if clone && !first {
        match axis {
            Axis::Horizontal => source.padding.top + source.style.border_width.top,
            Axis::Vertical { reverse: true, .. } => {
                source.padding.right + source.style.border_width.right
            }
            Axis::Vertical { reverse: false, .. } => {
                source.padding.left + source.style.border_width.left
            }
        }
    } else {
        0.0
    };
    let clone_bottom = if clone && !last {
        match axis {
            Axis::Horizontal => source.padding.bottom + source.style.border_width.bottom,
            Axis::Vertical { reverse: true, .. } => {
                source.padding.left + source.style.border_width.left
            }
            Axis::Vertical { reverse: false, .. } => {
                source.padding.right + source.style.border_width.right
            }
        }
    } else {
        0.0
    };
    let mut child_port = port;
    match axis {
        Axis::Horizontal => child_port.y += clone_top,
        Axis::Vertical { reverse: true, .. } => child_port.x -= clone_top,
        Axis::Vertical { reverse: false, .. } => child_port.x += clone_top,
    }
    let owns_line = |i: usize, r: Rect| {
        own_lines.get(i).map_or_else(
            || axis.measure(r).y >= range.start - 0.001 && axis.measure(r).y < range.end - 0.001,
            |line| {
                axis.measure(line.rect).y >= range.start - 0.001
                    && axis.measure(line.rect).y < range.end - 0.001
            },
        )
    };
    if atomic(source) || source.style.position.out_of_flow() {
        if !fixed
            && !(axis.measure(source.rect).y >= range.start - 0.001
                && axis.measure(source.rect).y < range.end - 0.001)
        {
            return None;
        }
        let mut result = source.clone();
        result.fragment_source = None;
        let (dx, dy) = axis.offset(port, range, fixed);
        super::positioning::translate(&mut result, dx, dy, &mut Vec::new());
        annotate(&mut result, index, port, (dx, dy), true, true);
        return Some(result);
    }
    let mut result = source.clone();
    result.fragment_source = None;
    result.children = source
        .children
        .iter()
        .filter_map(|child| {
            piece_axis(
                child,
                range,
                child_port,
                index,
                own_lines,
                repeat_fixed,
                axis,
            )
        })
        .collect();
    result
        .text_fragments
        .retain(|f| owns_line(f.line_index, f.ink_rect));
    result
        .inline_fragments
        .retain(|f| owns_line(f.line_index, f.rect));
    result.line_boxes.retain(|line| {
        axis.measure(line.rect).y >= range.start - 0.001
            && axis.measure(line.rect).y < range.end - 0.001
    });
    if !intersects(axis.measure(source.rect), range)
        && result.children.is_empty()
        && result.text_fragments.is_empty()
        && result.inline_fragments.is_empty()
    {
        return None;
    }
    result.rect = axis.crop(source.rect, range);
    result.content_rect = axis.crop(source.content_rect, range);
    match axis {
        Axis::Horizontal => {
            result.rect.height += clone_top + clone_bottom;
            result.content_rect.y += clone_top;
            for line in &mut result.line_boxes {
                line.rect.y += clone_top;
                line.content_rect.y += clone_top;
                line.baseline += clone_top;
            }
        }
        Axis::Vertical { reverse, .. } => {
            result.rect.width += clone_top + clone_bottom;
            if reverse {
                result.rect.x -= clone_top + clone_bottom;
                result.content_rect.x -= clone_top;
            } else {
                result.content_rect.x += clone_top;
            }
        }
    }
    if !source.style.fragmentation.clone_decoration() {
        if !first {
            match axis {
                Axis::Horizontal => {
                    result.style.border_width.top = 0.0;
                    result.padding.top = 0.0;
                }
                Axis::Vertical { reverse: true, .. } => {
                    result.style.border_width.right = 0.0;
                    result.padding.right = 0.0;
                }
                Axis::Vertical { reverse: false, .. } => {
                    result.style.border_width.left = 0.0;
                    result.padding.left = 0.0;
                }
            }
            result.list_marker = None;
        }
        if !last {
            match axis {
                Axis::Horizontal => {
                    result.style.border_width.bottom = 0.0;
                    result.padding.bottom = 0.0;
                }
                Axis::Vertical { reverse: true, .. } => {
                    result.style.border_width.left = 0.0;
                    result.padding.left = 0.0;
                }
                Axis::Vertical { reverse: false, .. } => {
                    result.style.border_width.right = 0.0;
                    result.padding.right = 0.0;
                }
            }
        }
    }
    // Children have already received their translations. Only this fragment's
    // own geometry is translated; all descendants now share its local origin.
    let children = std::mem::take(&mut result.children);
    let offset = axis.offset(port, range, false);
    super::positioning::translate(&mut result, offset.0, offset.1, &mut Vec::new());
    result.children = children;
    result.fragment_geometry = true;
    result.fragment = Some(Fragment {
        source_box: source
            .fragment
            .as_ref()
            .map_or(source.box_id, |f| f.source_box),
        fragmentainer: index,
        rect: port,
        offset,
        first,
        last,
    });
    Some(result)
}
fn annotate(
    b: &mut LayoutBox,
    index: usize,
    port: Rect,
    offset: (f32, f32),
    first: bool,
    last: bool,
) {
    b.fragment_geometry = true;
    b.fragment = Some(Fragment {
        source_box: b.fragment.as_ref().map_or(b.box_id, |f| f.source_box),
        fragmentainer: index,
        rect: port,
        offset,
        first,
        last,
    });
    for child in &mut b.children {
        annotate(child, index, port, offset, first, last);
    }
}
pub(crate) fn fragment_tree(
    source: &LayoutTree,
    range: Range,
    port: Rect,
    sheet: Rect,
    index: usize,
    blank: bool,
) -> LayoutTree {
    let mut result = source.clone();
    result.root = if blank {
        piece(
            &source.root,
            Range {
                start: 0.0,
                end: 0.0,
                next: 0.0,
                forced: Break::Auto,
            },
            port,
            index,
            &[],
            true,
        )
        .unwrap_or_else(|| {
            let mut root = source.root.clone();
            root.children.clear();
            root
        })
    } else {
        piece(&source.root, range, port, index, &[], true).unwrap_or_else(|| {
            let mut root = source.root.clone();
            root.children.clear();
            root
        })
    };
    result.root.rect = sheet;
    result.root.content_rect = port;
    result.page_clip = Some(port);
    let mut map = HashMap::new();
    remap_ids(&mut result.root, &mut map);
    remap_parents(&mut result.root, &map);
    result.overflow.clear();
    result.scroll_state = super::ScrollState::default();
    result.finalize_overflow(sheet);
    result
}
pub(crate) fn adjust_fixed(tree: &mut LayoutTree, delta: f32) {
    fn visit(b: &mut LayoutBox, delta: f32) {
        if b.style.position == crate::style::Position::Fixed {
            if b.style.inset.top == crate::css::values::LengthOrAuto::Auto
                && b.style.inset.bottom != crate::css::values::LengthOrAuto::Auto
            {
                super::positioning::translate(b, 0.0, delta, &mut Vec::new());
            }
            return;
        }
        for child in &mut b.children {
            visit(child, delta);
        }
    }
    visit(&mut tree.root, delta);
}

pub(super) fn column_metrics(b: &LayoutBox, inline: f32) -> (usize, f32, f32) {
    let gap = b
        .style
        .column_gap
        .as_ref()
        .map_or(b.style.font_size, |v| v.resolve(b.style.font_size, inline))
        .max(0.0);
    let preferred = b
        .style
        .fragmentation
        .width()
        .map(|v| v.resolve(b.style.font_size, inline).max(0.01));
    let fitting = preferred.map(|w| ((inline + gap) / (w + gap)).floor().max(1.0) as usize);
    let count = match (b.style.fragmentation.count(), fitting) {
        (Some(n), Some(f)) => usize::from(n).min(f),
        (Some(n), None) => usize::from(n),
        (None, Some(f)) => f,
        _ => 1,
    }
    .clamp(1, 256);
    (
        count,
        ((inline - (count - 1) as f32 * gap) / count as f32).max(0.0),
        gap,
    )
}

pub(super) fn layout_columns(
    b: &mut LayoutBox,
    (x, y, width): (f32, f32, f32),
    height: Option<f32>,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let source = b.children.clone();
    let (count, column_width, gap) = column_metrics(b, width);
    let mut pending = Vec::new();
    let mut output = Vec::new();
    let mut top = 0.0;
    let mut column_index = 0;
    for child in source
        .iter()
        .cloned()
        .map(Some)
        .chain(std::iter::once(None))
    {
        if child
            .as_ref()
            .is_some_and(|c| !c.style.fragmentation.span() || c.style.position.out_of_flow())
        {
            pending.push(child.unwrap());
            continue;
        }
        if !pending.is_empty() {
            b.children = std::mem::take(&mut pending);
            let extent = super::block::layout_children(b, x, 0.0, column_width, font, diagnostics);
            for child in &mut b.children {
                super::tree::absolutize_y(child, 0.0, diagnostics);
            }
            let mut band = b.clone();
            band.rect = Rect {
                x,
                y: 0.0,
                width: column_width,
                height: extent,
            };
            // The multicol root itself is not an indivisible nested multicol.
            band.style.fragmentation.set(
                crate::style::fragmentation::Property::Count,
                crate::style::fragmentation::Value::Integer(None),
            );
            band.style.fragmentation.set(
                crate::style::fragmentation::Property::Width,
                crate::style::fragmentation::Value::Length(None),
            );
            let opportunities = Opportunities::collect(&band, false);
            b.line_boxes.clear();
            let remaining = height.map(|h| (h - top).max(0.01));
            let used_height = match remaining {
                Some(height) if !b.style.fragmentation.balance() && child.is_none() => height,
                _ => opportunities.balanced_height(extent, count, remaining),
            };
            let ranges = opportunities.plan(used_height, extent, false);
            if ranges.len() == MAX_FRAGMENTAINERS && ranges.last().is_some_and(|r| r.next < extent)
            {
                diagnostics.push(Diagnostic {
                    node_id: b.node_id,
                    message: "multicolumn layout exceeds 1024 fragmentainers".into(),
                });
            }
            let band_height = ranges
                .iter()
                .map(|r| opportunities.used_height(*r))
                .fold(0.0, f32::max);
            for (i, range) in ranges.into_iter().enumerate() {
                let inline = if b.style.direction == Direction::Rtl {
                    count as f32 - 1.0 - i as f32
                } else {
                    i as f32
                };
                let port = Rect {
                    x: inline * (column_width + gap),
                    y: y + top,
                    width: column_width,
                    height: used_height,
                };
                for source in &b.children {
                    if let Some(mut fragment) = piece(
                        source,
                        range,
                        port,
                        column_index + i,
                        &band.line_boxes,
                        false,
                    ) {
                        let mut map = HashMap::new();
                        remap_ids(&mut fragment, &mut map);
                        remap_parents(&mut fragment, &map);
                        output.push(fragment);
                    }
                }
                if i > 0 {
                    let (rule_width, style, color) = b.style.fragmentation.rule(b.style.text_color);
                    if style.visible() {
                        b.column_rules.push(super::table::BorderSegment {
                            rect: Rect {
                                x: x + inline * (column_width + gap)
                                    + if b.style.direction == Direction::Rtl {
                                        column_width + gap * 0.5 - rule_width * 0.5
                                    } else {
                                        -gap * 0.5 - rule_width * 0.5
                                    },
                                y: y + top,
                                width: rule_width,
                                height: used_height,
                            },
                            style,
                            color,
                        });
                    }
                }
            }
            column_index += opportunities.plan(used_height, extent, false).len();
            top += band_height;
        }
        if let Some(mut spanner) = child {
            super::block::layout_caption(&mut spanner, x, width, font, diagnostics);
            spanner.rect.y = y + top + spanner.margin.top;
            spanner.content_rect.y += spanner.rect.y;
            super::tree::absolutize_y(&mut spanner, 0.0, diagnostics);
            mark_absolute(&mut spanner);
            top += spanner.margin.top + spanner.rect.height + spanner.margin.bottom;
            output.push(spanner);
        }
    }
    b.children = output;
    b.fragment_source = Some(source);
    top
}
fn mark_absolute(b: &mut LayoutBox) {
    b.fragment_geometry = true;
    for child in &mut b.children {
        mark_absolute(child);
    }
}

/// Native vertical lines are measured in logical block coordinates only for
/// break selection. Paintable glyphs remain in their original writing axes.
#[allow(clippy::too_many_arguments)]
pub(super) fn layout_vertical_columns(
    b: &mut LayoutBox,
    x: f32,
    available_width: f32,
    containing_height: Option<f32>,
    forced_width: Option<f32>,
    forced_height: Option<f32>,
    percentage_reference: f32,
    font: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source_children = b.children.clone();
    let fragmentation = b.style.fragmentation.clone();
    b.style.fragmentation.set(
        crate::style::fragmentation::Property::Count,
        crate::style::fragmentation::Value::Integer(None),
    );
    b.style.fragmentation.set(
        crate::style::fragmentation::Property::Width,
        crate::style::fragmentation::Value::Length(None),
    );
    super::writing::layout(
        b,
        x,
        available_width,
        containing_height,
        forced_width,
        forced_height,
        percentage_reference,
        font,
        diagnostics,
    );
    let full = b.clone();
    b.style.fragmentation = fragmentation;
    let (count, column_inline, gap) = column_metrics(b, full.content_rect.height);
    let mut native = full.clone();
    native.reset();
    native.children = source_children.clone();
    native.style.width = None;
    native.style.min_width = None;
    native.style.max_width = None;
    let inline_decor = full.rect.height - full.content_rect.height;
    super::writing::layout(
        &mut native,
        0.0,
        available_width,
        Some(column_inline + inline_decor),
        None,
        Some(column_inline + inline_decor),
        percentage_reference,
        font,
        diagnostics,
    );
    super::tree::absolutize_y(&mut native, 0.0, diagnostics);
    let extent = native.content_rect.width;
    let axis = Axis::Vertical {
        origin: native.content_rect.x,
        inline: native.content_rect.y,
        extent,
        reverse: b.style.writing_mode.block_reverse(),
    };
    let mut oracle = native.clone();
    fn measure(b: &mut LayoutBox, axis: Axis) {
        b.rect = axis.measure(b.rect);
        b.content_rect = axis.measure(b.content_rect);
        for line in &mut b.line_boxes {
            line.rect = axis.measure(line.rect);
            line.content_rect = axis.measure(line.content_rect);
        }
        for child in &mut b.children {
            measure(child, axis);
        }
    }
    measure(&mut oracle, axis);
    let opportunities = Opportunities::collect(&oracle, false);
    let cap = forced_width.or_else(|| {
        b.style
            .width
            .as_ref()
            .map(|w| w.resolve(b.style.font_size, available_width))
    });
    let used = match cap {
        Some(width) if !b.style.fragmentation.balance() => width.max(0.01),
        _ => opportunities.balanced_height(extent, count, cap),
    };
    let ranges = opportunities.plan(used, extent, false);
    let mut children = Vec::new();
    let reverse_inline = (b.style.direction == Direction::Rtl)
        != (b.style.writing_mode == crate::style::WritingMode::SidewaysLr);
    for (i, range) in ranges.iter().copied().enumerate() {
        let inline = if reverse_inline {
            count as f32 - 1.0 - i as f32
        } else {
            i as f32
        };
        let port = Rect {
            x: full.content_rect.x,
            y: full.content_rect.y + inline * (column_inline + gap),
            width: used,
            height: column_inline,
        };
        for source in &native.children {
            if let Some(mut fragment) =
                piece_axis(source, range, port, i, &native.line_boxes, false, axis)
            {
                let mut map = HashMap::new();
                remap_ids(&mut fragment, &mut map);
                remap_parents(&mut fragment, &map);
                children.push(fragment);
            }
        }
        if i > 0 {
            let (width, style, color) = b.style.fragmentation.rule(b.style.text_color);
            if style.visible() {
                b.column_rules.push(super::table::BorderSegment {
                    rect: Rect {
                        x: port.x,
                        y: port.y
                            + if reverse_inline {
                                column_inline + gap * 0.5 - width * 0.5
                            } else {
                                -gap * 0.5 - width * 0.5
                            },
                        width: used,
                        height: width,
                    },
                    style,
                    color,
                });
            }
        }
    }
    if source_children.iter().any(|c| c.style.fragmentation.span()) {
        diagnostics.push(Diagnostic {
            node_id: b.node_id,
            message:
                "vertical column spanner bands are outside the finite native multicolumn profile"
                    .into(),
        });
    }
    b.children = children;
    b.fragment_source = Some(source_children);
    b.line_boxes.clear();
    b.rect = full.rect;
    b.content_rect = full.content_rect;
    let block_decor = full.rect.width - full.content_rect.width;
    b.content_rect.width = cap.unwrap_or(used);
    b.rect.width = b.content_rect.width + block_decor;
}
