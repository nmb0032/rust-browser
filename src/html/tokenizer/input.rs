use std::collections::VecDeque;

/// Positions in the original UTF-8 source, with one-based normalized line/column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug)]
pub(super) struct Input {
    chunks: VecDeque<String>,
    offset: usize,
    position: SourcePosition,
    skip_lf: bool,
    pub consumed_characters: usize,
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
            consumed_characters: 0,
        }
    }
}

impl Input {
    pub fn feed(&mut self, chunk: &str) {
        if !chunk.is_empty() {
            self.chunks.push_back(chunk.to_owned());
        }
    }

    pub fn position(&self) -> SourcePosition {
        self.position
    }

    pub fn next(&mut self) -> Option<(char, SourcePosition)> {
        loop {
            let chunk = self.chunks.front()?;
            let ch = chunk[self.offset..].chars().next()?;
            self.offset += ch.len_utf8();
            if self.offset == chunk.len() {
                self.chunks.pop_front();
                self.offset = 0;
            }
            self.consumed_characters += 1;
            let position = self.position;
            self.position.byte_offset += ch.len_utf8();
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
                self.position.column += 1;
            }
            return Some((normalized, position));
        }
    }
}
