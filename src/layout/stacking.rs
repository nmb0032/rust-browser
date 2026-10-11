//! CSS2 Appendix E for the engine's horizontal block/inline formatting model.
//! Positioned auto boxes form paint groups, not stacking contexts: their
//! positioned descendants escape to the nearest real context.
use std::collections::HashMap;

use crate::style::{Position, ZIndex};

use super::{BoxKind, LayoutBox, LayoutTree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintPhase {
    Decoration,
    InlineDecoration(usize),
    Content,
    Text(usize),
}

struct Group<'a> {
    owner: usize,
    level: Option<i32>,
    parent: usize,
    order: usize,
    boxes: Vec<(&'a LayoutBox, usize)>,
}

pub(crate) fn paint_order(tree: &LayoutTree) -> Vec<(&LayoutBox, PaintPhase)> {
    fn flatten<'a>(b: &'a LayoutBox, flow: usize, boxes: &mut Vec<(&'a LayoutBox, usize)>) {
        let flow = if b.kind == BoxKind::Block {
            boxes.len()
        } else {
            flow
        };
        boxes.push((b, flow));
        for child in &b.children {
            flatten(child, flow, boxes);
        }
    }
    let mut boxes = Vec::new();
    flatten(tree.root(), 0, &mut boxes);
    let root = tree.root().box_id;
    let root_element = tree.root_element().map(|b| b.box_id);
    let definitions: HashMap<_, _> = boxes.iter().map(|(b, _)| (b.box_id, *b)).collect();
    let mut groups = HashMap::new();
    groups.insert(
        root,
        Group {
            owner: root,
            level: Some(0),
            parent: root,
            order: 0,
            boxes: Vec::new(),
        },
    );
    let group_id = |b: &LayoutBox| {
        let mut candidate = Some(b.box_id);
        while let Some(id) = candidate {
            let ancestor = definitions[&id];
            if Some(id) == root_element || id == root {
                return root;
            }
            if ancestor.style.position != Position::Static {
                return id;
            }
            candidate = ancestor.parent_box;
        }
        root
    };
    for (order, (b, flow)) in boxes.iter().enumerate() {
        let id = group_id(b);
        if id != root && !groups.contains_key(&id) {
            let owner = definitions[&id];
            let mut parent = owner.parent_box;
            let mut context = root;
            while let Some(id) = parent {
                let ancestor = definitions[&id];
                if Some(id) == root_element || id == root {
                    break;
                }
                if ancestor.style.position != Position::Static
                    && matches!(ancestor.style.z_index, ZIndex::Integer(_))
                {
                    context = id;
                    break;
                }
                parent = ancestor.parent_box;
            }
            let level = match owner.style.z_index {
                ZIndex::Auto => None,
                ZIndex::Integer(z) => Some(z),
            };
            groups.insert(
                id,
                Group {
                    owner: id,
                    level,
                    parent: context,
                    order,
                    boxes: Vec::new(),
                },
            );
        }
        groups.get_mut(&id).unwrap().boxes.push((b, *flow));
    }
    fn paint<'a>(
        id: usize,
        root_element: Option<usize>,
        groups: &HashMap<usize, Group<'a>>,
        list: &mut Vec<(&'a LayoutBox, PaintPhase)>,
    ) {
        let group = &groups[&id];
        let own = |b: &LayoutBox| {
            b.box_id == group.owner || id == group.parent && Some(b.box_id) == root_element
        };
        for (b, _) in &group.boxes {
            if own(b) {
                list.push((b, PaintPhase::Decoration));
            }
        }
        let mut children: Vec<_> = groups
            .iter()
            .filter(|(child, g)| **child != id && g.parent == id)
            .collect();
        children.sort_by_key(|(_, g)| (g.level.unwrap_or(0), g.order));
        for (child, _) in children
            .iter()
            .filter(|(_, g)| g.level.is_some_and(|z| z < 0))
        {
            paint(**child, root_element, groups, list);
        }
        // Block backgrounds/borders precede ALL in-flow inline foreground,
        // including text in an earlier sibling that overflows a later block.
        for (b, _) in &group.boxes {
            if !own(b) && b.kind == BoxKind::Block {
                list.push((b, PaintPhase::Decoration));
            }
        }
        // Preserve line identity before visual offsets: sorting coordinates
        // would reverse overlapping lines or relatively moved inline fragments.
        // Within each source line, decorations and foreground interleave.
        let mut foreground = Vec::new();
        for (order, (b, flow)) in group.boxes.iter().enumerate() {
            if b.kind == BoxKind::Block {
                if b.image.is_some() || b.list_marker.is_some() || !b.text_fragments.is_empty() {
                    foreground.push((*flow, 0, order, 0, *b, PaintPhase::Content));
                }
            } else if b.image.is_some() {
                let line = b.inline_line.unwrap_or(0);
                if !own(b) {
                    foreground.push((*flow, line, order, 0, *b, PaintPhase::Decoration));
                }
                foreground.push((*flow, line, order, 1, *b, PaintPhase::Content));
            } else {
                if !own(b) {
                    for (index, fragment) in b.inline_fragments.iter().enumerate() {
                        foreground.push((
                            *flow,
                            fragment.line_index,
                            order,
                            0,
                            *b,
                            PaintPhase::InlineDecoration(index),
                        ));
                    }
                }
                for (index, fragment) in b.text_fragments.iter().enumerate() {
                    foreground.push((
                        *flow,
                        fragment.line_index,
                        order,
                        1,
                        *b,
                        PaintPhase::Text(index),
                    ));
                }
            }
        }
        foreground.sort_by_key(|(flow, line, order, phase, _, _)| (*flow, *line, *order, *phase));
        for (_, _, _, _, b, phase) in foreground {
            list.push((b, phase));
        }
        for (child, _) in children.iter().filter(|(_, g)| g.level.unwrap_or(0) >= 0) {
            paint(**child, root_element, groups, list);
        }
    }
    let mut list = Vec::new();
    paint(root, root_element, &groups, &mut list);
    list
}
