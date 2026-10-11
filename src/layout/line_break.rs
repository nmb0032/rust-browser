//! CSS whitespace phases precede one UAX #14 pass over the logical inline
//! stream. Buffered intervals end only at legal opportunities, never at a
//! style boundary. Fitting measures each interval at most twice (tabs depend
//! on x); emission then installs the chosen line's whitespace geometry.
use unicode_linebreak::{BreakClass, BreakOpportunity, break_property, linebreaks};

use crate::style::{ComputedStyle, TextWrapMode, WhiteSpaceCollapse};

use super::{Assembly, BoxData, Token, finite_used};

fn character(token: &Token) -> Option<(usize, char)> {
    match token {
        Token::Text(id, text, _) => Some((*id, text.chars().next()?)),
        Token::Space(id, character, ..) | Token::Control(id, character) => Some((*id, *character)),
        Token::Image(id) => Some((*id, '\u{fffc}')),
        Token::Break(id) => Some((*id, '\n')),
        _ => None,
    }
}

fn collapsible(token: &Token) -> bool {
    matches!(token, Token::Space(_, _, _, false))
}

// Conservative UA segment-break heuristic, not language-dependent tailoring.
// Hangul uses word separators and is deliberately excluded.
fn han_or_kana(c: char) -> bool {
    matches!(c as u32, 0x2e80..=0x2fff | 0x3000..=0x30ff | 0x31f0..=0x31ff
        | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff
        | 0x20000..=0x2ffff | 0x30000..=0x323af)
        && c != '\u{3000}'
}

fn normalize(tokens: Vec<Token>, assembly: &mut Assembly<'_>) -> Vec<Token> {
    let mut result = Vec::new();
    for token in tokens {
        let Token::Text(id, text, _) = token else {
            result.push(token);
            continue;
        };
        let b = &assembly.boxes[id];
        let mode = b.style.white_space_collapse;
        for c in text.chars() {
            let c = if c == '\r' { ' ' } else { c };
            let token = match c {
                '\n' if mode.preserves_breaks() => Token::Break(id),
                ' ' | '\t' | '\n' => Token::Space(
                    id,
                    c,
                    if c == '\t' && mode.preserves_spaces() {
                        0.0
                    } else {
                        assembly.font.metrics(' ', b.style.font_size).advance_width
                    },
                    mode.preserves_spaces(),
                ),
                '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}' => Token::Break(id),
                c if super::nonprinting(c) => Token::Control(id, c),
                _ => Token::Text(
                    id,
                    c.to_string(),
                    finite_used(
                        assembly.font.metrics(c, b.style.font_size).advance_width as f64,
                        b.node,
                        "text advance",
                        assembly.diagnostics,
                    ),
                ),
            };
            result.push(token);
        }
    }
    let mut next = vec![None; result.len()];
    let mut solid = None;
    for (index, token) in result.iter().enumerate().rev() {
        next[index] = solid;
        if !collapsible(token)
            && let Some((_, c)) = character(token)
        {
            solid = Some(c);
        }
    }
    let mut pending = None;
    let mut previous = None;
    let mut after_segment = false;
    for index in 0..result.len() {
        if collapsible(&result[index]) {
            let (_, c) = character(&result[index]).unwrap();
            if c == '\n' {
                if pending
                    .is_some_and(|old| character(&result[old]).is_some_and(|(_, c)| c == '\n'))
                {
                    result[index] = Token::Omitted;
                    continue;
                }
                if let Some(old) = pending.take() {
                    result[old] = Token::Omitted;
                }
                let delete = previous == Some('\u{200b}')
                    || next[index] == Some('\u{200b}')
                    || previous.is_some_and(han_or_kana) && next[index].is_some_and(han_or_kana);
                if delete || after_segment {
                    result[index] = Token::Omitted;
                } else {
                    pending = Some(index);
                }
                after_segment = true;
            } else if pending.is_some() || after_segment {
                result[index] = Token::Omitted;
            } else {
                pending = Some(index);
            }
        } else if let Some((_, c)) = character(&result[index]) {
            if c == '\n' {
                if let Some(old) = pending.take() {
                    result[old] = Token::Omitted;
                }
                after_segment = true;
            } else {
                pending = None;
                after_segment = false;
            }
            previous = Some(c);
        }
    }
    result.retain(|token| !matches!(token, Token::Omitted));
    for token in &mut result {
        if let Token::Space(_, c, _, false) = token {
            *c = ' ';
        }
    }
    result
}

fn wrap_at(left: usize, right: usize, boxes: &[BoxData], block: &ComputedStyle) -> bool {
    // A boundary opportunity is governed by the nearest common ancestor;
    // opportunities created by disappearing characters use their own style.
    let (mut left, mut right) = (Some(left), Some(right));
    // Preorder identities increase down the tree, so climb the larger index.
    while let (Some(a), Some(b)) = (left, right) {
        if a == b {
            return boxes[a].style.text_wrap_mode == TextWrapMode::Wrap;
        }
        if a > b {
            left = boxes[a].parent;
        } else {
            right = boxes[b].parent;
        }
    }
    block.text_wrap_mode == TextWrapMode::Wrap
}

