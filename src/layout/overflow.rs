//! Overflow is spatial ancestry, not paint-group ancestry. Each flattened
//! phase receives a complete clip/translation so escaped z contexts retain
//! their containing-block clips without accidentally clipping fixed boxes.
use std::collections::HashMap;

use crate::{
    dom::NodeId,
    style::{Direction, Overflow, Position},
};

use super::{BoxKind, CoordinateSpace, LayoutBox, LayoutTree, PaintPhase, Rect, tree::finite_used};

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
}

impl ScrollState {
    pub(crate) fn same_document(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.identity, &other.identity)
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpatialState {
    /// Translation from layout to visual coordinates, before viewport Y scroll.
    pub offset: ScrollOffset,
    pub clip: ClipRect,
    pub matrix: super::Mat4,
    pub plane_clips: [Option<PlaneClip>; 32],
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneClip {
    pub matrix: super::Mat4,
    pub clip: ClipRect,
}

impl SpatialState {
    pub fn contains(self, x: f32, y: f32) -> bool {
        !self.hidden
            && self.clip.contains(x, y)
            && self.plane_clips.iter().flatten().all(|c| {
                c.matrix
                    .unproject(x, y)
                    .is_some_and(|(x, y)| c.clip.contains(x, y))
            })
    }
    pub fn point(self, x: f32, y: f32) -> Option<(f32, f32)> {
        if !self.contains(x, y) {
            return None;
        }
        self.matrix.unproject(x, y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scrollbar {
    pub(crate) owner: usize,
    pub track: Rect,
    pub thumb: Rect,
    pub vertical: bool,
    pub spatial: SpatialState,
    pub coordinate_space: CoordinateSpace,
    key: ScrollKey,
    range: ScrollRange,
}

impl Scrollbar {
    pub fn node_id(self) -> NodeId {
        self.key.node
    }

    fn point(self, x: f32, y: f32, root_scroll: f32) -> (f32, f32) {
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

    fn contains(self, x: f32, y: f32, root_scroll: f32) -> bool {
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

type Definitions<'a> = HashMap<usize, &'a LayoutBox>;

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
    let mut map = HashMap::new();
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
    /// Basic overlay tracks: no reserved gutter or CSS scrollbar styling.
    pub fn scrollbars(&self) -> Vec<Scrollbar> {
        if !self
            .overflow
            .values()
            .any(|c| c.overflow_x.user_scrollable() || c.overflow_y.user_scrollable())
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
            let thickness = 6.0f32.min(r.width).min(r.height);
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
                    spatial: decoration[&b.box_id],
                    coordinate_space: b.coordinate_space,
                    key,
                    range: container.range,
                });
            }
        }
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
        let bars = self.scrollbars();
        if bars.is_empty() {
            return None;
        }
        let decoration = self.spatial_states(false);
        let content = self.spatial_states(true);
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
                        && self.effect_hit(b, x, y, root_scroll)
                })
            {
                return Some(*bar);
            }
            let state = if matches!(phase, PaintPhase::Content | PaintPhase::Text(_)) {
                content[&b.box_id]
            } else {
                decoration[&b.box_id]
            };
            if b.node_id.is_some() && self.phase_hit(b, phase, state, x, y, root_scroll) {
                return None;
            }
        }
        None
    }

