//! Retained scroll requests and snap selection operate on completed geometry.
use super::*;
use crate::{
    css::values::Side,
    style::scrolling::{Align, Axis, Behavior, Property, Scrolling, Value},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    #[default]
    Auto,
    Instant,
    Smooth,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Motion {
    from: ScrollOffset,
    to: ScrollOffset,
    start: f64,
    duration: f64,
    x: bool,
    y: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Gesture {
    from: ScrollOffset,
    direction: ScrollOffset,
    pub deadline: f64,
}
#[derive(Clone, Copy)]
struct Candidate {
    node: NodeId,
    position: f32,
    interval: (f32, f32),
    always: bool,
}
impl LayoutTree {
    pub(super) fn viewport_scrollbars(&self) -> Vec<Scrollbar> {
        use crate::style::scrolling::Width;
        let Some(root) = self.root_element() else {
            return Vec::new();
        };
        let style = &root.style.scrolling;
        // Preserve the engine's undecorated viewport default. Author color or
        // thin width opts into an overlay; panels always use normal auto UI.
        if style.width() == Width::None
            || style.width() == Width::Auto && style.get(Property::Color) == Value::Color(None)
        {
            return Vec::new();
        }
        let Some(key) = key(root) else {
            return Vec::new();
        };
        let c = self.viewport_overflow;
        let r = c.port;
        let t = if style.width() == Width::Thin {
            3.0f32
        } else {
            6.0f32
        }
        .min(r.width)
        .min(r.height);
        let (thumb_color, track_color) = style.colors(root.style.text_color);
        let horizontal = c.overflow_x.user_scrollable() && c.range.max.x > c.range.min.x;
        let vertical = c.overflow_y.user_scrollable() && c.range.max.y > c.range.min.y;
        let mut bars = Vec::new();
        for (show, v) in [(horizontal, false), (vertical, true)] {
            if !show || t <= 0.0 {
                continue;
            }
            let (min, max, offset, size, length) = if v {
                (
                    c.range.min.y,
                    c.range.max.y,
                    self.viewport_scroll().y,
                    r.height,
                    r.height - if horizontal { t } else { 0.0 },
                )
            } else {
                (
                    c.range.min.x,
                    c.range.max.x,
                    self.viewport_scroll().x,
                    r.width,
                    r.width - if vertical { t } else { 0.0 },
                )
            };
            let track = if v {
                Rect {
                    x: r.x + r.width - t,
                    y: r.y,
                    width: t,
                    height: length,
                }
            } else {
                Rect {
                    x: r.x,
                    y: r.y + r.height - t,
                    width: length,
                    height: t,
                }
            };
            let thumb_length = ((length as f64 * size as f64
                / (size as f64 + max as f64 - min as f64))
                .max(12.0)
                .min(length as f64)) as f32;
            let fraction = (offset as f64 - min as f64) / (max as f64 - min as f64);
            let start = ((length - thumb_length) as f64 * fraction) as f32;
            let thumb = if v {
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
                owner: root.box_id,
                track,
                thumb,
                vertical: v,
                spatial: SpatialState::default(),
                coordinate_space: CoordinateSpace::Viewport,
                key,
                range: c.range,
                thumb_color,
                track_color,
                viewport: true,
            });
        }
        bars
    }
    pub(super) fn scroll_style(&self, node: Option<NodeId>) -> Scrolling {
        let b = if let Some(node) = node {
            boxes(&self.root)
                .into_iter()
                .find(|b| b.node_id == Some(node) && b.pseudo.is_none())
        } else {
            // Scroll-behavior and snap properties do not propagate from body.
            self.root_element()
        };
        b.map_or_else(Scrolling::default, |b| b.style.scrolling.clone())
    }
    fn request_key(node: Option<NodeId>) -> Option<ScrollKey> {
        node.map(|node| ScrollKey { node, pseudo: None })
    }
    fn current_scroll(&self, key: Option<ScrollKey>) -> ScrollOffset {
        key.map_or(self.viewport_scroll(), |k| self.offset_for(k))
    }
    fn container_for(&self, key: Option<ScrollKey>) -> Option<ScrollContainer> {
        key.map_or(Some(self.viewport_overflow), |k| {
            self.scroll_container(k.node)
        })
    }
    fn assign_scroll(&mut self, key: Option<ScrollKey>, value: ScrollOffset) -> bool {
        if let Some(key) = key {
            self.set_scroll_offset(key.node, value)
        } else {
            self.set_viewport_scroll(value)
        }
    }
    pub fn cancel_scroll_axis(&mut self, node: Option<NodeId>, x: bool, y: bool) {
        let key = Self::request_key(node);
        if let Some(m) = self.scroll_state.motions.get_mut(&key) {
            if x {
                m.x = false;
            }
            if y {
                m.y = false;
            }
            if !m.x && !m.y {
                self.scroll_state.motions.remove(&key);
            }
        }
        if x {
            self.scroll_state.snapped.remove(&(key, false));
        }
        if y {
            self.scroll_state.snapped.remove(&(key, true));
        }
    }
    pub fn cancel_scroll_animations(&mut self) {
        self.scroll_state.motions.clear();
        self.scroll_state.gestures.clear();
    }
    /// CSSOM-equivalent Rust request. None denotes the viewport. No JS binding.
    pub fn request_scroll(
        &mut self,
        node: Option<NodeId>,
        offset: ScrollOffset,
        behavior: ScrollBehavior,
    ) -> Result<bool, &'static str> {
        if !offset.x.is_finite() || !offset.y.is_finite() {
            return Err("scroll request must be finite");
        }
        let key = Self::request_key(node);
        let Some(container) = self.container_for(key) else {
            return Err("scroll request target has no scrollport");
        };
        let old = self.current_scroll(key);
        let to = self.select_snap(
            key,
            clamped(offset, container.range),
            old,
            ScrollOffset::default(),
            false,
        );
        self.start_scroll(key, to, behavior)
    }
    pub fn scroll_key(
        &mut self,
        x: f32,
        y: f32,
        delta: ScrollOffset,
    ) -> Result<bool, &'static str> {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return Err("keyboard scroll must be finite");
        }
        let saved = self.scroll_state.clone();
        self.scroll_wheel(x, y, delta);
        let mut plans = Vec::new();
        for (&key, &to) in &self.scroll_state.nodes {
            if saved.nodes.get(&key).copied().unwrap_or_default() != to {
                plans.push((Some(key), to));
            }
        }
        if self.viewport_scroll() != saved.viewport {
            plans.push((None, self.viewport_scroll()));
        }
        self.scroll_state = saved;
        let mut changed = false;
        for (key, to) in plans {
            let from = self.current_scroll(key);
            let to = self.select_snap(key, to, from, delta, true);
            changed |= self.start_scroll(key, to, ScrollBehavior::Auto)?;
        }
        Ok(changed)
    }
    fn start_scroll(
        &mut self,
        key: Option<ScrollKey>,
        to: ScrollOffset,
        behavior: ScrollBehavior,
    ) -> Result<bool, &'static str> {
        let old = self.current_scroll(key);
        self.scroll_state.gestures.remove(&key);
        self.scroll_state.motions.remove(&key);
        let smooth = behavior == ScrollBehavior::Smooth
            || behavior == ScrollBehavior::Auto
                && self.scroll_style(key.map(|k| k.node)).behavior() == Behavior::Smooth;
        if old == to {
            return Ok(false);
        }
        if !smooth {
            return Ok(self.assign_scroll(key, to));
        }
        let distance = (to.x as f64 - old.x as f64).hypot(to.y as f64 - old.y as f64);
        self.scroll_state.motions.insert(
            key,
            Motion {
                from: old,
                to,
                start: self.scroll_state.clock,
                duration: (0.18 + distance / 2000.0).clamp(0.18, 0.5),
                x: old.x != to.x,
                y: old.y != to.y,
            },
        );
        Ok(true)
    }
    pub(super) fn record_gesture(
        &mut self,
        key: Option<ScrollKey>,
        old: ScrollOffset,
        next: ScrollOffset,
    ) {
        if old == next {
            return;
        }
        self.cancel_scroll_axis(key.map(|k| k.node), old.x != next.x, old.y != next.y);
        let entry = self.scroll_state.gestures.entry(key).or_insert(Gesture {
            from: old,
            direction: ScrollOffset::default(),
            deadline: 0.0,
        });
        if old.x != next.x {
            if entry.direction.x * (next.x - old.x) < 0.0 {
                entry.from.x = old.x;
            }
            entry.direction.x = next.x - old.x;
        }
        if old.y != next.y {
            if entry.direction.y * (next.y - old.y) < 0.0 {
                entry.from.y = old.y;
            }
            entry.direction.y = next.y - old.y;
        }
        entry.deadline = self.scroll_state.clock + 0.12;
    }
    /// TouchPhase::Ended or the native mouse-wheel idle deadline.
    pub fn finish_scroll_gesture(&mut self) -> bool {
        let keys: Vec<_> = self.scroll_state.gestures.keys().copied().collect();
        let mut changed = false;
        for key in keys {
            changed |= self.finish_gesture(key);
        }
        changed
    }
    fn finish_gesture(&mut self, key: Option<ScrollKey>) -> bool {
        let Some(g) = self.scroll_state.gestures.remove(&key) else {
            return false;
        };
        let end = self.current_scroll(key);
        let snap = self.select_snap(key, end, g.from, g.direction, true);
        self.assign_scroll(key, snap)
    }
    /// Explicit time, no sleeps; sample before CSS scroll timelines and paint.
    pub fn sample_scrolling(&mut self, seconds: f64) -> Result<bool, &'static str> {
        if !crate::style::animation::valid_time(seconds) {
            return Err("invalid scroll clock");
        }
        self.scroll_state.clock = seconds;
        let due: Vec<_> = self
            .scroll_state
            .gestures
            .iter()
            .filter(|(_, g)| seconds >= g.deadline)
            .map(|(&k, _)| k)
            .collect();
        let mut changed = false;
        for k in due {
            changed |= self.finish_gesture(k);
        }
        let motions: Vec<_> = self
            .scroll_state
            .motions
            .iter()
            .map(|(&k, m)| (k, m.clone()))
            .collect();
        for (key, mut m) in motions {
            if self.container_for(key).is_none() {
                self.scroll_state.motions.remove(&key);
                continue;
            }
            for vertical in [false, true] {
                if let Some(&node) = self.scroll_state.snapped.get(&(key, vertical)) {
                    let end = if vertical { m.to.y } else { m.to.x };
                    if let Some(c) = self
                        .candidates(key, vertical, end)
                        .into_iter()
                        .find(|c| c.node == node)
                    {
                        if vertical {
                            m.to.y = c.position;
                        } else {
                            m.to.x = c.position;
                        }
                    }
                }
            }
            let t = ((seconds - m.start) / m.duration).clamp(0.0, 1.0);
            let eased = t * t * (3.0 - 2.0 * t);
            let old = self.current_scroll(key);
            let position = ScrollOffset {
                x: if m.x {
                    (m.from.x as f64 + (m.to.x as f64 - m.from.x as f64) * eased) as f32
                } else {
                    old.x
                },
                y: if m.y {
                    (m.from.y as f64 + (m.to.y as f64 - m.from.y as f64) * eased) as f32
                } else {
                    old.y
                },
            };
            changed |= self.assign_scroll(key, position);
            if t == 1.0 {
                self.scroll_state.motions.remove(&key);
            }
        }
        changed |= self.resnap();
        Ok(changed)
    }
    fn candidates(&self, key: Option<ScrollKey>, vertical: bool, end: f32) -> Vec<Candidate> {
        let Some(container) = self.container_for(key) else {
            return Vec::new();
        };
        let map = definitions(&self.root);
        let style = self.scroll_style(key.map(|k| k.node));
        let owner = key.and_then(|key| map.values().find(|b| super::key(b) == Some(key)).copied());
        let (port_start, port_size, low_side, high_side) = if vertical {
            (
                container.port.y,
                container.port.height,
                Side::Top,
                Side::Bottom,
            )
        } else {
            (
                container.port.x,
                container.port.width,
                Side::Left,
                Side::Right,
            )
        };
        let low = port_start + style.edge(low_side, true, port_size);
        let high = (port_start + port_size - style.edge(high_side, true, port_size)).max(low);
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for b in boxes(&self.root) {
            let Some(node) = b.node_id else { continue };
            if b.pseudo.is_some()
                || b.coordinate_space == CoordinateSpace::Viewport
                || !seen.insert(node)
            {
                continue;
            }
            let path = lineage(b, &map, false);
            let nearest = path
                .iter()
                .rev()
                .find(|a| {
                    self.overflow.get(&a.box_id).is_some_and(|c| {
                        c.overflow_x.scroll_container() || c.overflow_y.scroll_container()
                    })
                })
                .and_then(|a| super::key(a));
            if nearest != key {
                continue;
            }
            if path.iter().any(|a| {
                self.overflow.get(&a.box_id).is_some_and(|c| {
                    !c.overflow_x.scroll_container()
                        && !c.overflow_y.scroll_container()
                        && self
                            .unscrolled_rect(b, Some(a), &map)
                            .is_none_or(|r| clip(*c, ScrollOffset::default()).apply(r).is_none())
                })
            }) {
                continue;
            }
            let Value::Align(block, inline) = b.style.scrolling.get(Property::SnapAlign) else {
                unreachable!()
            };
            let align = if vertical
                ^ owner.map_or_else(
                    || {
                        self.root_element()
                            .is_some_and(|b| b.style.writing_mode.vertical())
                    },
                    |b| b.style.writing_mode.vertical(),
                ) {
                block
            } else {
                inline
            };
            if align == Align::None {
                continue;
            }
            let Some(rect) = self.unscrolled_rect(b, owner, &map) else {
                continue;
            };
            let start = if vertical { rect.y } else { rect.x }
                - b.style.scrolling.edge(low_side, false, 0.0);
            let stop = if vertical {
                rect.y + rect.height
            } else {
                rect.x + rect.width
            } + b.style.scrolling.edge(high_side, false, 0.0);
            let size = stop - start;
            let reverse = owner.or_else(|| self.root_element()).is_some_and(|b| {
                let (x, y) = scroll_reversals(&b.style);
                if vertical { y } else { x }
            });
            let position = match (align, reverse) {
                (Align::Start, false) | (Align::End, true) => start - low,
                (Align::End, false) | (Align::Start, true) => stop - high,
                (Align::Center, _) => (start + stop - low - high) / 2.0,
                _ => unreachable!(),
            };
            let range = if vertical {
                (container.range.min.y, container.range.max.y)
            } else {
                (container.range.min.x, container.range.max.x)
            };
            let interval = if size > high - low {
                (
                    (start - low).clamp(range.0, range.1),
                    (stop - high).clamp(range.0, range.1),
                )
            } else {
                (
                    position.clamp(range.0, range.1),
                    position.clamp(range.0, range.1),
                )
            };
            let position = if interval.0 < interval.1 {
                end.clamp(interval.0, interval.1)
            } else {
                position.clamp(range.0, range.1)
            };
            if !position.is_finite() {
                continue;
            }
            result.push(Candidate {
                node,
                position,
                interval,
                always: b.style.scrolling.get(Property::SnapStop) == Value::Stop(true),
            });
        }
        result
    }
    fn unscrolled_matrix(&self, b: &LayoutBox, map: &Definitions<'_>) -> super::super::Mat4 {
        lineage(b, map, true)
            .into_iter()
            .fold(super::super::Mat4::IDENTITY, |m, a| {
                m.then(
                    a.style
                        .effects
                        .matrix(a.rect, a.content_rect, a.style.font_size),
                )
            })
    }
    fn unscrolled_rect(
        &self,
        b: &LayoutBox,
        owner: Option<&LayoutBox>,
        map: &Definitions<'_>,
    ) -> Option<Rect> {
        let m = self.unscrolled_matrix(b, map);
        let rect = m.bounds(b.rect)?;
        if let Some(owner) = owner {
            let m = self.unscrolled_matrix(owner, map);
            let (x, y) = m.unproject(rect.x, rect.y)?;
            let (right, bottom) = m.unproject(rect.x + rect.width, rect.y + rect.height)?;
            Some(Rect {
                x: x.min(right),
                y: y.min(bottom),
                width: (right - x).abs(),
                height: (bottom - y).abs(),
            })
        } else {
            Some(rect)
        }
    }
    fn select_snap(
        &mut self,
        key: Option<ScrollKey>,
        mut end: ScrollOffset,
        from: ScrollOffset,
        direction: ScrollOffset,
        gesture: bool,
    ) -> ScrollOffset {
        let Value::SnapType(Some((axis, mandatory))) = self
            .scroll_style(key.map(|k| k.node))
            .get(Property::SnapType)
        else {
            return end;
        };
        let vertical_writing = key
            .map_or_else(
                || self.root_element(),
                |k| {
                    boxes(&self.root)
                        .into_iter()
                        .find(|b| b.node_id == Some(k.node))
                },
            )
            .is_some_and(|b| b.style.writing_mode.vertical());
        let joint = if axis == Axis::Both && (!gesture || direction.x != 0.0 && direction.y != 0.0)
        {
            let xs = self.candidates(key, false, end.x);
            let ys = self.candidates(key, true, end.y);
            xs.iter()
                .filter_map(|x| ys.iter().find(|y| y.node == x.node).map(|y| (x, y)))
                .filter(|(x, y)| {
                    !gesture
                        || (x.position - from.x) * direction.x >= 0.0
                            && (y.position - from.y) * direction.y >= 0.0
                })
                .min_by(|(ax, ay), (bx, by)| {
                    let distance = |x: &Candidate, y: &Candidate| {
                        (x.position as f64 - end.x as f64).hypot(y.position as f64 - end.y as f64)
                    };
                    distance(ax, ay).total_cmp(&distance(bx, by))
                })
                .map(|(x, _)| x.node)
        } else {
            None
        };
        for vertical in [false, true] {
            let eligible = match axis {
                Axis::Both => true,
                Axis::X => !vertical,
                Axis::Y => vertical,
                Axis::Block => vertical != vertical_writing,
                Axis::Inline => vertical == vertical_writing,
            };
            if !eligible {
                continue;
            }
            let (position, start, dir) = if vertical {
                (end.y, from.y, direction.y)
            } else {
                (end.x, from.x, direction.x)
            };
            if gesture && dir == 0.0 {
                continue;
            }
            let candidates = self.candidates(key, vertical, position);
            let distance = |c: &&Candidate| (c.position - position).abs();
            let stop = if gesture {
                candidates
                    .iter()
                    .filter(|c| {
                        c.always
                            && (c.position - start) * dir > 0.0
                            && (position - c.position) * dir >= 0.0
                    })
                    .min_by(|a, b| {
                        (a.position - start)
                            .abs()
                            .total_cmp(&(b.position - start).abs())
                    })
            } else {
                None
            };
            let chosen = stop
                .or_else(|| joint.and_then(|node| candidates.iter().find(|c| c.node == node)))
                .or_else(|| {
                    candidates
                        .iter()
                        .filter(|c| {
                            !gesture
                                || dir == 0.0
                                || (c.position - start) * dir >= 0.0
                                || position >= c.interval.0 && position <= c.interval.1
                        })
                        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                })
                .or_else(|| {
                    candidates
                        .iter()
                        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                });
            if let Some(c) = chosen {
                let port = self.container_for(key).expect("snap scrollport").port;
                let threshold = (if vertical { port.height } else { port.width }) * 0.3;
                if mandatory || stop.is_some() || (c.position - position).abs() <= threshold {
                    if vertical {
                        end.y = c.position;
                    } else {
                        end.x = c.position;
                    }
                    self.scroll_state.snapped.insert((key, vertical), c.node);
                }
            }
        }
        end
    }
    /// Preserve the previously selected DOM target across relayout, not its old
    /// pixel coordinate. Active user gestures and smooth requests take priority.
    pub(super) fn resnap(&mut self) -> bool {
        let mut changed = false;
        let active: Vec<_> = self
            .scroll_state
            .snapped
            .iter()
            .map(|(&(k, v), &n)| (k, v, n))
            .collect();
        for (key, vertical, node) in active {
            if self.scroll_state.motions.contains_key(&key)
                || self.scroll_state.gestures.contains_key(&key)
            {
                continue;
            }
            if self
                .scroll_style(key.map(|k| k.node))
                .get(Property::SnapType)
                == Value::SnapType(None)
            {
                self.scroll_state.snapped.remove(&(key, vertical));
                continue;
            }
            let mut old = self.current_scroll(key);
            let position = if vertical { old.y } else { old.x };
            if let Some(c) = self
                .candidates(key, vertical, position)
                .into_iter()
                .find(|c| c.node == node)
            {
                if vertical {
                    old.y = c.position;
                } else {
                    old.x = c.position;
                }
                changed |= self.assign_scroll(key, old);
            } else {
                self.scroll_state.snapped.remove(&(key, vertical));
                let end = self.select_snap(key, old, old, ScrollOffset::default(), false);
                changed |= self.assign_scroll(key, end);
            }
        }
        changed
    }
    /// Inside-out physical-axis reveal, shared by fragments and native clients.
    /// Transform quads use their axis-aligned bounds, not pixel hit visibility.
    pub fn scroll_into_view(
        &mut self,
        document: &crate::dom::Document,
        node: NodeId,
        behavior: ScrollBehavior,
    ) -> Result<bool, &'static str> {
        self.scroll_into_view_with_style(document, node, None, behavior)
    }
    pub(crate) fn scroll_into_view_with_style(
        &mut self,
        document: &crate::dom::Document,
        node: NodeId,
        target_style: Option<&crate::style::ComputedStyle>,
        behavior: ScrollBehavior,
    ) -> Result<bool, &'static str> {
        let map = definitions(&self.root);
        let descendants: std::collections::HashSet<_> = document
            .preorder()
            .filter(|&id| {
                let mut current = Some(id);
                while let Some(id) = current {
                    if id == node {
                        return true;
                    }
                    current = document.node(id).and_then(|n| n.parent());
                }
                false
            })
            .collect();
        let subjects: Vec<_> = boxes(&self.root)
            .into_iter()
            .filter(|b| {
                b.node_id.is_some_and(|n| descendants.contains(&n))
                    && b.pseudo.is_none()
                    && b.kind != BoxKind::Text
            })
            .collect();
        let Some(subject) = subjects
            .iter()
            .find(|b| b.node_id == Some(node))
            .or_else(|| subjects.first())
            .copied()
        else {
            return Ok(false);
        };
        let path = lineage(subject, &map, false);
        let owners: Vec<_> = path
            .into_iter()
            .rev()
            .filter(|b| {
                self.overflow.get(&b.box_id).is_some_and(|c| {
                    c.overflow_x.scroll_container() || c.overflow_y.scroll_container()
                })
            })
            .map(|b| (b.node_id, b.box_id))
            .chain(std::iter::once((None, self.root.box_id)))
            .collect();
        let subject_id = subject.box_id;
        let fallback_rect = subjects
            .iter()
            .filter_map(|b| self.unscrolled_rect(b, None, &map))
            .reduce(Rect::union);
        drop(map);
        let mut changed = false;
        // Plan against the eventual inner destination, then restore live offsets
        // and issue smooth motions simultaneously.
        let saved = self.scroll_state.clone();
        let planned = (|| {
            let mut plans = Vec::new();
            for (owner_node, owner_id) in owners {
                let map = definitions(&self.root);
                let b = map[&subject_id];
                if b.coordinate_space == CoordinateSpace::Viewport {
                    break;
                }
                let key = Self::request_key(owner_node);
                let container = self
                    .container_for(key)
                    .ok_or("fragment ancestor lost scrollport")?;
                let owner = owner_node.map(|_| map[&owner_id]);
                let state = self.spatial(b, false, &map);
                let source = b.inline_fragments.first().map_or(b.rect, |f| f.rect);
                let mut rect = if b.node_id == Some(node) {
                    state.matrix.bounds(source)
                } else {
                    boxes(&self.root)
                        .into_iter()
                        .filter(|b| {
                            b.node_id.is_some_and(|n| descendants.contains(&n))
                                && b.pseudo.is_none()
                        })
                        .filter_map(|b| self.spatial(b, false, &map).matrix.bounds(b.rect))
                        .reduce(Rect::union)
                }
                .or(fallback_rect)
                .ok_or("fragment has singular transform")?;
                if let Some(owner) = owner {
                    let state = self.spatial(owner, false, &map);
                    let (x, y) = state
                        .matrix
                        .unproject(rect.x, rect.y)
                        .ok_or("fragment scrollport has singular transform")?;
                    let (r, b) = state
                        .matrix
                        .unproject(rect.x + rect.width, rect.y + rect.height)
                        .ok_or("fragment scrollport has singular transform")?;
                    rect = Rect {
                        x: x.min(r),
                        y: y.min(b),
                        width: (r - x).abs(),
                        height: (b - y).abs(),
                    };
                } else {
                    rect.y -= self.viewport_scroll().y;
                }
                let style = self.scroll_style(owner_node);
                let target_style = target_style.unwrap_or(&b.style);
                let margin = target_style.scrolling.clone();
                let old = self.current_scroll(key);
                let reveal = |start: f32,
                              size: f32,
                              port_start: f32,
                              port_size: f32,
                              low: Side,
                              high: Side,
                              block: bool| {
                    let a = start - margin.edge(low, false, 0.0);
                    let z = start + size + margin.edge(high, false, 0.0);
                    let p = port_start + style.edge(low, true, port_size);
                    let q = port_start + port_size - style.edge(high, true, port_size);
                    if block {
                        a - p
                    } else if a < p && z > q {
                        0.0
                    } else if a < p {
                        if z - a > q - p { z - q } else { a - p }
                    } else if z > q {
                        if z - a > q - p { a - p } else { z - q }
                    } else {
                        0.0
                    }
                };
                let vertical = target_style.writing_mode.vertical();
                let mut to = ScrollOffset {
                    x: old.x
                        + reveal(
                            rect.x,
                            rect.width,
                            container.port.x,
                            container.port.width,
                            Side::Left,
                            Side::Right,
                            vertical,
                        ),
                    y: old.y
                        + reveal(
                            rect.y,
                            rect.height,
                            container.port.y,
                            container.port.height,
                            Side::Top,
                            Side::Bottom,
                            !vertical,
                        ),
                };
                if vertical && target_style.writing_mode.block_reverse() {
                    to.x = old.x + rect.x + rect.width + margin.edge(Side::Right, false, 0.0)
                        - (container.port.x + container.port.width
                            - style.edge(Side::Right, true, container.port.width));
                }
                drop(map);
                to = self.select_snap(
                    key,
                    clamped(to, container.range),
                    old,
                    ScrollOffset::default(),
                    false,
                );
                self.assign_scroll(key, to);
                plans.push((key, to));
            }
            Ok::<_, &'static str>(plans)
        })();
        let targets = self.scroll_state.snapped.clone();
        self.scroll_state = saved;
        let plans = planned?;
        self.scroll_state.snapped = targets;
        for (key, to) in plans {
            changed |= self.start_scroll(key, to, behavior)?;
        }
        Ok(changed)
    }
}
