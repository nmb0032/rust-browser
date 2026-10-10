use super::SourcePosition;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lookahead {
    Character(char),
    End,
    Pending,
}

#[derive(Debug, Default)]
pub(super) struct Input {
    buffer: String,
    cursor: usize,
    base: usize,
    line: usize,
    column: usize,
    pub finished: bool,
}

impl Input {
    pub fn feed(&mut self, chunk: &str) {
        assert!(!self.finished, "cannot feed a finished CSS tokenizer");
        if self.cursor >= 4096 {
            self.buffer.drain(..self.cursor);
            self.base += self.cursor;
            self.cursor = 0;
        }
        self.buffer.push_str(chunk);
    }

    pub fn position(&self) -> SourcePosition {
        SourcePosition {
            byte_offset: self.base + self.cursor,
            line: self.line + 1,
            column: self.column + 1,
        }
    }

    pub fn peek(&self, index: usize) -> Lookahead {
        let mut chars = self.buffer[self.cursor..].chars().peekable();
        for current in 0..=index {
            let Some(ch) = chars.next() else {
                return if self.finished {
                    Lookahead::End
                } else {
                    Lookahead::Pending
                };
            };
            let ch = match ch {
                '\r' => {
                    match chars.peek() {
                        Some('\n') => {
                            chars.next();
                        }
                        None if !self.finished => return Lookahead::Pending,
                        _ => {}
                    }
                    '\n'
                }
                '\x0c' => '\n',
                '\0' => '\u{fffd}',
                _ => ch,
            };
            if current == index {
                return Lookahead::Character(ch);
            }
        }
        unreachable!("lookahead loop returns at the requested index")
    }

    pub fn consume(&mut self) -> Option<char> {
        let Lookahead::Character(ch) = self.peek(0) else {
            return None;
        };
        let remaining = &self.buffer[self.cursor..];
        self.cursor += if remaining.starts_with("\r\n") {
            2
        } else if remaining.starts_with(['\r', '\x0c', '\0']) {
            1
        } else {
            ch.len_utf8()
        };
        if ch == '\n' {
            self.line += 1;
            self.column = 0;
        } else {
            self.column += 1;
        }
        Some(ch)
    }
}
