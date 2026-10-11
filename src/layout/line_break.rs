//! CSS whitespace phases precede one UAX #14 pass over the logical inline
//! stream. Buffered intervals end only at legal opportunities, never at a
//! style boundary. Fitting measures each interval at most twice (tabs depend
//! on x); emission then installs the chosen line's whitespace geometry.
use unicode_linebreak::{BreakClass, BreakOpportunity, break_property, linebreaks};
use unicode_segmentation::UnicodeSegmentation;

use crate::style::{ComputedStyle, TextWrapMode, WhiteSpaceCollapse};

use super::{Assembly, BoxData, Token, finite_used};

fn character(token: &Token) -> Option<(usize, char)> {
    match token {
        Token::Text(id, text, _) | Token::Glyphs(id, text, _) => Some((*id, text.chars().next()?)),
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
                        assembly
                            .font
                            .select(&b.style)
                            .metrics(' ', b.style.font_size)
                            .advance_width
                    },
                    mode.preserves_spaces(),
                ),
                '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}' => Token::Break(id),
                c if super::nonprinting(c) => Token::Control(id, c),
                _ => Token::Text(id, c.to_string(), 0.0),
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

fn same_font(a: &ComputedStyle, b: &ComputedStyle) -> bool {
    a.font_size == b.font_size
        && a.font_family == b.font_family
        && a.font_weight == b.font_weight
        && a.font_style == b.font_style
        && a.font_language == b.font_language
}

