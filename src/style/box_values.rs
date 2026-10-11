use super::{
    AspectRatio, Display, DisplayInside as I, DisplayOutside as O, ObjectAxis, ObjectFit,
    ObjectPosition,
};
use crate::css::{
    tokenizer::TokenKind,
    values::{Length, components::Component},
};

pub(super) fn display(tokens: &[Component]) -> Option<Display> {
    let words = tokens
        .iter()
        .map(|t| Some(t.ident()?.to_ascii_lowercase()))
        .collect::<Option<Vec<_>>>()?;
    if let [word] = words.as_slice() {
        let single = match word.as_str() {
            "contents" => Some(Display::Contents),
            "none" => Some(Display::None),
            "inline-block" => Some(Display::InlineBlock),
            "inline-flex" => Some(Display::InlineFlex),
            "inline-grid" => Some(Display::InlineGrid),
            "inline-table" => Some(Display::InlineTable),
            "table-caption" => Some(Display::TableCaption),
            "table-row-group" => Some(Display::TableRowGroup),
            "table-header-group" => Some(Display::TableHeaderGroup),
            "table-footer-group" => Some(Display::TableFooterGroup),
            "table-row" => Some(Display::TableRow),
            "table-cell" => Some(Display::TableCell),
            "table-column" => Some(Display::TableColumn),
            "table-column-group" => Some(Display::TableColumnGroup),
            _ => None,
        };
        if single.is_some() {
            return single;
        }
    }
    let (mut outer, mut inner, mut list) = (None, None, false);
    if words.is_empty() || words.len() > 3 {
        return None;
    }
    for word in words {
        match word.as_str() {
            "block" if outer.is_none() => outer = Some(O::Block),
            "inline" if outer.is_none() => outer = Some(O::Inline),
            "flow" if inner.is_none() => inner = Some(I::Flow),
            "flow-root" if inner.is_none() => inner = Some(I::FlowRoot),
            "flex" if inner.is_none() => inner = Some(I::Flex),
            "grid" if inner.is_none() => inner = Some(I::Grid),
            "table" if inner.is_none() => inner = Some(I::Table),
            "list-item" if !list => list = true,
            _ => return None,
        }
    }
    let inline = outer == Some(O::Inline);
    Some(match (inner.unwrap_or(I::Flow), inline, list) {
        (I::Flow, false, false) => Display::Block,
        (I::Flow, true, false) => Display::Inline,
        (I::FlowRoot, false, false) => Display::FlowRoot,
        (I::FlowRoot, true, false) => Display::InlineBlock,
        (I::Flow, false, true) => Display::ListItem,
        (I::Flow, true, true) => Display::InlineListItem,
        (I::FlowRoot, false, true) => Display::FlowRootListItem,
        (I::FlowRoot, true, true) => Display::InlineFlowRootListItem,
        (I::Flex, false, false) => Display::Flex,
        (I::Flex, true, false) => Display::InlineFlex,
        (I::Grid, false, false) => Display::Grid,
        (I::Grid, true, false) => Display::InlineGrid,
        (I::Table, false, false) => Display::Table,
        (I::Table, true, false) => Display::InlineTable,
        _ => return None,
    })
}

pub(super) fn ratio(tokens: &[Component]) -> Option<AspectRatio> {
    if tokens
        .iter()
        .enumerate()
        .any(|(i, t)| t.is_ident("auto") && i != 0 && i + 1 != tokens.len())
    {
        return None;
    }
    let mut parts = tokens
        .iter()
        .filter(|t| !t.is_ident("auto"))
        .collect::<Vec<_>>();
    let auto_count = tokens.iter().filter(|t| t.is_ident("auto")).count();
    if auto_count > 1 {
        return None;
    }
    let ratio = match parts.as_mut_slice() {
        [] if auto_count == 1 => None,
        [n] => Some(n.number()?),
        [a, Component::Token(TokenKind::Delim('/')), b] => {
            let (a, b) = (a.number()?, b.number()?);
            if a < 0.0 || b < 0.0 {
                return None;
            }
            if a == 0.0 || b == 0.0 {
                None
            } else {
                Some(a / b)
            }
        }
        _ => return None,
    };
    if ratio.is_some_and(|r| !r.is_finite() || r < 0.0) {
        return None;
    }
    Some(AspectRatio {
        auto: auto_count == 1,
        ratio: ratio.filter(|&r| r > 0.0),
    })
}

pub(super) fn fit(tokens: &[Component]) -> Option<ObjectFit> {
    let [t] = tokens else { return None };
    Some(match t.ident()?.to_ascii_lowercase().as_str() {
        "fill" => ObjectFit::Fill,
        "contain" => ObjectFit::Contain,
        "cover" => ObjectFit::Cover,
        "none" => ObjectFit::None,
        "scale-down" => ObjectFit::ScaleDown,
        _ => return None,
    })
}

fn axis(t: &Component) -> Option<(Option<bool>, ObjectAxis)> {
    let (vertical, end, offset) = match t.ident().unwrap_or("").to_ascii_lowercase().as_str() {
        "left" => (Some(false), false, Length::Percent(0.0)),
        "right" => (Some(false), true, Length::Percent(0.0)),
        "top" => (Some(true), false, Length::Percent(0.0)),
        "bottom" => (Some(true), true, Length::Percent(0.0)),
        "center" => (None, false, Length::Percent(50.0)),
        _ => (None, false, t.length()?),
    };
    Some((
        vertical,
        ObjectAxis {
            from_end: end,
            offset,
        },
    ))
}

pub(super) fn position(tokens: &[Component]) -> Option<ObjectPosition> {
    let center = || ObjectPosition::default().x;
    if tokens.is_empty() || tokens.len() > 4 {
        return None;
    }
    if tokens.len() <= 2 {
        let (a_axis, a) = axis(&tokens[0])?;
        let Some(second) = tokens.get(1) else {
            return Some(if a_axis == Some(true) {
                ObjectPosition { x: center(), y: a }
            } else {
                ObjectPosition { x: a, y: center() }
            });
        };
        let (b_axis, b) = axis(second)?;
        if a_axis == Some(true) {
            if b_axis == Some(true) || (b_axis.is_none() && !second.is_ident("center")) {
                return None;
            }
            return Some(ObjectPosition { x: b, y: a });
        }
        if b_axis == Some(false) {
            if a_axis == Some(false) || !tokens[0].is_ident("center") {
                return None;
            }
            return Some(ObjectPosition { x: b, y: a });
        }
        return Some(ObjectPosition { x: a, y: b });
    }
    let (mut x, mut y, mut centered, mut i) = (None, None, 0, 0);
    while i < tokens.len() {
        let (vertical, mut a) = axis(&tokens[i])?;
        if tokens[i].is_ident("center") {
            centered += 1;
            i += 1;
            continue;
        }
        let vertical = vertical?;
        i += 1;
        if !tokens[i - 1].is_ident("center")
            && let Some(offset) = tokens.get(i).and_then(Component::length)
        {
            a.offset = offset;
            i += 1;
        }
        let slot = if vertical { &mut y } else { &mut x };
        if slot.replace(a).is_some() {
            return None;
        }
    }
    if centered == 1 {
        if x.is_none() && y.is_some() {
            x = Some(center());
        } else if y.is_none() && x.is_some() {
            y = Some(center());
        } else {
            return None;
        }
    } else if centered > 1 {
        return None;
    }
    Some(ObjectPosition { x: x?, y: y? })
}