#[derive(Clone, Copy)]
struct Opportunity {
    end: usize,
    forced: bool,
    hyphen: Option<usize>,
}

fn opportunities(tokens: &[Token], boxes: &[BoxData], block: &ComputedStyle) -> Vec<Opportunity> {
    let mut logical = String::new();
    let mut positions = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if let Some((id, c)) = character(token) {
            logical.push(c);
            positions.push((logical.len(), index, id, c));
        }
    }
    let mut uax = linebreaks(&logical).peekable();
    let mut result = Vec::new();
    for (position, &(offset, index, id, c)) in positions.iter().enumerate() {
        while uax.peek().is_some_and(|(byte, _)| *byte < offset) {
            uax.next();
        }
        let kind = uax
            .peek()
            .filter(|(byte, _)| *byte == offset)
            .map(|(_, kind)| *kind);
        let forced = matches!(tokens[index], Token::Break(_))
            || kind == Some(BreakOpportunity::Mandatory) && offset != logical.len();
        let right = positions.get(position + 1);
        let break_spaces = matches!(tokens[index], Token::Space(_, _, _, true))
            && boxes[id].style.white_space_collapse == WhiteSpaceCollapse::BreakSpaces;
        let protected = |c| {
            matches!(
                break_property(c as u32),
                BreakClass::WordJoiner | BreakClass::ZeroWidthJoiner | BreakClass::NonBreakingGlue
            )
        };
        let atomic = right.is_some_and(|&(_, right_index, _, right_char)| {
            (matches!(tokens[index], Token::Image(_))
                || matches!(tokens[right_index], Token::Image(_)))
                && (c == '\u{00a0}' || !protected(c))
                && (right_char == '\u{00a0}' || !protected(right_char))
        });
        let break_spaces = break_spaces
            && !right.is_some_and(|&(_, _, _, c)| {
                protected(c) || break_property(c as u32) == BreakClass::CombiningMark
            });
        if !forced && kind != Some(BreakOpportunity::Allowed) && !break_spaces && !atomic {
            continue;
        }
        let wrapping = if collapsible(&tokens[index]) || matches!(c, '\u{200b}' | '\u{00ad}') {
            boxes[id].style.text_wrap_mode == TextWrapMode::Wrap
        } else {
            right.is_some_and(|&(_, _, right, _)| wrap_at(id, right, boxes, block))
        };
        if forced || wrapping && (kind == Some(BreakOpportunity::Allowed) || break_spaces || atomic)
        {
            let mut end = index + 1;
            // Closing box edges belong to the preceding line; opening edges
            // belong to the next. This also preserves collapsed space ownership.
            while matches!(tokens.get(end), Some(Token::End(_))) {
                end += 1;
            }
            result.push(Opportunity {
                end,
                forced,
                hyphen: (c == '\u{00ad}' && !forced).then_some(id),
            });
        }
    }
    if let Some(last) = result.last_mut()
        && last.end == tokens.len()
    {
        last.forced = true;
        last.hyphen = None;
    } else {
        result.push(Opportunity {
            end: tokens.len(),
            forced: true,
            hyphen: None,
        });
    }
    result
}