    /// Distance into the thumb, or center the thumb for a track click.
    pub fn scrollbar_grab(&self, bar: Scrollbar, x: f32, y: f32) -> f32 {
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

    pub fn drag_scrollbar(&mut self, bar: Scrollbar, x: f32, y: f32, grab: f32) -> bool {
        let Some(bar) = self
            .scrollbars()
            .into_iter()
            .find(|current| current.key == bar.key && current.vertical == bar.vertical)
        else {
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
        let old = self.offset_for(bar.key);
        let next = if bar.vertical {
            ScrollOffset { y: position, ..old }
        } else {
            ScrollOffset { x: position, ..old }
        };
        self.scroll_state.nodes.insert(bar.key, next);
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

    pub(crate) fn spatial_states(&self, own: bool) -> HashMap<usize, SpatialState> {
        let map = definitions(&self.root);
        map.values()
            .map(|b| (b.box_id, self.spatial(b, own, &map)))
            .collect()
    }

    fn spatial(&self, b: &LayoutBox, own: bool, map: &Definitions<'_>) -> SpatialState {
        self.spatial_metrics(b, own, map, &mut Vec::new())
    }

    fn spatial_metrics(
        &self,
        b: &LayoutBox,
        own: bool,
        map: &Definitions<'_>,
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
        let mut dx = if b.coordinate_space == CoordinateSpace::Document {
            -self.scroll_state.viewport.x as f64
        } else {
            0.0
        };
        let mut dy = 0.0f64;
        for ancestor in lineage(b, map, own) {
            if let Some(container) = self.overflow.get(&ancestor.box_id) {
                let r = container.port;
                state.clip = state.clip.intersect(ClipRect {
                    x: container.overflow_x.clips().then(|| {
                        (
                            metric(r.x as f64 + dx, "scroll clip left"),
                            metric(r.x as f64 + r.width as f64 + dx, "scroll clip right"),
                        )
                    }),
                    y: container.overflow_y.clips().then(|| {
                        (
                            metric(r.y as f64 + dy, "scroll clip top"),
                            metric(r.y as f64 + r.height as f64 + dy, "scroll clip bottom"),
                        )
                    }),
                });
                let offset =
                    key(ancestor).map_or(ScrollOffset::default(), |id| self.offset_for(id));
                dx -= offset.x as f64;
                dy -= offset.y as f64;
            }
        }
        state.offset.x = metric(dx, "scroll translation x");
        state.offset.y = metric(dy, "scroll translation y");
        state.matrix = super::Mat4::translation(dx, dy, 0.0);
        let path = lineage(b, map, true);
        if path.iter().any(|a| a.style.effects.containing_block()) {
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
            let mut count = 0;
            for ancestor in path {
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
                    if count == state.plane_clips.len() {
                        state.hidden = true;
                        metric(
                            f64::INFINITY,
                            "effects transformed clip ancestry exceeds 32",
                        );
                        break;
                    }
                    state.plane_clips[count] = Some(PlaneClip {
                        matrix,
                        clip: clip(*container, ScrollOffset::default()),
                    });
                    count += 1;
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
        if reports.len() != before {
            state.clip = ClipRect {
                x: Some((0.0, 0.0)),
                y: Some((0.0, 0.0)),
            };
        }
        state
    }

    fn report_spatial_failures(&mut self) {
        let map = definitions(&self.root);
        let mut reports = Vec::new();
        for b in map.values() {
            for own in [false, true] {
                self.spatial_metrics(b, own, &map, &mut reports);
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
        let map = definitions(&self.root);
        let decoration = self.spatial_states(false);
        let content = self.spatial_states(true);
        let target = self
            .scrollbar_at(x, y)
            .map(|bar| map[&bar.owner])
            .or_else(|| {
                super::paint_order(self)
                    .into_iter()
                    .rev()
                    .find_map(|(b, phase)| {
                        let state = if matches!(phase, PaintPhase::Content | PaintPhase::Text(_)) {
                            content[&b.box_id]
                        } else {
                            decoration[&b.box_id]
                        };
                        self.phase_hit(b, phase, state, x, y, self.viewport_scroll().y)
                            .then_some(b)
                    })
            });
        let nodes: Vec<_> = target
            .map(|b| lineage(b, &map, true))
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
        }

        let old = self.viewport_scroll();
        let next = consume(
            old,
            self.viewport_overflow,
            &mut delta,
            None,
            &mut self.diagnostics,
        );
        changed |= old != next;
        self.scroll_state.viewport = next;
        self.report_spatial_failures();
        changed
    }

    pub(super) fn effect_hit(&self, b: &LayoutBox, x: f32, y: f32, scroll: f32) -> bool {
        use crate::style::effects::{Keyword, Property, Value};
        let map = definitions(&self.root);
        let mut candidate = Some(b.box_id);
        while let Some(a) = candidate.and_then(|id| map.get(&id)) {
            candidate = a.parent_box;
            let state = self.spatial(a, false, &map);
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
                            ancestor
                                .style
                                .effects
                                .perspective(ancestor.rect, ancestor.style.font_size)
                                .bounds(r)
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
