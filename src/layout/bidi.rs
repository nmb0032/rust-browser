use super::inline::{BoxData, Token};
use crate::style::{ComputedStyle, Direction, UnicodeBidi};
use unicode_bidi::{BidiInfo, Level};

pub(super) struct Paragraph {
    pub text: String,
    pub offsets: Vec<usize>,
    pub base: Option<Level>,
}

pub(super) fn controls(style: &ComputedStyle) -> (&'static str, &'static str) {
    let rtl = style.direction == Direction::Rtl;
    match style.unicode_bidi {
        UnicodeBidi::Normal => ("", ""),
        UnicodeBidi::Embed => (if rtl { "\u{202b}" } else { "\u{202a}" }, "\u{202c}"),
        UnicodeBidi::Isolate => (if rtl { "\u{2067}" } else { "\u{2066}" }, "\u{2069}"),
        UnicodeBidi::BidiOverride => (if rtl { "\u{202e}" } else { "\u{202d}" }, "\u{202c}"),
        UnicodeBidi::IsolateOverride => (
            if rtl {
                "\u{2068}\u{202e}"
            } else {
                "\u{2068}\u{202d}"
            },
            "\u{202c}\u{2069}",
        ),
        UnicodeBidi::Plaintext => ("\u{2068}", "\u{2069}"),
    }
}

impl Paragraph {
    pub fn new(tokens: &[Token], boxes: &[BoxData], block: &ComputedStyle) -> Self {
        let mut text = String::new();
        let override_control = if matches!(
            block.unicode_bidi,
            UnicodeBidi::BidiOverride | UnicodeBidi::IsolateOverride
        ) {
            if block.direction == Direction::Rtl {
                "\u{202e}"
            } else {
                "\u{202d}"
            }
        } else {
            ""
        };
        text.push_str(override_control);
        let mut offsets = Vec::with_capacity(tokens.len() + 1);
        let mut active = Vec::new();
        for token in tokens {
            offsets.push(text.len());
            match token {
                Token::Begin(id) => {
                    text.push_str(controls(&boxes[*id].style).0);
                    active.push(*id);
                }
                Token::End(id) => {
                    text.push_str(controls(&boxes[*id].style).1);
                    if active.last() == Some(id) {
                        active.pop();
                    }
                }
                Token::Text(id, s, _, _) | Token::Glyphs(id, s, _) => {
                    if boxes[*id].style.writing_mode.vertical()
                        && boxes[*id].style.text_orientation
                            == crate::style::TextOrientation::Upright
                    {
                        for c in s.chars() {
                            text.push(if crate::text::invisible(c) { c } else { 'A' });
                        }
                    } else {
                        text.push_str(s);
                    }
                }
                Token::Space(_, c, ..) | Token::Control(_, c) => text.push(*c),
                Token::Image(_) => text.push('\u{fffc}'),
                Token::Break(_) => {
                    for &id in active.iter().rev() {
                        text.push_str(controls(&boxes[id].style).1);
                    }
                    if !override_control.is_empty() {
                        text.push('\u{202c}');
                    }
                    text.push('\n');
                    text.push_str(override_control);
                    for &id in &active {
                        text.push_str(controls(&boxes[id].style).0);
                    }
                }
                _ => {}
            }
            if text.len() > 262_144 {
                let mut end = 262_144;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
            }
        }
        offsets.push(text.len());
        Self {
            text,
            offsets,
            base: if block.unicode_bidi == UnicodeBidi::Plaintext {
                None
            } else {
                Some(
                    if block.direction == Direction::Rtl
                        && !(block.writing_mode.vertical()
                            && block.text_orientation == crate::style::TextOrientation::Upright)
                    {
                        Level::rtl()
                    } else {
                        Level::ltr()
                    },
                )
            },
        }
    }

    pub fn levels(
        &self,
        info: &BidiInfo<'_>,
        range: std::ops::Range<usize>,
        line: bool,
    ) -> Vec<Level> {
        let start = self.offsets[range.start];
        let end = self.offsets[range.end];
        let para = info
            .paragraphs
            .iter()
            .find(|p| p.range.start <= start && p.range.end > start);
        let levels = if line {
            para.map(|p| info.reordered_levels(p, start..end.min(p.range.end)))
        } else {
            None
        };
        let source = levels.as_deref().unwrap_or(&info.levels);
        let base = para.map_or(self.base.unwrap_or(Level::ltr()), |p| p.level);
        range
            .map(|i| source.get(self.offsets[i]).copied().unwrap_or(base))
            .collect()
    }

    pub fn direction(&self, info: &BidiInfo<'_>, token: usize) -> Direction {
        let offset = self.offsets[token];
        if info
            .paragraphs
            .iter()
            .find(|p| p.range.start <= offset && p.range.end > offset)
            .is_some_and(|p| p.level.is_rtl())
        {
            Direction::Rtl
        } else {
            Direction::Ltr
        }
    }
}
