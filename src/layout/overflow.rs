//! Overflow is spatial ancestry, not paint-group ancestry. Each flattened
//! phase receives a complete clip/translation so escaped z contexts retain
//! their containing-block clips without accidentally clipping fixed boxes.
use std::collections::HashMap;

use crate::hash::FastMap;

use crate::{
    dom::NodeId,
    style::{Direction, Overflow, Position},
};

use super::{BoxKind, CoordinateSpace, LayoutBox, LayoutTree, PaintPhase, Rect, tree::finite_used};
mod scrolling;
pub use scrolling::ScrollBehavior;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollOffset {
    pub x: f32,
    pub y: f32,
}

/// Snapshot-local, real DOM identity; construction IDs never survive rebuilds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollState {
    identity: std::sync::Arc<()>,
    nodes: HashMap<ScrollKey, ScrollOffset>,
    viewport: ScrollOffset,
    clock: f64,
    motions: HashMap<Option<ScrollKey>, scrolling::Motion>,
    gestures: HashMap<Option<ScrollKey>, scrolling::Gesture>,
    snapped: HashMap<(Option<ScrollKey>, bool), NodeId>,
}

impl ScrollState {
    pub(crate) fn same_document(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.identity, &other.identity)
    }
    pub fn needs_tick(&self) -> bool {
        !self.motions.is_empty() || !self.gestures.is_empty()
    }
    pub fn next_deadline(&self) -> Option<f64> {
        self.gestures
            .values()
            .map(|g| g.deadline)
            .min_by(f64::total_cmp)
    }
    pub fn has_animations(&self) -> bool {
        !self.motions.is_empty()
    }
    pub fn viewport_scroll(&self) -> ScrollOffset {
        self.viewport
    }
    pub(crate) fn set_time(&mut self, seconds: f64) {
        self.clock = seconds;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ScrollKey {
    node: NodeId,
    pseudo: Option<crate::style::PseudoElement>,
}

fn key(b: &LayoutBox) -> Option<ScrollKey> {
    b.node_id.map(|node| ScrollKey {
        node,
        pseudo: b.pseudo,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollRange {
    pub min: ScrollOffset,
    pub max: ScrollOffset,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollContainer {
    pub port: Rect,
    /// Unscrolled content bounds, including ink, distinct from the border box.
    pub extent: Rect,
    pub range: ScrollRange,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
}

/// Unbounded axes use None, never sentinel infinities in layout arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClipRect {
    pub x: Option<(f32, f32)>,
    pub y: Option<(f32, f32)>,
}

impl ClipRect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        self.x.is_none_or(|(a, b)| x >= a && x < b) && self.y.is_none_or(|(a, b)| y >= a && y < b)
    }

    fn intersect(self, other: Self) -> Self {
        fn axis(a: Option<(f32, f32)>, b: Option<(f32, f32)>) -> Option<(f32, f32)> {
            match (a, b) {
                (Some((a, b)), Some((c, d))) => Some((a.max(c), b.min(d))),
                (a, b) => a.or(b),
            }
        }
        Self {
            x: axis(self.x, other.x),
            y: axis(self.y, other.y),
        }
    }

    pub(crate) fn apply(self, rect: Rect) -> Option<Rect> {
        let left = self.x.map_or(rect.x, |(a, _)| a.max(rect.x));
        let right = self
            .x
            .map_or(rect.x + rect.width, |(_, b)| b.min(rect.x + rect.width));
        let top = self.y.map_or(rect.y, |(a, _)| a.max(rect.y));
        let bottom = self
            .y
            .map_or(rect.y + rect.height, |(_, b)| b.min(rect.y + rect.height));
        (right > left && bottom > top).then_some(Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// Most boxes have no transformed clipping ancestry, so plane clips are
/// shared rather than stored inline; this keeps per-box states cheap to copy.
pub const MAX_PLANE_CLIPS: usize = 32;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpatialState {
    /// Translation from layout to visual coordinates, before viewport Y scroll.
    pub offset: ScrollOffset,
    pub clip: ClipRect,
    pub matrix: super::Mat4,
    pub plane_clips: std::sync::Arc<[PlaneClip]>,
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneClip {
    pub matrix: super::Mat4,
    pub clip: ClipRect,
}

impl SpatialState {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        !self.hidden
            && self.clip.contains(x, y)
            && self.plane_clips.iter().all(|c| {
                c.matrix
                    .unproject(x, y)
                    .is_some_and(|(x, y)| c.clip.contains(x, y))
            })
    }
    pub fn point(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        if !self.contains(x, y) {
            return None;
        }
        self.matrix.unproject(x, y)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scrollbar {
    pub(crate) owner: usize,
    pub track: Rect,
    pub thumb: Rect,
    pub vertical: bool,
    pub spatial: SpatialState,
    pub coordinate_space: CoordinateSpace,
    pub thumb_color: crate::css::values::ResolvedColor,
    pub track_color: crate::css::values::ResolvedColor,
    key: ScrollKey,
    range: ScrollRange,
    pub(crate) viewport: bool,
}

impl Scrollbar {
    pub fn node_id(&self) -> NodeId {
        self.key.node
    }

    fn point(&self, x: f32, y: f32, root_scroll: f32) -> (f32, f32) {
        let y = y + if self.coordinate_space == CoordinateSpace::Document {
            root_scroll
        } else {
            0.0
        };
        self.spatial
            .matrix
            .unproject(x, y)
            .unwrap_or((f32::NAN, f32::NAN))
    }

    fn contains(&self, x: f32, y: f32, root_scroll: f32) -> bool {
        let doc_y = y + if self.coordinate_space == CoordinateSpace::Document {
            root_scroll
        } else {
            0.0
        };
        let (x_local, y_local) = self.point(x, y, root_scroll);
        self.spatial.contains(x, doc_y)
            && x_local >= self.track.x
            && x_local < self.track.x + self.track.width
            && y_local >= self.track.y
            && y_local < self.track.y + self.track.height
    }
}

type Definitions<'a> = FastMap<usize, &'a LayoutBox>;
/// Per-box spatial states keyed by box id.
pub(crate) type SpatialStates = FastMap<usize, SpatialState>;

#[derive(Clone, Debug, Default)]
struct SpatialPrefix {
    dx: f64,
    dy: f64,
    clip: ClipRect,
    /// Non-finite clip metrics, replayed as each descendant's diagnostics.
    failures: Vec<&'static str>,
    containing_block: bool,
}

/// Per-tree state shared by every candidate of one hit test, so probing a
/// paint item never rebuilds the box map or spatial states.
pub(crate) struct HitContext<'a> {
    map: Definitions<'a>,
    pub(crate) decoration: SpatialStates,
    pub(crate) content: SpatialStates,
}

impl HitContext<'_> {
    pub(crate) fn state(&self, b: &LayoutBox, phase: PaintPhase) -> &SpatialState {
        if matches!(phase, PaintPhase::Content | PaintPhase::Text(_)) {
            &self.content[&b.box_id]
        } else {
            &self.decoration[&b.box_id]
        }
    }
}

/// Keyed by (box id, document coordinate space).
#[derive(Default)]
struct SpatialMemo(FastMap<(usize, bool), SpatialPrefix>);

fn boxes(root: &LayoutBox) -> Vec<&LayoutBox> {
    fn walk<'a>(b: &'a LayoutBox, result: &mut Vec<&'a LayoutBox>) {
        result.push(b);
        for child in &b.children {
            walk(child, result);
        }
    }
    let mut result = Vec::new();
    walk(root, &mut result);
    result
}

fn definitions(root: &LayoutBox) -> Definitions<'_> {
    fn walk<'a>(b: &'a LayoutBox, map: &mut Definitions<'a>) {
        map.entry(b.box_id).or_insert(b);
        for child in &b.children {
            walk(child, map);
        }
    }
    let mut map = Definitions::default();
    walk(root, &mut map);
    map
}

fn next(b: &LayoutBox) -> Option<usize> {
    match b.style.position {
        Position::Fixed if b.coordinate_space == CoordinateSpace::Viewport => None,
        Position::Fixed => b.position_parent,
        Position::Absolute => b.position_parent,
        _ => b.parent_box,
    }
}

fn lineage<'a>(b: &'a LayoutBox, map: &Definitions<'a>, own: bool) -> Vec<&'a LayoutBox> {
    let mut path = Vec::new();
    let mut id = if own { Some(b.box_id) } else { next(b) };
    while let Some(ancestor) = id.and_then(|id| map.get(&id)) {
        path.push(*ancestor);
        id = next(ancestor);
    }
    path.reverse();
    path
}

fn port(b: &LayoutBox) -> Rect {
    let border = b.style.border_width;
    Rect {
        x: b.rect.x + border.left,
        y: b.rect.y + border.top,
        width: (b.rect.width - border.left - border.right).max(0.0),
        height: (b.rect.height - border.top - border.bottom).max(0.0),
    }
}

fn clip(container: ScrollContainer, offset: ScrollOffset) -> ClipRect {
    let r = container.port;
    ClipRect {
        x: container
            .overflow_x
            .clips()
            .then_some((r.x + offset.x, r.x + r.width + offset.x)),
        y: container
            .overflow_y
            .clips()
            .then_some((r.y + offset.y, r.y + r.height + offset.y)),
    }
}

fn content_ink(b: &LayoutBox) -> Option<Rect> {
    let mut rect = (b.kind == BoxKind::Text || b.image.is_some()).then_some(b.content_rect);
    let mut include = |ink: Rect| {
        rect = Some(rect.map_or(ink, |rect| rect.union(ink)));
    };
    for f in &b.text_fragments {
        include(f.ink_rect);
    }
    for line in &b.line_boxes {
        include(line.content_rect);
    }
    if let Some(marker) = &b.list_marker {
        include(
            marker
                .glyphs
                .ink_bounds(marker.style.font_size, marker.x, marker.baseline),
        );
    }
    rect
}

fn clamped(offset: ScrollOffset, range: ScrollRange) -> ScrollOffset {
    ScrollOffset {
        x: offset.x.clamp(range.min.x, range.max.x),
        y: offset.y.clamp(range.min.y, range.max.y),
    }
}

impl LayoutTree {
    /// Styled overlay tracks; no reserved gutter.
    pub fn scrollbars(&self) -> Vec<Scrollbar> {
        if !self
            .overflow
            .values()
            .any(|c| c.overflow_x.user_scrollable() || c.overflow_y.user_scrollable())
            && !self.root_element().is_some_and(|b| {
                b.style
                    .scrolling
                    .get(crate::style::scrolling::Property::Color)
                    != crate::style::scrolling::Value::Color(None)
                    || b.style.scrolling.width() == crate::style::scrolling::Width::Thin
            })
        {
            return Vec::new();
        }
        let decoration = self.spatial_states(false);
        let mut bars = Vec::new();
        for (b, phase) in super::paint_order(self) {
            if phase != PaintPhase::Scrollbars {
                continue;
            }
            let (Some(container), Some(key)) = (self.overflow.get(&b.box_id), key(b)) else {
                continue;
            };
            let r = container.port;
            if b.style.scrolling.width() == crate::style::scrolling::Width::None {
                continue;
            }
            let offset = self.offset_for(key);
            let show = |mode: Overflow, min: f32, max: f32| {
                mode == Overflow::Scroll || mode == Overflow::Auto && max > min
            };
            let horizontal = show(
                container.overflow_x,
                container.range.min.x,
                container.range.max.x,
            );
            let vertical = show(
                container.overflow_y,
                container.range.min.y,
                container.range.max.y,
            );
            let thickness = (if b.style.scrolling.width() == crate::style::scrolling::Width::Thin {
                3.0f32
            } else {
                6.0f32
            })
            .min(r.width)
            .min(r.height);
            let (thumb_color, track_color) = b.style.scrolling.colors(b.style.text_color);
            if thickness <= 0.0 {
                continue;
            }
            for (visible, is_vertical) in [(horizontal, false), (vertical, true)] {
                if !visible {
                    continue;
                }
                let (min, max, position, size) = if is_vertical {
                    (
                        container.range.min.y,
                        container.range.max.y,
                        offset.y,
                        r.height,
                    )
                } else {
                    (
                        container.range.min.x,
                        container.range.max.x,
                        offset.x,
                        r.width,
                    )
                };
                let track = if is_vertical {
                    Rect {
                        x: r.x + r.width - thickness,
                        y: r.y,
                        width: thickness,
                        height: (r.height - if horizontal { thickness } else { 0.0 }).max(0.0),
                    }
                } else {
                    Rect {
                        x: r.x,
                        y: r.y + r.height - thickness,
                        width: (r.width - if vertical { thickness } else { 0.0 }).max(0.0),
                        height: thickness,
                    }
                };
                let length = if is_vertical {
                    track.height
                } else {
                    track.width
                };
                let thumb_length = (length as f64 * size as f64
                    / (size as f64 + max as f64 - min as f64))
                    .max(12.0)
                    .min(length as f64) as f32;
                let fraction = if max > min {
                    (position as f64 - min as f64) / (max as f64 - min as f64)
                } else {
                    0.0
                };
                let start = ((length - thumb_length) as f64 * fraction) as f32;
                let thumb = if is_vertical {
                    Rect {
                        y: track.y + start,
                        height: thumb_length,
                        ..track
                    }
                } else {
                    Rect {
                        x: track.x + start,
                        width: thumb_length,
                        ..track
                    }
                };
                bars.push(Scrollbar {
                    owner: b.box_id,
                    track,
                    thumb,
                    vertical: is_vertical,
                    spatial: decoration[&b.box_id].clone(),
                    coordinate_space: b.coordinate_space,
                    thumb_color,
                    track_color,
                    key,
                    range: container.range,
                    viewport: false,
                });
            }
        }
        bars.extend(self.viewport_scrollbars());
        bars
    }

    pub fn scrollbar_at(&self, x: f32, y: f32) -> Option<Scrollbar> {
        self.scrollbar_at_in_space(x, y, self.viewport_scroll().y, false)
    }

    pub(crate) fn scrollbar_at_in_space(
        &self,
        x: f32,
        y: f32,
        root_scroll: f32,
        document_x: bool,
    ) -> Option<Scrollbar> {
        let mut context = None;
        self.scrollbar_at_with(x, y, root_scroll, document_x, &mut context)
    }

    pub(crate) fn hit_context(&self) -> HitContext<'_> {
        let map = definitions(&self.root);
        let sticky = self.sticky_offsets(&map);
        let mut memo = SpatialMemo::default();
        let mut decoration = SpatialStates::default();
        let mut content = SpatialStates::default();
        decoration.reserve(map.len());
        content.reserve(map.len());
        for b in map.values() {
            let reports = &mut Vec::new();
            decoration.insert(
                b.box_id,
                self.spatial_metrics(b, false, &map, &sticky, &mut memo, reports),
            );
            content.insert(
                b.box_id,
                self.spatial_metrics(b, true, &map, &sticky, &mut memo, reports),
            );
        }
        HitContext {
            map,
            decoration,
            content,
        }
    }

    /// `scrollbar_at_in_space`, building `context` only when a non-viewport
    /// bar needs the paint-order probe; callers reuse it for their own hits.
    pub(crate) fn scrollbar_at_with<'a>(
        &'a self,
        x: f32,
        y: f32,
        root_scroll: f32,
        document_x: bool,
        context: &mut Option<HitContext<'a>>,
    ) -> Option<Scrollbar> {
        let bars = self.scrollbars();
        if bars.is_empty() {
            return None;
        }
        if let Some(bar) = bars
            .iter()
            .find(|bar| bar.viewport && bar.contains(x, y, root_scroll))
        {
            return Some(bar.clone());
        }
        if bars.iter().all(|bar| bar.viewport) {
            return None;
        }
        let context = context.get_or_insert_with(|| self.hit_context());
        for (b, phase) in super::paint_order(self).into_iter().rev() {
            let x = x - if document_x && b.coordinate_space == CoordinateSpace::Document {
                self.viewport_scroll().x
            } else {
                0.0
            };
            if phase == PaintPhase::Scrollbars
                && let Some(bar) = bars.iter().rev().find(|bar| {
                    bar.owner == b.box_id
                        && bar.contains(x, y, root_scroll)
                        && self.effect_hit(b, x, y, root_scroll, context)
                })
            {
                return Some(bar.clone());
            }
            if b.node_id.is_some() && self.phase_hit(b, phase, context, x, y, root_scroll) {
                return None;
            }
        }
        None
    }

    /// Distance into the thumb, or center the thumb for a track click.
    pub fn scrollbar_grab(&self, bar: &Scrollbar, x: f32, y: f32) -> f32 {
        let (x, y) = bar.point(x, y, self.viewport_scroll().y);
        if !x.is_finite() || !y.is_finite() {
            return 0.0;
        }
        let (point, start, length) = if bar.vertical {
            (y, bar.thumb.y, bar.thumb.height)
        } else {
            (x, bar.thumb.x, bar.thumb.width)
        };
        if point >= start && point < start + length {
            point - start
        } else {
            length / 2.0
        }
    }

    pub fn drag_scrollbar(&mut self, bar: &Scrollbar, x: f32, y: f32, grab: f32) -> bool {
        let Some(bar) = self.scrollbars().into_iter().find(|current| {
            current.key == bar.key
                && current.vertical == bar.vertical
                && current.viewport == bar.viewport
        }) else {
            return false;
        };
        let (x, y) = bar.point(x, y, self.viewport_scroll().y);
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let (point, start, track, thumb, min, max) = if bar.vertical {
            (
                y,
                bar.track.y,
                bar.track.height,
                bar.thumb.height,
                bar.range.min.y,
                bar.range.max.y,
            )
        } else {
            (
                x,
                bar.track.x,
                bar.track.width,
                bar.thumb.width,
                bar.range.min.x,
                bar.range.max.x,
            )
        };
        if track <= thumb {
            return false;
        }
        let fraction = ((point as f64 - start as f64 - grab as f64)
            / (track as f64 - thumb as f64))
            .clamp(0.0, 1.0);
        let position = finite_used(
            min as f64 + fraction * (max as f64 - min as f64),
            Some(bar.key.node),
            "scrollbar position",
            &mut self.diagnostics,
        );
        let old = if bar.viewport {
            self.viewport_scroll()
        } else {
            self.offset_for(bar.key)
        };
        let next = if bar.vertical {
            ScrollOffset { y: position, ..old }
        } else {
            ScrollOffset { x: position, ..old }
        };
        if bar.viewport {
            self.scroll_state.viewport = next;
        } else {
            self.scroll_state.nodes.insert(bar.key, next);
        }
        self.cancel_scroll_axis(
            if bar.viewport {
                None
            } else {
                Some(bar.key.node)
            },
            !bar.vertical,
            bar.vertical,
        );
        self.report_spatial_failures();
        old != next
    }

    pub fn scroll_state(&self) -> &ScrollState {
        &self.scroll_state
    }

    /// Seed before layout; the final pass clamps after measuring current ink.
    pub fn restore_scroll_state(&mut self, state: ScrollState) {
        self.scroll_state = state;
    }

    pub fn viewport_scroll(&self) -> ScrollOffset {
        self.scroll_state.viewport
    }

    /// Carry scrolling performed against an older layout of the same document
    /// onto this one, clamped to this layout's ranges. Returns whether the
    /// state changed; a different document's state is ignored.
    pub fn adopt_scroll_state(&mut self, state: &ScrollState) -> bool {
        if !self.scroll_state.same_document(state) || *state == self.scroll_state {
            return false;
        }
        let mut next = state.clone();
        next.viewport = clamped(next.viewport, self.viewport_overflow.range);
        if !next.nodes.is_empty() {
            let ranges: HashMap<ScrollKey, ScrollRange> = definitions(&self.root)
                .values()
                .filter_map(|b| Some((key(b)?, self.overflow.get(&b.box_id)?.range)))
                .collect();
            for (key, offset) in &mut next.nodes {
                if let Some(range) = ranges.get(key) {
                    *offset = clamped(*offset, *range);
                }
            }
        }
        let changed = next != self.scroll_state;
        self.scroll_state = next;
        changed
    }

    pub fn viewport_scroll_container(&self) -> ScrollContainer {
        self.viewport_overflow
    }

    pub fn scroll_container(&self, node: NodeId) -> Option<ScrollContainer> {
        let b = definitions(&self.root)
            .values()
            .find(|b| b.node_id == Some(node) && b.pseudo.is_none())
            .copied()?;
        self.overflow.get(&b.box_id).copied()
    }

    pub(crate) fn box_scroll_container(&self, b: &LayoutBox) -> Option<ScrollContainer> {
        self.overflow.get(&b.box_id).copied()
    }

    pub(crate) fn box_scroll_offset(&self, b: &LayoutBox) -> ScrollOffset {
        key(b).map_or(ScrollOffset::default(), |key| self.offset_for(key))
    }

    pub fn scroll_offset(&self, node: NodeId) -> ScrollOffset {
        self.offset_for(ScrollKey { node, pseudo: None })
    }

    fn offset_for(&self, key: ScrollKey) -> ScrollOffset {
        self.scroll_state
            .nodes
            .get(&key)
            .copied()
            .unwrap_or_default()
    }

    fn valid_offset(&mut self, offset: ScrollOffset, node: Option<NodeId>) -> ScrollOffset {
        ScrollOffset {
            x: finite_used(offset.x as f64, node, "scroll x", &mut self.diagnostics),
            y: finite_used(offset.y as f64, node, "scroll y", &mut self.diagnostics),
        }
    }

    /// Hidden allows programmatic scrolling; clip/visible axes have no range.
    pub fn set_scroll_offset(&mut self, node: NodeId, offset: ScrollOffset) -> bool {
        let offset = self.valid_offset(offset, Some(node));
        let Some(container) = self.scroll_container(node) else {
            return false;
        };
        let next = clamped(offset, container.range);
        let changed = self.scroll_offset(node) != next;
        self.scroll_state
            .nodes
            .insert(ScrollKey { node, pseudo: None }, next);
        self.report_spatial_failures();
        changed
    }

    pub fn set_viewport_scroll(&mut self, offset: ScrollOffset) -> bool {
        let offset = self.valid_offset(offset, None);
        let next = clamped(offset, self.viewport_overflow.range);
        let changed = self.scroll_state.viewport != next;
        self.scroll_state.viewport = next;
        self.report_spatial_failures();
        changed
    }

    pub(crate) fn spatial_states(&self, own: bool) -> SpatialStates {
        let map = definitions(&self.root);
        let sticky = self.sticky_offsets(&map);
        let mut memo = SpatialMemo::default();
        map.values()
            .map(|b| {
                (
                    b.box_id,
                    self.spatial_metrics(b, own, &map, &sticky, &mut memo, &mut Vec::new()),
                )
            })
            .collect()
    }

    /// `(decoration, content)` states, i.e. `spatial_states(false)` and
    /// `spatial_states(true)`, sharing one ancestry walk and prefix memo.
    pub(crate) fn spatial_state_pair(&self) -> (SpatialStates, SpatialStates) {
        let context = self.hit_context();
        (context.decoration, context.content)
    }

    fn spatial(&self, b: &LayoutBox, own: bool, map: &Definitions<'_>) -> SpatialState {
        self.spatial_metrics(
            b,
            own,
            map,
            &self.sticky_offsets(map),
            &mut SpatialMemo::default(),
            &mut Vec::new(),
        )
    }

    /// Fold one lineage member into the accumulated scroll translation and
    /// clip; `container` is false only for a box excluding its own scrollport.
    fn fold_ancestor(
        &self,
        prefix: &mut SpatialPrefix,
        ancestor: &LayoutBox,
        sticky: &HashMap<usize, ScrollOffset>,
        container: bool,
    ) {
        fn metric(value: f64, property: &'static str, failures: &mut Vec<&'static str>) -> f32 {
            let result = value as f32;
            if result.is_finite() {
                result
            } else {
                failures.push(property);
                0.0
            }
        }
        let shift = sticky.get(&ancestor.box_id).copied().unwrap_or_default();
        prefix.dx += shift.x as f64;
        prefix.dy += shift.y as f64;
        prefix.containing_block |= ancestor.style.effects.containing_block();
        if !container {
            return;
        }
        if let Some(container) = self.overflow.get(&ancestor.box_id) {
            let r = container.port;
            let (dx, dy) = (prefix.dx, prefix.dy);
            let failures = &mut prefix.failures;
            let x = container.overflow_x.clips().then(|| {
                (
                    metric(r.x as f64 + dx, "scroll clip left", failures),
                    metric(
                        r.x as f64 + r.width as f64 + dx,
                        "scroll clip right",
                        failures,
                    ),
                )
            });
            let y = container.overflow_y.clips().then(|| {
                (
                    metric(r.y as f64 + dy, "scroll clip top", failures),
                    metric(
                        r.y as f64 + r.height as f64 + dy,
                        "scroll clip bottom",
                        failures,
                    ),
                )
            });
            prefix.clip = prefix.clip.intersect(ClipRect { x, y });
            let offset = key(ancestor).map_or(ScrollOffset::default(), |id| self.offset_for(id));
            prefix.dx -= offset.x as f64;
            prefix.dy -= offset.y as f64;
        }
    }

    /// Accumulated state over `lineage(head, map, true)`, memoized per
    /// ancestor so every box costs O(1) amortized instead of O(depth).
    fn spatial_prefix(
        &self,
        head: Option<&LayoutBox>,
        document: bool,
        map: &Definitions<'_>,
        sticky: &HashMap<usize, ScrollOffset>,
        memo: &mut SpatialMemo,
    ) -> SpatialPrefix {
        let mut chain = Vec::new();
        let mut base = None;
        let mut current = head;
        while let Some(ancestor) = current {
            if let Some(prefix) = memo.0.get(&(ancestor.box_id, document)) {
                base = Some(prefix.clone());
                break;
            }
            chain.push(ancestor);
            current = next(ancestor).and_then(|id| map.get(&id)).copied();
        }
        let mut prefix = base.unwrap_or_else(|| SpatialPrefix {
            dx: if document {
                -self.scroll_state.viewport.x as f64
            } else {
                0.0
            },
            ..SpatialPrefix::default()
        });
        for ancestor in chain.into_iter().rev() {
            self.fold_ancestor(&mut prefix, ancestor, sticky, true);
            memo.0.insert((ancestor.box_id, document), prefix.clone());
        }
        prefix
    }

    fn spatial_metrics(
        &self,
        b: &LayoutBox,
        own: bool,
        map: &Definitions<'_>,
        sticky: &HashMap<usize, ScrollOffset>,
        memo: &mut SpatialMemo,
        reports: &mut Vec<crate::style::Diagnostic>,
    ) -> SpatialState {
        let before = reports.len();
        let mut metric = |value: f64, property: &str| {
            let result = value as f32;
            if result.is_finite() {
                result
            } else {
                reports.push(crate::style::Diagnostic {
                    node_id: b.node_id,
                    message: format!("{property}: invalid at used-value time: non-finite layout result; compatibility fallback is 0px"),
                });
                0.0
            }
        };
        let mut state = SpatialState::default();
        let document = b.coordinate_space == CoordinateSpace::Document;
        let head = map.get(&b.box_id).copied();
        let prefix = if own {
            self.spatial_prefix(head, document, map, sticky, memo)
        } else {
            let parent = head.and_then(next).and_then(|id| map.get(&id)).copied();
            let mut prefix = self.spatial_prefix(parent, document, map, sticky, memo);
            if let Some(head) = head {
                self.fold_ancestor(&mut prefix, head, sticky, false);
            }
            prefix
        };
        for property in &prefix.failures {
            metric(f64::INFINITY, property);
        }
        state.clip = prefix.clip;
        let (dx, dy) = (prefix.dx, prefix.dy);
        state.offset.x = metric(dx, "scroll translation x");
        state.offset.y = metric(dy, "scroll translation y");
        state.matrix = super::Mat4::translation(dx, dy, 0.0);
        if prefix.containing_block {
            let path = lineage(b, map, true);
            use crate::style::effects::{Keyword, Property};
            state.clip = ClipRect::default();
            let mut matrix = super::Mat4::translation(
                if b.coordinate_space == CoordinateSpace::Document {
                    -self.scroll_state.viewport.x as f64
                } else {
                    0.0
                },
                0.0,
                0.0,
            );
            let mut plane_clips = Vec::new();
            for ancestor in path {
                let shift = sticky.get(&ancestor.box_id).copied().unwrap_or_default();
                matrix = matrix.then(super::Mat4::translation(
                    shift.x as f64,
                    shift.y as f64,
                    0.0,
                ));
                let effects = &ancestor.style.effects;
                matrix = matrix.then(effects.matrix(
                    ancestor.rect,
                    ancestor.content_rect,
                    ancestor.style.font_size,
                ));
                if effects.keyword(Property::BackfaceVisibility) == Keyword::Hidden
                    && matrix.back_facing()
                {
                    state.hidden = true;
                }
                if (own || ancestor.box_id != b.box_id)
                    && let Some(container) = self.overflow.get(&ancestor.box_id)
                {
                    if plane_clips.len() == MAX_PLANE_CLIPS {
                        state.hidden = true;
                        metric(
                            f64::INFINITY,
                            "effects transformed clip ancestry exceeds 32",
                        );
                        break;
                    }
                    plane_clips.push(PlaneClip {
                        matrix,
                        clip: clip(*container, ScrollOffset::default()),
                    });
                    let offset =
                        key(ancestor).map_or(ScrollOffset::default(), |id| self.offset_for(id));
                    matrix = matrix.then(super::Mat4::translation(
                        -offset.x as f64,
                        -offset.y as f64,
                        0.0,
                    ));
                }
                if ancestor.box_id != b.box_id {
                    if effects.keyword(Property::TransformStyle) != Keyword::Preserve3d
                        || effects.grouped()
                    {
                        matrix = matrix.then(super::Mat4::scale(1.0, 1.0, 0.0));
                    }
                    matrix =
                        matrix.then(effects.perspective(ancestor.rect, ancestor.style.font_size));
                }
            }
            state.plane_clips = plane_clips.into();
            state.matrix = matrix;
            state.hidden |= !matrix.finite();
        }
        for rect in [b.rect, b.content_rect] {
            metric(rect.x as f64 + dx, "scrolled box x");
            metric(rect.y as f64 + dy, "scrolled box y");
        }
        for text in &b.text_fragments {
            metric(text.x as f64 + dx, "scrolled glyph x");
            metric(text.baseline as f64 + dy, "scrolled glyph baseline");
        }
        if let Some(r) = self.page_clip {
            state.clip = state.clip.intersect(ClipRect {
                x: Some((r.x, r.x + r.width)),
                y: Some((r.y, r.y + r.height)),
            });
        }
        if reports.len() != before {
            state.clip = ClipRect {
                x: Some((0.0, 0.0)),
                y: Some((0.0, 0.0)),
            };
        }
        state
    }

    fn sticky_offsets(&self, map: &Definitions<'_>) -> HashMap<usize, ScrollOffset> {
        use crate::css::values::{LengthContext, LengthOrAuto};
        let mut result: HashMap<usize, ScrollOffset> = HashMap::new();
        let mut candidates: Vec<_> = map
            .values()
            .filter(|b| b.style.position == Position::Sticky)
            .copied()
            .collect();
        candidates.sort_by_key(|b| lineage(b, map, true).len());
        for b in candidates {
            let path = lineage(b, map, false);
            let nearest = path.iter().rposition(|a| {
                self.overflow.get(&a.box_id).is_some_and(|c| {
                    c.overflow_x.scroll_container() || c.overflow_y.scroll_container()
                })
            });
            let port = nearest.map_or(self.viewport_overflow.port, |i| {
                self.overflow[&path[i].box_id].port
            });
            let mut scrolled =
                nearest.map_or(self.viewport_scroll(), |i| self.box_scroll_offset(path[i]));
            let mut inherited = ScrollOffset::default();
            for a in path.iter().skip(nearest.map_or(0, |i| i + 1)) {
                let shift = result.get(&a.box_id).copied().unwrap_or_default();
                inherited.x += shift.x;
                inherited.y += shift.y;
                let offset = self.box_scroll_offset(a);
                scrolled.x += offset.x;
                scrolled.y += offset.y;
            }
            let cb =
                b.parent_box
                    .and_then(|id| map.get(&id))
                    .map_or(self.root.content_rect, |parent| {
                        self.overflow
                            .get(&parent.box_id)
                            .filter(|c| {
                                c.overflow_x.scroll_container() || c.overflow_y.scroll_container()
                            })
                            .map_or(parent.content_rect, |c| parent.content_rect.union(c.extent))
                    });
            let inset = |l: &LengthOrAuto, size: f32| match l {
                LengthOrAuto::Auto => None,
                LengthOrAuto::Length(l) => l
                    .used(&LengthContext::default(), size)
                    .map(|v| v.0)
                    .filter(|v| v.is_finite()),
            };
            // End inset is reduced when the sticky view rectangle is smaller
            // than the border box; the margin box remains in its flow CB.
            let axis = |start: f32,
                        size: f32,
                        port_start: f32,
                        port_size: f32,
                        scroll: f32,
                        inherited: f32,
                        low: Option<f32>,
                        high: Option<f32>,
                        cb_start: f32,
                        cb_size: f32,
                        margin_start: f32,
                        margin_end: f32,
                        reverse: bool| {
                let (mut lo, mut hi) = (low.unwrap_or(0.0), high.unwrap_or(0.0));
                let shortage = (size - (port_size - lo - hi)).max(0.0);
                if reverse {
                    lo -= shortage;
                } else {
                    hi -= shortage;
                }
                let visual = start + inherited - scroll;
                let mut shift = 0.0f32;
                if low.is_some() {
                    shift = shift.max(port_start + lo - visual);
                }
                if high.is_some() {
                    shift = shift.min(port_start + port_size - hi - visual - size);
                }
                let lower = (cb_start - (start - margin_start)).min(0.0);
                let upper = (cb_start + cb_size - (start + size + margin_end)).max(0.0);
                shift.clamp(lower, upper)
            };
            let (rx, ry) = scroll_reversals(&b.style);
            let shift = ScrollOffset {
                x: axis(
                    b.rect.x,
                    b.rect.width,
                    port.x,
                    port.width,
                    scrolled.x,
                    inherited.x,
                    inset(&b.style.inset.left, port.width),
                    inset(&b.style.inset.right, port.width),
                    cb.x,
                    cb.width,
                    b.margin.left,
                    b.margin.right,
                    rx,
                ),
                y: axis(
                    b.rect.y,
                    b.rect.height,
                    port.y,
                    port.height,
                    scrolled.y,
                    inherited.y,
                    inset(&b.style.inset.top, port.height),
                    inset(&b.style.inset.bottom, port.height),
                    cb.y,
                    cb.height,
                    b.margin.top,
                    b.margin.bottom,
                    ry,
                ),
            };
            result.insert(b.box_id, shift);
        }
        result
    }

    fn report_spatial_failures(&mut self) {
        let map = definitions(&self.root);
        let sticky = self.sticky_offsets(&map);
        let mut reports = Vec::new();
        let mut memo = SpatialMemo::default();
        for b in map.values() {
            for own in [false, true] {
                self.spatial_metrics(b, own, &map, &sticky, &mut memo, &mut reports);
            }
        }
        for report in reports {
            if !self
                .diagnostics
                .iter()
                .any(|d| d.node_id == report.node_id && d.message == report.message)
            {
                eprintln!("CSS at {:?}: {}", report.node_id, report.message);
                self.diagnostics.push(report);
            }
        }
    }

    /// Used/layout bounds remain untouched; this is visible border geometry.
    pub fn visual_node_rect(&self, node: NodeId, root_scroll: f32) -> Option<Rect> {
        let map = definitions(&self.root);
        boxes(&self.root)
            .into_iter()
            .filter(|b| b.node_id == Some(node) && b.pseudo.is_none())
            .filter_map(|b| {
                let state = self.spatial(b, false, &map);
                if state.hidden {
                    return None;
                }
                let rect = state.matrix.bounds(b.rect)?;
                state
                    .clip
                    .apply(rect)
                    .map(|mut rect| {
                        if b.coordinate_space == CoordinateSpace::Document {
                            rect.y -= root_scroll;
                        }
                        rect
                    })
                    .and_then(|rect| {
                        ClipRect {
                            x: Some((0.0, self.viewport_overflow.port.width)),
                            y: Some((0.0, self.viewport_overflow.port.height)),
                        }
                        .apply(rect)
                    })
            })
            .reduce(Rect::union)
    }

    /// Deepest visible hit's containing-block scroll lineage consumes each
    /// axis independently, then chains the unconsumed delta to the viewport.
    pub fn scroll_wheel(&mut self, x: f32, y: f32, delta: ScrollOffset) -> bool {
        let mut delta = self.valid_offset(delta, None);
        let mut changed = false;
        let mut context = None;
        let root_scroll = self.viewport_scroll().y;
        let bar = self.scrollbar_at_with(x, y, root_scroll, false, &mut context);
        let context = context.unwrap_or_else(|| self.hit_context());
        let target = bar.map(|bar| context.map[&bar.owner]).or_else(|| {
            super::paint_order(self)
                .into_iter()
                .rev()
                .find_map(|(b, phase)| {
                    self.phase_hit(b, phase, &context, x, y, root_scroll)
                        .then_some(b)
                })
        });
        let nodes: Vec<_> = target
            .map(|b| lineage(b, &context.map, true))
            .unwrap_or_default()
            .into_iter()
            .rev()
            .filter_map(|b| {
                self.overflow
                    .get(&b.box_id)
                    .and_then(|container| key(b).map(|id| (id, *container)))
            })
            .collect();
        for (id, container) in nodes {
            self.cancel_scroll_axis(Some(id.node), delta.x != 0.0, delta.y != 0.0);
            let old = self.offset_for(id);
            let next = consume(
                old,
                container,
                &mut delta,
                Some(id.node),
                &mut self.diagnostics,
            );
            changed |= old != next;
            self.scroll_state.nodes.insert(id, next);
            self.record_gesture(Some(id), old, next);
            let overscroll = self.scroll_style(Some(id.node));
            if container.overflow_x.scroll_container()
                && overscroll.overscroll(false) != crate::style::scrolling::Overscroll::Auto
            {
                delta.x = 0.0;
            }
            if container.overflow_y.scroll_container()
                && overscroll.overscroll(true) != crate::style::scrolling::Overscroll::Auto
            {
                delta.y = 0.0;
            }
        }

        let old = self.viewport_scroll();
        self.cancel_scroll_axis(None, delta.x != 0.0, delta.y != 0.0);
        let next = consume(
            old,
            self.viewport_overflow,
            &mut delta,
            None,
            &mut self.diagnostics,
        );
        changed |= old != next;
        self.scroll_state.viewport = next;
        self.record_gesture(None, old, next);
        self.report_spatial_failures();
        changed
    }

    pub(super) fn effect_hit(
        &self,
        b: &LayoutBox,
        x: f32,
        y: f32,
        scroll: f32,
        context: &HitContext<'_>,
    ) -> bool {
        use crate::style::effects::{Keyword, Property, Value};
        let mut candidate = Some(b.box_id);
        while let Some(a) = candidate.and_then(|id| context.map.get(&id)) {
            candidate = a.parent_box;
            let state = &context.decoration[&a.box_id];
            if state.hidden {
                return false;
            }
            if a.style.effects.keyword(Property::BackfaceVisibility) == Keyword::Hidden
                && state.matrix.back_facing()
            {
                return false;
            }
            if let Value::Shape(Some(shape)) = a.style.effects.get(Property::ClipPath) {
                let y = y + if a.coordinate_space == CoordinateSpace::Document {
                    scroll
                } else {
                    0.0
                };
                let Some((x, y)) = state.matrix.unproject(x, y) else {
                    return false;
                };
                if !shape.contains(a.rect, a.style.font_size, x, y) {
                    return false;
                }
            }
        }
        true
    }

    pub(super) fn finalize_overflow(&mut self, viewport: Rect) {
        let map = definitions(&self.root);
        let root = self.root_element();
        let propagated = root.and_then(|root| {
            if root.tag_name == "<html>"
                && root.style.overflow_x == Overflow::Visible
                && root.style.overflow_y == Overflow::Visible
            {
                root.children.iter().find(|b| b.tag_name == "<body>")
            } else {
                Some(root)
            }
        });
        let mut viewport_modes = propagated.map_or((Overflow::Auto, Overflow::Auto), |b| {
            (b.style.overflow_x, b.style.overflow_y)
        });
        // visible on the viewport is auto; clip does not establish scrolling.
        if viewport_modes.0 == Overflow::Visible {
            viewport_modes.0 = Overflow::Auto;
        }
        if viewport_modes.1 == Overflow::Visible {
            viewport_modes.1 = Overflow::Auto;
        }
        let suppressed = propagated.map(|b| b.box_id);
        let viewport_reversals = propagated.map_or((false, false), |b| scroll_reversals(&b.style));
        self.overflow.clear();
        for b in map.values() {
            if (b.kind != BoxKind::Block && !b.atomic_inline())
                || b.box_id == self.root.box_id
                || Some(b.box_id) == suppressed
                || !(b.style.overflow_x.clips() || b.style.overflow_y.clips())
            {
                continue;
            }
            self.overflow.insert(
                b.box_id,
                ScrollContainer {
                    port: port(b),
                    extent: port(b),
                    range: ScrollRange::default(),
                    overflow_x: b.style.overflow_x,
                    overflow_y: b.style.overflow_y,
                },
            );
        }
        let mut viewport_extent = viewport.union(self.root.rect);
        let mut extents: HashMap<_, _> =
            self.overflow.iter().map(|(&id, c)| (id, c.port)).collect();
        for b in boxes(&self.root) {
            for (geometry, own) in [(Some(b.rect), false), (content_ink(b), true)] {
                let path = lineage(b, &map, own);
                let mut rect = geometry.map(|r| {
                    if own {
                        r
                    } else {
                        b.style
                            .effects
                            .matrix(b.rect, b.content_rect, b.style.font_size)
                            .bounds(b.style.effects.ink_bounds(r, b.style.font_size))
                            .map_or(r, |visual| r.union(visual))
                    }
                });
                for ancestor in path.into_iter().rev() {
                    if ancestor.box_id != b.box_id {
                        rect = rect.and_then(|r| {
                            let perspective = ancestor
                                .style
                                .effects
                                .perspective(ancestor.rect, ancestor.style.font_size);
                            if perspective == super::Mat4::IDENTITY {
                                Some(r)
                            } else {
                                perspective.bounds(r)
                            }
                        });
                    }
                    if let Some(container) = self.overflow.get(&ancestor.box_id)
                        && let Some(r) = rect
                    {
                        extents
                            .entry(ancestor.box_id)
                            .and_modify(|extent| *extent = extent.union(r));
                        rect = clip(*container, ScrollOffset::default()).apply(r);
                    }
                    rect = rect.map(|r| {
                        ancestor
                            .style
                            .effects
                            .matrix(
                                ancestor.rect,
                                ancestor.content_rect,
                                ancestor.style.font_size,
                            )
                            .bounds(
                                ancestor
                                    .style
                                    .effects
                                    .ink_bounds(r, ancestor.style.font_size),
                            )
                            .map_or(r, |visual| r.union(visual))
                    });
                }
                if b.coordinate_space == CoordinateSpace::Document
                    && let Some(r) = rect
                {
                    viewport_extent = viewport_extent.union(r);
                }
            }
        }
        for (&id, container) in &mut self.overflow {
            let b = map[&id];
            let extent = extents[&id];
            // End padding remains reachable after the last overflowing child.
            container.extent = super::tree::finite_rect(
                Rect {
                    x: extent.x
                        - if extent.x < container.port.x {
                            b.padding.left
                        } else {
                            0.0
                        },
                    y: extent.y
                        - if extent.y < container.port.y {
                            b.padding.top
                        } else {
                            0.0
                        },
                    width: extent.width
                        + if extent.x < container.port.x {
                            b.padding.left
                        } else {
                            0.0
                        }
                        + if extent.x + extent.width > container.port.x + container.port.width {
                            b.padding.right
                        } else {
                            0.0
                        },
                    height: extent.height
                        + if extent.y < container.port.y {
                            b.padding.top
                        } else {
                            0.0
                        }
                        + if extent.y + extent.height > container.port.y + container.port.height {
                            b.padding.bottom
                        } else {
                            0.0
                        },
                },
                b.node_id,
                &mut self.diagnostics,
            );
            container.range = range(
                *container,
                scroll_reversals(&b.style),
                b.node_id,
                &mut self.diagnostics,
            );
        }
        self.viewport_overflow = ScrollContainer {
            port: viewport,
            extent: super::tree::finite_rect(viewport_extent, None, &mut self.diagnostics),
            range: ScrollRange::default(),
            overflow_x: viewport_modes.0,
            overflow_y: viewport_modes.1,
        };
        self.viewport_overflow.range = range(
            self.viewport_overflow,
            viewport_reversals,
            None,
            &mut self.diagnostics,
        );
        // Keep temporarily absent nodes (display:none/conditional styles) so
        // they regain their position, but never reuse state in a new Frame.
        let offsets: Vec<_> = self
            .scroll_state
            .nodes
            .iter()
            .map(|(&id, &o)| (id, o))
            .collect();
        for (id, offset) in offsets {
            let container = map
                .values()
                .filter(|b| key(b) == Some(id))
                .find_map(|b| self.overflow.get(&b.box_id))
                .copied();
            if let Some(container) = container {
                let offset = ScrollOffset {
                    x: finite_used(
                        offset.x as f64,
                        Some(id.node),
                        "scroll x",
                        &mut self.diagnostics,
                    ),
                    y: finite_used(
                        offset.y as f64,
                        Some(id.node),
                        "scroll y",
                        &mut self.diagnostics,
                    ),
                };
                self.scroll_state
                    .nodes
                    .insert(id, clamped(offset, container.range));
            }
        }
        self.set_viewport_scroll(self.scroll_state.viewport);
        self.resnap();
    }
}

