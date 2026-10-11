use super::Direction;
use crate::dom::{Document, NodeId, NodeKind};

/// HTML's auto scan skips independent directional subtrees and script/style.
pub(super) fn auto(document: &Document, root: NodeId) -> Direction {
    fn scan(
        document: &Document,
        id: NodeId,
        root: NodeId,
        budget: &mut usize,
    ) -> Option<Direction> {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let node = document.node(id)?;
        match node.kind() {
            NodeKind::Text(text) => {
                for c in text.chars().take(*budget) {
                    *budget = budget.saturating_sub(1);
                    match unicode_bidi::bidi_class(c) {
                        unicode_bidi::BidiClass::L => return Some(Direction::Ltr),
                        unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL => {
                            return Some(Direction::Rtl);
                        }
                        _ => {}
                    }
                }
            }
            NodeKind::Element(e)
                if id != root
                    && (matches!(e.tag_name(), "bdi" | "script" | "style" | "textarea")
                        || e.attribute("dir").is_some_and(|d| {
                            ["ltr", "rtl", "auto"]
                                .iter()
                                .any(|v| d.eq_ignore_ascii_case(v))
                        })) =>
            {
                return None;
            }
            _ => {}
        }
        node.children()
            .iter()
            .find_map(|&child| scan(document, child, root, budget))
    }
    scan(document, root, root, &mut 1_048_576).unwrap_or(Direction::Ltr)
}
