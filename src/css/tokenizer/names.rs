use super::{HashKind, Lookahead, Numeric, SourcePosition, State, Step, TokenKind, Tokenizer};

#[derive(Debug)]
pub(super) enum Target {
    Ident,
    AtKeyword,
    Hash(HashKind),
    Dimension(Numeric),
}

#[derive(Debug)]
pub(super) struct Name {
    value: String,
    target: Target,
    escape: Option<Escape>,
}

impl Name {
    pub fn new(target: Target) -> Self {
        Self {
            value: String::new(),
            target,
            escape: None,
        }
    }
}

#[derive(Debug)]
pub(super) struct Escape {
    pub position: SourcePosition,
    value: u32,
    digits: u8,
}

impl Escape {
    pub fn new(position: SourcePosition) -> Self {
        Self {
            position,
            value: 0,
            digits: 0,
        }
    }
}

pub(super) enum Escaped {
    Pending,
    Continue,
    Character(char),
}

impl Tokenizer {
    pub(super) fn name(&mut self, mut name: Name) -> Step {
        if let Some(escape) = &mut name.escape {
            match self.escape(escape) {
                Escaped::Pending => {
                    self.state = State::Name(name);
                    return Step::Pending;
                }
                Escaped::Continue => {
                    self.state = State::Name(name);
                    return Step::Continue;
                }
                Escaped::Character(ch) => {
                    name.value.push(ch);
                    name.escape = None;
                }
            }
        } else {
            match self.input.peek(0) {
                Lookahead::Pending => {
                    self.state = State::Name(name);
                    return Step::Pending;
                }
                Lookahead::Character(ch) if ident(ch) => {
                    self.input.consume();
                    name.value.push(ch);
                    self.state = State::Name(name);
                    return Step::Continue;
                }
                Lookahead::Character('\\') => match self.valid_escape(0) {
                    None => {
                        self.state = State::Name(name);
                        return Step::Pending;
                    }
                    Some(true) => {
                        name.escape = Some(Escape::new(self.input.position()));
                        self.input.consume();
                        self.state = State::Name(name);
                        return Step::Continue;
                    }
                    Some(false) => {}
                },
                _ => {}
            }
            return match name.target {
                Target::Ident if self.input.peek(0) == Lookahead::Character('(') => {
                    self.input.consume();
                    if name.value.eq_ignore_ascii_case("url") {
                        self.state = State::UrlStart(name.value);
                        Step::Continue
                    } else {
                        Step::Emit(TokenKind::Function(name.value))
                    }
                }
                Target::Ident => Step::Emit(TokenKind::Ident(name.value)),
                Target::AtKeyword => Step::Emit(TokenKind::AtKeyword(name.value)),
                Target::Hash(kind) => Step::Emit(TokenKind::Hash {
                    value: name.value,
                    kind,
                }),
                Target::Dimension(number) => Step::Emit(TokenKind::Dimension {
                    number,
                    unit: name.value,
                }),
            };
        }
        self.state = State::Name(name);
        Step::Continue
    }

    pub(super) fn escape(&mut self, escape: &mut Escape) -> Escaped {
        match self.input.peek(0) {
            Lookahead::Pending => Escaped::Pending,
            Lookahead::Character(ch) if escape.digits < 6 && ch.is_ascii_hexdigit() => {
                self.input.consume();
                let digit = match ch {
                    '0'..='9' => ch as u32 - '0' as u32,
                    'a'..='f' => ch as u32 - 'a' as u32 + 10,
                    'A'..='F' => ch as u32 - 'A' as u32 + 10,
                    _ => unreachable!("ASCII hex digit was checked"),
                };
                escape.value = escape.value * 16 + digit;
                escape.digits += 1;
                Escaped::Continue
            }
            Lookahead::End if escape.digits == 0 => {
                self.error("eof-in-escape", escape.position);
                Escaped::Character('\u{fffd}')
            }
            Lookahead::Character(ch) if escape.digits == 0 => {
                self.input.consume();
                Escaped::Character(ch)
            }
            next => {
                if matches!(next, Lookahead::Character(ch) if super::whitespace(ch)) {
                    self.input.consume();
                }
                Escaped::Character(
                    char::from_u32(escape.value)
                        .filter(|ch| *ch != '\0')
                        .unwrap_or('\u{fffd}'),
                )
            }
        }
    }
}

pub(super) fn ident(ch: char) -> bool {
    ident_start(ch) || ch.is_ascii_digit() || ch == '-'
}

pub(super) fn ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic()
        || ch == '_'
        || matches!(
            ch,
            '\u{b7}'
                | '\u{c0}'..='\u{d6}'
                | '\u{d8}'..='\u{f6}'
                | '\u{f8}'..='\u{37d}'
                | '\u{37f}'..='\u{1fff}'
                | '\u{200c}'..='\u{200d}'
                | '\u{203f}'..='\u{2040}'
                | '\u{2070}'..='\u{218f}'
                | '\u{2c00}'..='\u{2fef}'
                | '\u{3001}'..='\u{d7ff}'
                | '\u{f900}'..='\u{fdcf}'
                | '\u{fdf0}'..='\u{fffd}'
                | '\u{10000}'..='\u{10ffff}'
        )
}