fn consume(
    old: ScrollOffset,
    container: ScrollContainer,
    delta: &mut ScrollOffset,
    node: Option<NodeId>,
    diagnostics: &mut Vec<crate::style::Diagnostic>,
) -> ScrollOffset {
    let next = clamped(
        ScrollOffset {
            x: if container.overflow_x.user_scrollable() {
                finite_used(old.x as f64 + delta.x as f64, node, "wheel x", diagnostics)
            } else {
                old.x
            },
            y: if container.overflow_y.user_scrollable() {
                finite_used(old.y as f64 + delta.y as f64, node, "wheel y", diagnostics)
            } else {
                old.y
            },
        },
        container.range,
    );
    delta.x = finite_used(
        delta.x as f64 - (next.x as f64 - old.x as f64),
        node,
        "wheel remainder x",
        diagnostics,
    );
    delta.y = finite_used(
        delta.y as f64 - (next.y as f64 - old.y as f64),
        node,
        "wheel remainder y",
        diagnostics,
    );
    next
}

fn scroll_reversals(style: &crate::style::ComputedStyle) -> (bool, bool) {
    if style.writing_mode.vertical() {
        (
            style.writing_mode.block_reverse(),
            (style.writing_mode == crate::style::WritingMode::SidewaysLr)
                ^ (style.direction == Direction::Rtl
                    && style.text_orientation != crate::style::TextOrientation::Upright),
        )
    } else {
        (style.direction == Direction::Rtl, false)
    }
}

