use super::*;
use crate::{
    dom::{Document, NodeId},
    layout::{LayoutTree, Rect, ScrollContainer, ScrollOffset},
    style::{ComputedStyle, ComputedStyles, Direction as TextDirection, WritingMode},
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct Node {
    parent: Option<NodeId>,
    rect: Option<Rect>,
    container: Option<ScrollContainer>,
    scroll: ScrollOffset,
    style: ComputedStyle,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    nodes: HashMap<NodeId, Node>,
    viewport: Option<ScrollContainer>,
    scroll: ScrollOffset,
    root: Option<NodeId>,
}
impl Snapshot {
    pub(crate) fn from_layout(
        document: &Document,
        styles: &ComputedStyles,
        tree: &LayoutTree,
    ) -> Self {
        let mut result = Self {
            viewport: Some(tree.viewport_scroll_container()),
            scroll: tree.viewport_scroll(),
            ..Self::default()
        };
        for id in document.preorder() {
            let Some(style) = styles.get(id) else {
                continue;
            };
            if document.node(id).is_none_or(|n| n.as_element().is_none()) {
                continue;
            }
            if result.root.is_none() {
                result.root = Some(id)
            }
            result.nodes.insert(
                id,
                Node {
                    parent: document.node(id).and_then(|n| n.parent()),
                    rect: tree.node_rect(id),
                    container: tree.scroll_container(id),
                    scroll: tree.scroll_offset(id),
                    style: style.clone(),
                },
            );
        }
        result
    }
    fn ancestors(&self, start: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut next = Some(start);
        while let Some(id) = next {
            if out.len() >= MAX_KEYFRAMES {
                break;
            }
            out.push(id);
            next = self.nodes.get(&id).and_then(|n| n.parent);
        }
        out
    }
    fn nearest(&self, subject: NodeId, include: bool) -> Option<NodeId> {
        self.ancestors(subject)
            .into_iter()
            .skip(usize::from(!include))
            .find(|id| self.nodes.get(id).is_some_and(|n| n.container.is_some()))
    }
    fn source(&self, subject: NodeId, scroller: Scroller) -> Option<NodeId> {
        match scroller {
            Scroller::Root => None,
            Scroller::Self_ => Some(subject),
            Scroller::Nearest => self.nearest(subject, false),
        }
    }
    fn port(
        &self,
        source: Option<NodeId>,
    ) -> Option<(ScrollContainer, ScrollOffset, &ComputedStyle)> {
        if let Some(id) = source {
            let n = self.nodes.get(&id)?;
            // Overflow propagated from the root belongs to the viewport.
            if Some(id) == self.root {
                return Some((self.viewport?, self.scroll, &n.style));
            }
            Some((n.container?, n.scroll, &n.style))
        } else {
            Some((
                self.viewport?,
                self.scroll,
                &self.nodes.get(&self.root?)?.style,
            ))
        }
    }
    pub(crate) fn progress(
        &self,
        subject: NodeId,
        c: &Config,
        style: &ComputedStyle,
    ) -> Option<f64> {
        let (source, axis, view) = match &c.timeline {
            Timeline::Scroll(s, a) => (self.source(subject, *s), *a, None),
            Timeline::View(a, insets) => (
                self.nearest(subject, false),
                *a,
                Some((subject, insets.clone())),
            ),
            Timeline::Named(name) => {
                let mut found = None;
                for id in self.ancestors(subject) {
                    let n = self.nodes.get(&id)?;
                    for (p, ap, view) in [
                        (Property::ScrollName, Property::ScrollAxis, false),
                        (Property::ViewName, Property::ViewAxis, true),
                    ] {
                        let Value::Names(names) = n.style.motion.get(p) else {
                            continue;
                        };
                        if let Some(i) = names.iter().rposition(|s| s == name) {
                            let Value::Axes(axes) = n.style.motion.get(ap) else {
                                continue;
                            };
                            if view {
                                let Value::Insets(v) = n.style.motion.get(Property::ViewInset)
                                else {
                                    continue;
                                };
                                found = Some((
                                    self.nearest(id, false),
                                    axes[i % axes.len()],
                                    Some((id, v[i % v.len()].clone())),
                                ));
                            } else {
                                found = Some((Some(id), axes[i % axes.len()], None));
                            }
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                found?
            }
            _ => return None,
        };
        let (port, scroll, source_style) = self.port(source)?;
        let (x, mut reverse) = physical(axis, source_style);
        let (min, max, offset, size, start) = if x {
            (
                port.range.min.x,
                port.range.max.x,
                scroll.x,
                port.port.width,
                port.port.x,
            )
        } else {
            (
                port.range.min.y,
                port.range.max.y,
                scroll.y,
                port.port.height,
                port.port.y,
            )
        };
        if max <= min || !c.timing.active_duration().is_finite() {
            return None;
        }
        reverse |= min < 0.0 && max == 0.0;
        let (mut value, mut bounds) = if let Some((subject, insets)) = &view {
            let rect = self.nodes.get(subject)?.rect?;
            let lo = insets[0]
                .as_ref()
                .map_or(0.0, |l| l.resolve(source_style.font_size, size));
            let hi = insets[1]
                .as_ref()
                .map_or(0.0, |l| l.resolve(source_style.font_size, size));
            let near = start + lo;
            let far = start + size - hi;
            let pos = if x { rect.x } else { rect.y };
            let end = pos + if x { rect.width } else { rect.height };
            let cover = (pos - far, end - near);
            let contain = ((end - far).min(pos - near), (end - far).max(pos - near));
            let range = |r: &RangeOffset, is_end: bool| {
                let pair = match r.name {
                    RangeName::Normal | RangeName::Cover => cover,
                    RangeName::Contain => contain,
                    RangeName::Entry => (cover.0, contain.0),
                    RangeName::Exit => (contain.1, cover.1),
                    RangeName::EntryCrossing => (cover.0, end - far),
                    RangeName::ExitCrossing => (pos - near, cover.1),
                };
                let _ = is_end;
                pair.0 + r.offset.resolve(style.font_size, pair.1 - pair.0)
            };
            (
                offset,
                (range(&c.range[0], false), range(&c.range[1], true)),
            )
        } else {
            let size = max - min;
            (
                offset - min,
                (
                    c.range[0].offset.resolve(style.font_size, size),
                    c.range[1].offset.resolve(style.font_size, size),
                ),
            )
        };
        if reverse {
            if view.is_some() {
                value = -value;
                bounds = (-bounds.1, -bounds.0);
            } else {
                value = (max - min) - value;
            }
        }
        let distance = bounds.1 - bounds.0;
        (distance > 0.0 && distance.is_finite()).then(|| f64::from((value - bounds.0) / distance))
    }
}
fn physical(axis: Axis, s: &ComputedStyle) -> (bool, bool) {
    match axis {
        Axis::X => (true, false),
        Axis::Y => (false, false),
        Axis::Block => (
            s.writing_mode.vertical(),
            matches!(
                s.writing_mode,
                WritingMode::VerticalRl | WritingMode::SidewaysRl
            ),
        ),
        Axis::Inline => (
            !s.writing_mode.vertical(),
            (s.direction == TextDirection::Rtl) ^ (s.writing_mode == WritingMode::SidewaysLr),
        ),
    }
}