fn shape_tokens(tokens: &mut [Token], assembly: &mut Assembly<'_>) {
    let mut logical = String::new();
    let mut offsets = Vec::with_capacity(tokens.len());
    for token in tokens.iter() {
        offsets.push(logical.len());
        if logical.len() <= crate::text::MAX_TEXT_BYTES {
            if let Some((_, c)) = character(token) {
                logical.push(c);
            } else if token.width(assembly.boxes) != 0.0 {
                logical.push('\u{fffc}');
            }
        }
    }
    let context_bounded = logical.len() <= crate::text::MAX_TEXT_BYTES;
    let graphemes: Vec<_> = logical
        .grapheme_indices(true)
        .map(|(i, g)| (i, i + g.len()))
        .collect();
    let mut index = 0;
    while index < tokens.len() {
        let Some((owner, _)) = character(&tokens[index]) else {
            index += 1;
            continue;
        };
        if !matches!(tokens[index], Token::Text(..) | Token::Control(..)) {
            index += 1;
            continue;
        }
        let start = index;
        let mut source = String::new();
        let mut positions = Vec::new();
        let mut paint_boundaries = Vec::new();
        let mut paint_owner = owner;
        let mut oversized = false;
        while index < tokens.len() {
            let offset = offsets[index];
            let cluster = graphemes.get(graphemes.partition_point(|&(_, end)| end <= offset));
            let continues_cluster = context_bounded
                && cluster.is_some_and(|&(begin, _)| begin < offset && begin >= offsets[start]);
            match &tokens[index] {
                Token::Text(id, text, _)
                    if continues_cluster
                        || same_font(&assembly.boxes[owner].style, &assembly.boxes[*id].style) =>
                {
                    if assembly.boxes[paint_owner].style.text_fill_color
                        != assembly.boxes[*id].style.text_fill_color
                    {
                        paint_boundaries.push(source.len());
                    }
                    paint_owner = *id;
                    if source.len() + text.len() > crate::text::MAX_TEXT_BYTES {
                        oversized = true;
                    }
                    if !oversized {
                        positions.push((index, source.len(), *id));
                        source.push_str(text);
                    }
                }
                Token::Control(id, c)
                    if continues_cluster
                        || same_font(&assembly.boxes[owner].style, &assembly.boxes[*id].style) =>
                {
                    if source.len() + c.len_utf8() > crate::text::MAX_TEXT_BYTES {
                        oversized = true;
                    }
                    if !oversized {
                        positions.push((index, source.len(), *id));
                        source.push(*c);
                    }
                }
                Token::Begin(_) | Token::End(_) if tokens[index].width(assembly.boxes) == 0.0 => {}
                _ => break,
            }
            index += 1;
        }
        if index == start {
            index += 1;
            continue;
        }
        if oversized {
            assembly.diagnostics.push(crate::style::Diagnostic {
                node_id: assembly.boxes[owner].node,
                message: format!(
                    "text exceeds {}-byte shaping context limit; run omitted",
                    crate::text::MAX_TEXT_BYTES
                ),
            });
            for token in &mut tokens[start..index] {
                if matches!(token, Token::Text(..) | Token::Control(..)) {
                    *token = Token::Omitted;
                }
            }
            continue;
        }
        let (before, after) = if context_bounded {
            (
                &logical[..offsets[start]],
                &logical[offsets.get(index).copied().unwrap_or(logical.len())..],
            )
        } else {
            ("", "")
        };
        let run = assembly.font.shape_with_context(
            &assembly.boxes[owner].style,
            &source,
            &paint_boundaries,
            before,
            after,
        );
        let mut widths = vec![0.0; positions.len()];
        let mut glyphs = vec![Vec::new(); positions.len()];
        let mut cluster_advance = vec![0.0; positions.len()];
        let mut cluster_end = vec![0; positions.len()];
        let mut unsafe_after = vec![false; positions.len()];
        for glyph in &run.glyphs {
            let first = positions.partition_point(|p| p.1 < glyph.cluster);
            if first >= positions.len() {
                continue;
            }
            cluster_advance[first] += glyph.advance;
            cluster_end[first] = positions.partition_point(|p| p.1 < glyph.cluster_end);
            glyphs[first].push(glyph.clone());
        }
        for first in 0..positions.len() {
            if cluster_end[first] == 0 {
                continue;
            }
            unsafe_after[first..cluster_end[first].saturating_sub(1)].fill(true);
            // Distribute a cross-node ligature's advance to its DOM owners. Ink
            // is emitted only once, by its cluster starter; hit surfaces retain
            // the later owner's share. Marks in one owner keep zero advance.
            let mut owners: Vec<usize> = Vec::new();
            for member in first..cluster_end[first] {
                let c = character(&tokens[positions[member].0]).map(|(_, c)| c);
                let advancing = c.is_some_and(|c| {
                    !super::nonprinting(c)
                        && glyphs[first]
                            .first()
                            .is_some_and(|g| g.face.metrics(c, g.size).advance_width != 0.0)
                });
                if advancing
                    && owners
                        .last()
                        .is_none_or(|&i| positions[i].2 != positions[member].2)
                {
                    owners.push(member);
                }
            }
            let count = owners.len().max(1) as f32;
            for &owner in &owners {
                widths[owner] += cluster_advance[first] / count;
            }
            let multiple_colors = owners.windows(2).any(|pair| {
                assembly.boxes[positions[pair[0]].2].style.text_fill_color
                    != assembly.boxes[positions[pair[1]].2].style.text_fill_color
            });
            let original = std::mem::take(&mut glyphs[first]);
            let cluster_pen = original
                .iter()
                .map(|g| g.x - g.x_offset)
                .min_by(f32::total_cmp)
                .unwrap_or(0.0);
            let mut mark_owner = first + 1;
            for glyph in original {
                if glyph.advance == 0.0
                    && glyph.face.metrics_indexed(glyph.id, glyph.size).width != 0
                {
                    while mark_owner < cluster_end[first]
                        && character(&tokens[positions[mark_owner].0]).is_none_or(|(_, c)| {
                            super::nonprinting(c)
                                || glyph.face.metrics(c, glyph.size).advance_width != 0.0
                        })
                    {
                        mark_owner += 1;
                    }
                    if mark_owner < cluster_end[first] {
                        glyphs[mark_owner].push(glyph);
                        mark_owner += 1;
                        continue;
                    }
                }
                if multiple_colors {
                    let pen = cluster_pen;
                    for (part, &owner) in owners.iter().enumerate() {
                        let visual_part = if glyph.rtl {
                            owners.len() - part - 1
                        } else {
                            part
                        };
                        let mut copy = glyph.clone();
                        copy.clip = Some((
                            if visual_part == 0 {
                                f32::NEG_INFINITY
                            } else {
                                pen + cluster_advance[first] * visual_part as f32 / count - glyph.x
                            },
                            if visual_part + 1 == owners.len() {
                                f32::INFINITY
                            } else {
                                pen + cluster_advance[first] * (visual_part + 1) as f32 / count
                                    - glyph.x
                            },
                        ));
                        glyphs[owner].push(copy);
                    }
                } else {
                    glyphs[first].push(glyph);
                }
            }
        }
        let mut logical_x = 0.0;
        for (position, &(token_index, offset, id)) in positions.iter().enumerate() {
            let end = positions.get(position + 1).map_or(source.len(), |p| p.1);
            let mut part = crate::text::ShapedText {
                width: widths[position],
                ascent: run.ascent,
                descent: run.descent,
                unsafe_after: unsafe_after[position],
                line_extents: run.line_extents,
                ..crate::text::ShapedText::default()
            };
            part.glyphs = std::mem::take(&mut glyphs[position])
                .into_iter()
                .map(|mut g| {
                    g.x -= logical_x;
                    g
                })
                .collect();
            let text: String = source[offset..end]
                .chars()
                .filter(|&c| !matches!(c, '\u{00ad}' | '\u{200b}'))
                .collect();
            if matches!(
                tokens[token_index],
                Token::Control(_, '\u{00ad}' | '\u{200b}')
            ) {
                // Keep discretionary opportunities as controls, but their
                // presence has already influenced the shaping buffer.
            } else {
                tokens[token_index] = Token::Glyphs(id, text, std::sync::Arc::new(part));
            }
            logical_x += widths[position];
        }
    }
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
    context_break: bool,
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
                context_break: !forced && matches!(c, '\u{00ad}' | '\u{200b}'),
            });
        }
    }
    if let Some(last) = result.last_mut()
        && last.end == tokens.len()
    {
        last.forced = true;
        last.hyphen = None;
        last.context_break = false;
    } else {
        result.push(Opportunity {
            end: tokens.len(),
            forced: true,
            hyphen: None,
            context_break: false,
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
    let mut tokens = tokens.to_vec();
    let hyphen_at = if soft {
        boundary.hyphen.and_then(|id| {
            tokens.iter().rposition(
                |token| matches!(token, Token::Control(owner, '\u{00ad}') if *owner == id),
            )
        })
    } else {
        None
    };
    reshape(&mut tokens, hyphen_at, assembly);
    let mut measure = Measure::default();
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
    }
    assembly.finish(measure.aligned(!soft, assembly.width as f64));
}

fn reshape(tokens: &mut [Token], hyphen_at: Option<usize>, assembly: &mut Assembly<'_>) {
    for (index, token) in tokens.iter_mut().enumerate() {
        if let Token::Glyphs(id, text, _) = token {
            *token = Token::Text(*id, text.clone(), 0.0);
        } else if hyphen_at == Some(index)
            && let Token::Control(id, _) = token
        {
            *token = Token::Text(*id, "-".into(), 0.0);
        }
    }
    shape_tokens(tokens, assembly);
}

pub(super) fn layout(tokens: Vec<Token>, block: &ComputedStyle, assembly: &mut Assembly<'_>) {
    let mut tokens = normalize(tokens, assembly);
    let opportunities = opportunities(&tokens, assembly.boxes, block);
    shape_tokens(&mut tokens, assembly);
    let face = assembly.font.select(block);
    let tab = (face.metrics(' ', block.font_size).advance_width as f64 * 8.0).max(0.0001);
    let half_ch = face.metrics('0', block.font_size).advance_width as f64 * 0.5;
    let mut start = 0;
    let mut measured_to = 0;
    let mut measure = Measure::default();
    let mut previous = None;
    let mut discretionary_work = 0usize;
    for boundary in opportunities {
        let left = tokens[..boundary.end]
            .iter()
            .rev()
            .find(|t| character(t).is_some());
        if !boundary.forced
            && boundary.hyphen.is_none()
            && matches!(left, Some(Token::Glyphs(_, _, run)) if run.unsafe_after)
        {
            continue;
        }
        if boundary.context_break {
            discretionary_work += boundary.end - start;
            if discretionary_work > 1_048_576 {
                if !assembly
                    .diagnostics
                    .iter()
                    .any(|d| d.message.starts_with("discretionary shaping work limit"))
                {
                    assembly.diagnostics.push(crate::style::Diagnostic {
                        node_id: left.and_then(character).and_then(|(id, _)| assembly.boxes[id].node),
                        message:
                            "discretionary shaping work limit reached; further discretionary breaks ignored"
                                .into(),
                    });
                }
                continue;
            }
        }
        let before = measure;
        for token in &tokens[measured_to..boundary.end] {
            measure.add(token, assembly.boxes, tab, half_ch);
        }
        let candidate_fit = if boundary.context_break {
            let mut candidate = tokens[start..boundary.end].to_vec();
            let hyphen_at = boundary.hyphen.and_then(|id| {
                candidate
                    .iter()
                    .rposition(|t| matches!(t, Token::Control(owner, '\u{00ad}') if *owner == id))
            });
            reshape(&mut candidate, hyphen_at, assembly);
            let mut fitted = Measure::default();
            for token in &candidate {
                fitted.add(token, assembly.boxes, tab, half_ch);
            }
            fitted.fit()
        } else {
            measure.fit()
        };
        if candidate_fit > assembly.width as f64
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
            // A selected discretionary break changes joining/ligature context
            // on both sides; the remaining word must use its new initial form.
            if last.context_break {
                let end = tokens[start..]
                    .iter()
                    .position(|t| matches!(t, Token::Space(..) | Token::Break(_) | Token::Image(_)))
                    .map_or(tokens.len(), |i| start + i);
                reshape(&mut tokens[start..end], None, assembly);
            }
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