fn other_separator(token: &Token) -> bool {
    character(token).is_some_and(|(_, c)| {
        matches!(
            c,
            '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
    })
}

fn hanging(token: &Token, boxes: &[BoxData]) -> bool {
    let Some((id, _)) = character(token) else {
        return false;
    };
    let b = &boxes[id];
    (matches!(token, Token::Space(_, _, _, true)) || other_separator(token))
        && b.style.white_space_collapse != WhiteSpaceCollapse::BreakSpaces
        && !(b.style.white_space_collapse == WhiteSpaceCollapse::Preserve
            && b.style.text_wrap_mode == TextWrapMode::Nowrap)
}

#[derive(Clone, Copy, Default)]
struct Measure {
    x: f64,
    trimmed: f64,
    hung: f64,
    conditional: f64,
    content: bool,
}

impl Measure {
    fn add(&mut self, token: &Token, boxes: &[BoxData], tab: f64, half_ch: f64) -> f64 {
        let width = if matches!(token, Token::Space(_, '\t', _, true)) {
            let mut distance = ((self.x / tab).floor() + 1.0) * tab - self.x;
            if distance < half_ch {
                distance += tab;
            }
            distance
        } else if collapsible(token) && !self.content {
            0.0
        } else {
            token.width(boxes)
        };
        self.x += width;
        if collapsible(token)
            || character(token).is_some_and(|(id, c)| {
                c == '\u{1680}' && !boxes[id].style.white_space_collapse.preserves_spaces()
            })
        {
            self.trimmed += width;
        } else if hanging(token, boxes) {
            self.hung += width;
            if character(token).is_some_and(|(id, _)| {
                boxes[id].style.white_space_collapse == WhiteSpaceCollapse::Preserve
            }) {
                self.conditional += width;
            }
        } else if character(token).is_some_and(|(_, c)| c != '\n')
            && !matches!(token, Token::Control(..))
        {
            self.trimmed = 0.0;
            self.hung = 0.0;
            self.conditional = 0.0;
        }
        if !collapsible(token)
            && !matches!(token, Token::Control(..))
            && character(token).is_some_and(|(_, c)| c != '\n')
        {
            self.content = true;
        }
        width
    }

    fn fit(self) -> f64 {
        self.x - self.trimmed - self.hung
    }

    fn aligned(self, forced: bool, width: f64) -> f64 {
        let advance = self.x - self.trimmed;
        if forced && advance <= width {
            advance - self.hung + self.conditional
        } else {
            advance - self.hung
        }
    }
}

fn emit(
    tokens: &[Token],
    boundary: Opportunity,
    soft: bool,
    tab: f64,
    half_ch: f64,
    assembly: &mut Assembly<'_>,
) {
    let mut measure = Measure::default();
    let hyphen = if soft { boundary.hyphen } else { None };
    let hyphen_at = hyphen.and_then(|id| {
        tokens
            .iter()
            .rposition(|token| matches!(token, Token::Control(owner, '\u{00ad}') if *owner == id))
    });
    // Collapsible trailing whitespace is removed before contributing metrics
    // or decorations. Preserved hanging whitespace retains its hit geometry.
    let mut trim_from = tokens.len();
    for (index, token) in tokens.iter().enumerate().rev() {
        if matches!(
            token,
            Token::Begin(_) | Token::End(_) | Token::Control(..) | Token::Break(_)
        ) {
            continue;
        }
        if collapsible(token)
            || character(token).is_some_and(|(id, c)| {
                c == '\u{1680}'
                    && !assembly.boxes[id]
                        .style
                        .white_space_collapse
                        .preserves_spaces()
            })
        {
            trim_from = index;
        } else {
            break;
        }
    }
    for (index, token) in tokens.iter().enumerate() {
        let leading = collapsible(token) && !measure.content;
        let mut token = token.clone();
        let advance = measure.add(&token, assembly.boxes, tab, half_ch);
        if collapsible(&token) && (leading || index >= trim_from)
            || index >= trim_from && character(&token).is_some_and(|(_, c)| c == '\u{1680}')
        {
            continue;
        }
        if let Token::Space(_, _, width, _) = &mut token {
            *width = finite_used(advance, None, "whitespace advance", assembly.diagnostics);
        }
        assembly.add(token);
        if hyphen_at == Some(index) {
            let id = hyphen.unwrap();
            let width = assembly
                .font
                .metrics('-', assembly.boxes[id].style.font_size)
                .advance_width;
            assembly.add(Token::Text(id, "-".into(), width));
            measure.x += width as f64;
        }
    }
    assembly.finish(measure.aligned(!soft, assembly.width as f64));
}

pub(super) fn layout(tokens: Vec<Token>, block: &ComputedStyle, assembly: &mut Assembly<'_>) {
    let tokens = normalize(tokens, assembly);
    let opportunities = opportunities(&tokens, assembly.boxes, block);
    let tab = (assembly.font.metrics(' ', block.font_size).advance_width as f64 * 8.0).max(0.0001);
    let half_ch = assembly.font.metrics('0', block.font_size).advance_width as f64 * 0.5;
    let mut start = 0;
    let mut measured_to = 0;
    let mut measure = Measure::default();
    let mut previous = None;
    for boundary in opportunities {
        let before = measure;
        for token in &tokens[measured_to..boundary.end] {
            measure.add(token, assembly.boxes, tab, half_ch);
        }
        let hyphen = boundary.hyphen.map_or(0.0, |id| {
            assembly
                .font
                .metrics('-', assembly.boxes[id].style.font_size)
                .advance_width as f64
        });
        if measure.fit() + hyphen > assembly.width as f64
            && let Some(last) = previous
            && before.content
        {
            emit(
                &tokens[start..measured_to],
                last,
                true,
                tab,
                half_ch,
                assembly,
            );
            start = measured_to;
            measure = Measure::default();
            for token in &tokens[start..boundary.end] {
                measure.add(token, assembly.boxes, tab, half_ch);
            }
        }
        measured_to = boundary.end;
        previous = Some(boundary);
        if boundary.forced {
            emit(
                &tokens[start..boundary.end],
                boundary,
                false,
                tab,
                half_ch,
                assembly,
            );
            start = boundary.end;
            measure = Measure::default();
            previous = None;
        }
    }
}
