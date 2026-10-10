//! Resumable CSS Syntax Level 3 tokenization of decoded Unicode text.
//! https://www.w3.org/TR/css-syntax-3/#tokenization
//!
//! Comments are discarded; brackets and function arguments remain flat tokens.
//! Unicode-range descriptor parsing is a parser-level opt-in, not the default
//! tokenization mode. Byte spans refer to the original, unfiltered UTF-8 input.

mod input;
mod names;
mod strings;
#[cfg(test)]
mod tests;

use input::{Input, Lookahead};

pub const MAX_PARSE_ERRORS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    pub position: SourcePosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashKind {
    Id,
    Unrestricted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericKind {
    Integer,
    Number,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numeric {
    pub representation: String,
    pub kind: NumericKind,
}

impl Numeric {
    /// Property grammars decide whether overflow/infinity is an allowed value.
    pub fn value(&self) -> Result<f64, std::num::ParseFloatError> {
        self.representation.parse()
    }

    pub fn sign(&self) -> Option<char> {
        self.representation
            .chars()
            .next()
            .filter(|ch| matches!(ch, '+' | '-'))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Function(String),
    AtKeyword(String),
    Hash { value: String, kind: HashKind },
    String(String),
    BadString,
    Url(String),
    BadUrl,
    Delim(char),
    Number(Numeric),
    Percentage(Numeric),
    Dimension { number: Numeric, unit: String },
    Whitespace,
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: SourceSpan,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Data,
    Comment {
        star: bool,
    },
    Whitespace,
    Name(names::Name),
    Number {
        number: Numeric,
        phase: NumberPhase,
    },
    String(strings::QuotedString),
    UrlStart(String),
    Url(strings::Url),
    BadUrl {
        escape: Option<names::Escape>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumberPhase {
    Start,
    Integer,
    Fraction,
    Exponent,
}

enum Step {
    Continue,
    Pending,
    Emit(TokenKind),
}

#[derive(Debug)]
pub struct Tokenizer {
    input: Input,
    state: State,
    start: SourcePosition,
    errors: Vec<ParseError>,
    dropped_errors: usize,
    emitted_eof: bool,
}

impl Default for Tokenizer {
    fn default() -> Self {
        let input = Input::default();
        Self {
            start: input.position(),
            input,
            state: State::Data,
            errors: Vec::new(),
            dropped_errors: 0,
            emitted_eof: false,
        }
    }
}

impl Tokenizer {
    /// Chunks must contain decoded UTF-8 text; lexical constructs may be split.
    pub fn feed(&mut self, chunk: &str) {
        self.input.feed(chunk);
    }

    pub fn finish(&mut self) {
        self.input.finished = true;
    }

    /// `None` means more input is needed, or the single EOF token was emitted.
    pub fn next_token(&mut self) -> Option<Token> {
        if self.emitted_eof {
            return None;
        }
        loop {
            match self.step() {
                Step::Continue => {}
                Step::Pending => return None,
                Step::Emit(kind) => {
                    self.emitted_eof = kind == TokenKind::Eof;
                    return Some(Token {
                        kind,
                        span: SourceSpan {
                            start: self.start,
                            end: self.input.position(),
                        },
                    });
                }
            }
        }
    }

    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub fn dropped_errors(&self) -> usize {
        self.dropped_errors
    }

    fn error(&mut self, code: &'static str, position: SourcePosition) {
        if self.errors.len() < MAX_PARSE_ERRORS {
            self.errors.push(ParseError { code, position });
        } else {
            self.dropped_errors += 1;
        }
    }

    fn step(&mut self) -> Step {
        match std::mem::take(&mut self.state) {
            State::Data => self.data(),
            State::Comment { star } => self.comment(star),
            State::Whitespace => match self.input.peek(0) {
                Lookahead::Character(ch) if whitespace(ch) => {
                    self.input.consume();
                    self.state = State::Whitespace;
                    Step::Continue
                }
                Lookahead::Pending => {
                    self.state = State::Whitespace;
                    Step::Pending
                }
                _ => Step::Emit(TokenKind::Whitespace),
            },
            State::Name(name) => self.name(name),
            State::Number { number, phase } => self.number(number, phase),
            State::String(string) => self.string(string),
            State::UrlStart(name) => self.url_start(name),
            State::Url(url) => self.url(url),
            State::BadUrl { escape } => self.bad_url(escape),
        }
    }

    fn data(&mut self) -> Step {
        self.start = self.input.position();
        let ch = match self.input.peek(0) {
            Lookahead::Pending => return Step::Pending,
            Lookahead::End => return Step::Emit(TokenKind::Eof),
            Lookahead::Character(ch) => ch,
        };
        let kind = match ch {
            ch if whitespace(ch) => {
                self.input.consume();
                self.state = State::Whitespace;
                return Step::Continue;
            }
            '"' | '\'' => {
                self.input.consume();
                self.state = State::String(strings::QuotedString::new(ch));
                return Step::Continue;
            }
            '/' => match self.starts_with("/*") {
                None => return Step::Pending,
                Some(true) => {
                    self.input.consume();
                    self.input.consume();
                    self.state = State::Comment { star: false };
                    return Step::Continue;
                }
                Some(false) => TokenKind::Delim(ch),
            },
            '#' => {
                let has_name = match self.input.peek(1) {
                    Lookahead::Character(ch) if names::ident(ch) => true,
                    Lookahead::Pending => return Step::Pending,
                    _ => match self.valid_escape(1) {
                        Some(value) => value,
                        None => return Step::Pending,
                    },
                };
                if has_name {
                    let kind = match self.starts_ident(1) {
                        Some(true) => HashKind::Id,
                        Some(false) => HashKind::Unrestricted,
                        None => return Step::Pending,
                    };
                    self.input.consume();
                    self.state = State::Name(names::Name::new(names::Target::Hash(kind)));
                    return Step::Continue;
                }
                TokenKind::Delim(ch)
            }
            '+' | '-' | '.' | '0'..='9' => {
                match self.starts_number() {
                    None => return Step::Pending,
                    Some(true) => {
                        self.state = State::Number {
                            number: Numeric {
                                representation: String::new(),
                                kind: NumericKind::Integer,
                            },
                            phase: NumberPhase::Start,
                        };
                        return Step::Continue;
                    }
                    Some(false) => {}
                }
                if ch == '-' {
                    match self.starts_with("-->") {
                        None => return Step::Pending,
                        Some(true) => {
                            self.input.consume();
                            self.input.consume();
                            self.input.consume();
                            return Step::Emit(TokenKind::Cdc);
                        }
                        Some(false) => {}
                    }
                    match self.starts_ident(0) {
                        None => return Step::Pending,
                        Some(true) => {
                            self.state = State::Name(names::Name::new(names::Target::Ident));
                            return Step::Continue;
                        }
                        Some(false) => {}
                    }
                }
                TokenKind::Delim(ch)
            }
            '<' => match self.starts_with("<!--") {
                None => return Step::Pending,
                Some(true) => {
                    for _ in 0..4 {
                        self.input.consume();
                    }
                    return Step::Emit(TokenKind::Cdo);
                }
                Some(false) => TokenKind::Delim(ch),
            },
            '@' => match self.starts_ident(1) {
                None => return Step::Pending,
                Some(true) => {
                    self.input.consume();
                    self.state = State::Name(names::Name::new(names::Target::AtKeyword));
                    return Step::Continue;
                }
                Some(false) => TokenKind::Delim(ch),
            },
            '\\' => match self.valid_escape(0) {
                None => return Step::Pending,
                Some(true) => {
                    self.state = State::Name(names::Name::new(names::Target::Ident));
                    return Step::Continue;
                }
                Some(false) => {
                    self.error("invalid-escape", self.start);
                    TokenKind::Delim(ch)
                }
            },
            ch if names::ident_start(ch) => {
                self.state = State::Name(names::Name::new(names::Target::Ident));
                return Step::Continue;
            }
            ':' => TokenKind::Colon,
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            '[' => TokenKind::OpenSquare,
            ']' => TokenKind::CloseSquare,
            '(' => TokenKind::OpenParen,
            ')' => TokenKind::CloseParen,
            '{' => TokenKind::OpenCurly,
            '}' => TokenKind::CloseCurly,
            _ => TokenKind::Delim(ch),
        };
        self.input.consume();
        Step::Emit(kind)
    }

    fn comment(&mut self, star: bool) -> Step {
        match self.input.peek(0) {
            Lookahead::Pending => {
                self.state = State::Comment { star };
                Step::Pending
            }
            Lookahead::End => {
                self.error("eof-in-comment", self.input.position());
                Step::Continue
            }
            Lookahead::Character(ch) => {
                self.input.consume();
                if !(star && ch == '/') {
                    self.state = State::Comment { star: ch == '*' };
                }
                Step::Continue
            }
        }
    }

    fn starts_with(&self, literal: &str) -> Option<bool> {
        for (index, expected) in literal.chars().enumerate() {
            match self.input.peek(index) {
                Lookahead::Pending => return None,
                Lookahead::Character(ch) if ch == expected => {}
                _ => return Some(false),
            }
        }
        Some(true)
    }

    fn valid_escape(&self, offset: usize) -> Option<bool> {
        match self.input.peek(offset) {
            Lookahead::Pending => None,
            Lookahead::Character('\\') => match self.input.peek(offset + 1) {
                Lookahead::Pending => None,
                Lookahead::Character('\n') => Some(false),
                _ => Some(true),
            },
            _ => Some(false),
        }
    }

    fn starts_ident(&self, offset: usize) -> Option<bool> {
        match self.input.peek(offset) {
            Lookahead::Pending => None,
            Lookahead::Character(ch) if names::ident_start(ch) => Some(true),
            Lookahead::Character('-') => match self.input.peek(offset + 1) {
                Lookahead::Pending => None,
                Lookahead::Character(ch) if names::ident_start(ch) || ch == '-' => Some(true),
                _ => self.valid_escape(offset + 1),
            },
            _ => self.valid_escape(offset),
        }
    }

    fn starts_number(&self) -> Option<bool> {
        match self.input.peek(0) {
            Lookahead::Character(ch) if ch.is_ascii_digit() => Some(true),
            Lookahead::Character('+' | '-') => match self.input.peek(1) {
                Lookahead::Pending => None,
                Lookahead::Character(ch) if ch.is_ascii_digit() => Some(true),
                Lookahead::Character('.') => self.digit_at(2),
                _ => Some(false),
            },
            Lookahead::Character('.') => self.digit_at(1),
            _ => Some(false),
        }
    }

    fn digit_at(&self, offset: usize) -> Option<bool> {
        match self.input.peek(offset) {
            Lookahead::Pending => None,
            Lookahead::Character(ch) => Some(ch.is_ascii_digit()),
            Lookahead::End => Some(false),
        }
    }

    fn number(&mut self, mut number: Numeric, mut phase: NumberPhase) -> Step {
        let next = self.input.peek(0);
        if next == Lookahead::Pending {
            self.state = State::Number { number, phase };
            return Step::Pending;
        }
        if phase == NumberPhase::Start {
            if let Lookahead::Character(ch @ ('+' | '-')) = next {
                self.input.consume();
                number.representation.push(ch);
            }
            self.state = State::Number {
                number,
                phase: NumberPhase::Integer,
            };
            return Step::Continue;
        }
        if let Lookahead::Character(ch) = next
            && ch.is_ascii_digit()
        {
            self.input.consume();
            number.representation.push(ch);
            self.state = State::Number { number, phase };
            return Step::Continue;
        }
        if phase == NumberPhase::Integer && next == Lookahead::Character('.') {
            match self.digit_at(1) {
                None => {
                    self.state = State::Number { number, phase };
                    return Step::Pending;
                }
                Some(true) => {
                    self.input.consume();
                    number.representation.push('.');
                    number.kind = NumericKind::Number;
                    self.state = State::Number {
                        number,
                        phase: NumberPhase::Fraction,
                    };
                    return Step::Continue;
                }
                Some(false) => {}
            }
        }
        if phase != NumberPhase::Exponent && matches!(next, Lookahead::Character('e' | 'E')) {
            let sign = match self.input.peek(1) {
                Lookahead::Character(ch @ ('+' | '-')) => Some(ch),
                _ => None,
            };
            match self.digit_at(if sign.is_some() { 2 } else { 1 }) {
                None => {
                    self.state = State::Number { number, phase };
                    return Step::Pending;
                }
                Some(true) => {
                    if let Some(ch) = self.input.consume() {
                        number.representation.push(ch);
                    }
                    if let Some(sign) = sign {
                        self.input.consume();
                        number.representation.push(sign);
                    }
                    number.kind = NumericKind::Number;
                    phase = NumberPhase::Exponent;
                    self.state = State::Number { number, phase };
                    return Step::Continue;
                }
                Some(false) => {}
            }
        }
        match self.starts_ident(0) {
            None => {
                self.state = State::Number { number, phase };
                Step::Pending
            }
            Some(true) => {
                self.state = State::Name(names::Name::new(names::Target::Dimension(number)));
                Step::Continue
            }
            Some(false) if next == Lookahead::Character('%') => {
                self.input.consume();
                Step::Emit(TokenKind::Percentage(number))
            }
            Some(false) => Step::Emit(TokenKind::Number(number)),
        }
    }
}

fn whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n')
}