fn range(
    container: ScrollContainer,
    reverse: (bool, bool),
    node: Option<NodeId>,
    diagnostics: &mut Vec<crate::style::Diagnostic>,
) -> ScrollRange {
    let port = container.port;
    let extent = container.extent;
    let mut range = ScrollRange::default();
    if container.overflow_x.scroll_container() && port.width > 0.0 {
        if reverse.0 {
            range.min.x = finite_used(
                (extent.x as f64 - port.x as f64).min(0.0),
                node,
                "scroll minimum x",
                diagnostics,
            );
        } else {
            range.max.x = finite_used(
                (extent.x as f64 + extent.width as f64 - port.x as f64 - port.width as f64)
                    .max(0.0),
                node,
                "scroll maximum x",
                diagnostics,
            );
        }
    }
    if container.overflow_y.scroll_container() && port.height > 0.0 {
        if reverse.1 {
            range.min.y = finite_used(
                (extent.y as f64 - port.y as f64).min(0.0),
                node,
                "scroll minimum y",
                diagnostics,
            );
        } else {
            range.max.y = finite_used(
                (extent.y as f64 + extent.height as f64 - port.y as f64 - port.height as f64)
                    .max(0.0),
                node,
                "scroll maximum y",
                diagnostics,
            );
        }
    }
    range
}
