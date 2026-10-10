use std::collections::VecDeque;

/// Decoded UTF-8 offsets and one-based normalized UTF-16 line/column.
/// For UTF-16 input, lone surrogates occupy three bytes in the offset (WTF-8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug)]
pub(super) struct Input {
    chunks: VecDeque<Chunk>,
    offset: usize,
    position: SourcePosition,
    skip_lf: bool,
    pending: Option<u32>,
    pub consumed_characters: usize,
}

#[derive(Debug)]
enum Chunk {
    Utf8(String),
    Utf16(Vec<u16>),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum InputCharacter {
    Scalar(char),
    Surrogate(u16),
}

impl InputCharacter {
    pub fn classification(self) -> char {
        match self {
            Self::Scalar(ch) => ch,
            // Surrogates take the non-ASCII "anything else" branches. The
            // original code unit is retained separately, never replaced in output.
            Self::Surrogate(_) => '\u{FFFD}',
        }
    }

    pub fn literal(self, ch: char) -> Self {
        if ch == '\u{FFFD}' && matches!(self, Self::Surrogate(_)) {
            self
        } else {
            Self::Scalar(ch)
        }
    }
}

impl Default for Input {
    fn default() -> Self {
        Self {
            chunks: VecDeque::new(),
            offset: 0,
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            skip_lf: false,
            pending: None,
            consumed_characters: 0,
        }
    }
}

impl Input {
    pub fn feed(&mut self, chunk: &str) {
        if !chunk.is_empty() {
            self.chunks.push_back(Chunk::Utf8(chunk.to_owned()));
        }
    }

    pub fn feed_utf16(&mut self, chunk: &[u16]) {
        if !chunk.is_empty() {
            self.chunks.push_back(Chunk::Utf16(chunk.to_vec()));
        }
    }

    pub fn position(&self) -> SourcePosition {
        self.position
    }

    fn next_raw(&mut self) -> Option<u32> {
        let chunk = self.chunks.front()?;
        let (value, width, length) = match chunk {
            Chunk::Utf8(text) => {
                let ch = text[self.offset..].chars().next()?;
                (u32::from(ch), ch.len_utf8(), text.len())
            }
            Chunk::Utf16(units) => (u32::from(units[self.offset]), 1, units.len()),
        };
        self.offset += width;
        if self.offset == length {
            self.chunks.pop_front();
            self.offset = 0;
        }
        Some(value)
    }

    pub fn next(&mut self, is_final: bool) -> Option<(InputCharacter, SourcePosition)> {
        loop {
            let value = self.pending.take().or_else(|| self.next_raw())?;
            let character = if (0xD800..=0xDBFF).contains(&value) {
                match self.next_raw() {
                    Some(low) if (0xDC00..=0xDFFF).contains(&low) => InputCharacter::Scalar(
                        char::from_u32(0x10000 + ((value - 0xD800) << 10) + low - 0xDC00)
                            .expect("surrogate pair"),
                    ),
                    Some(next) => {
                        self.pending = Some(next);
                        InputCharacter::Surrogate(value as u16)
                    }
                    None if !is_final => {
                        self.pending = Some(value);
                        return None;
                    }
                    None => InputCharacter::Surrogate(value as u16),
                }
            } else if (0xDC00..=0xDFFF).contains(&value) {
                InputCharacter::Surrogate(value as u16)
            } else {
                InputCharacter::Scalar(char::from_u32(value).expect("scalar input"))
            };
            self.consumed_characters += 1;
            let position = self.position;
            let ch = character.classification();
            self.position.byte_offset += match character {
                InputCharacter::Scalar(ch) => ch.len_utf8(),
                InputCharacter::Surrogate(_) => 3,
            };
            if ch == '\n' && self.skip_lf {
                self.skip_lf = false;
                continue;
            }
            self.skip_lf = ch == '\r';
            let normalized = if ch == '\r' { '\n' } else { ch };
            if normalized == '\n' {
                self.position.line += 1;
                self.position.column = 1;
            } else {
                self.position.column += normalized.len_utf16();
            }
            return Some((character.literal(normalized), position));
        }
    }
}
