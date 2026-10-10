//! Resumable core states from HTML 13.2.5.
//! https://html.spec.whatwg.org/multipage/parsing.html#tokenization
//!
//! Character references, full comments/doctypes, RCDATA, script data and CDATA
//! remain roadmap steps 6.3-6.5 and 6.11. `Markup` and RAWTEXT retain the earlier browser's
//! declaration-skipping and style-text behavior without rescanning input.

mod compatibility;
mod core;
mod input;
mod tag;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;

pub use input::SourcePosition;
pub use tag::{TagAttribute, TagToken};

use input::Input;
use tag::PendingTag;

pub const MAX_PARSE_ERRORS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    pub position: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    StartTag(TagToken),
    EndTag(TagToken),
    Character(String),
    /// Compatibility-only declaration; not yet a comment or doctype token.
    Markup(String),
    Eof,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InitialState {
    #[default]
    Data,
    Rawtext,
    Plaintext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    DoubleQuoted,
    SingleQuoted,
    Unquoted,
    AfterQuoted,
    SelfClosing,
    Markup,
    Rawtext,
    RawLessThan,
    RawEndOpen,
    RawEndName,
    Plaintext,
}

#[derive(Debug)]
pub struct Tokenizer {
    input: Input,
    state: State,
    tag: PendingTag,
    characters: String,
    output: VecDeque<Token>,
    errors: Vec<ParseError>,
    dropped_errors: usize,
    current_position: SourcePosition,
    reconsume: Option<char>,
    is_final: bool,
    emitted_eof: bool,
    markup: String,
    markup_quote: Option<char>,
    raw_end_tag: Option<String>,
    raw_candidate: String,
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tokenizer {
    pub fn new() -> Self {
        Self::with_state(InitialState::Data, None)
    }

    pub fn with_state(state: InitialState, last_start_tag: Option<&str>) -> Self {
        let input = Input::default();
        Self {
            current_position: input.position(),
            input,
            state: match state {
                InitialState::Data => State::Data,
                InitialState::Rawtext => State::Rawtext,
                InitialState::Plaintext => State::Plaintext,
            },
            tag: PendingTag::default(),
            characters: String::new(),
            output: VecDeque::new(),
            errors: Vec::new(),
            dropped_errors: 0,
            reconsume: None,
            is_final: false,
            emitted_eof: false,
            markup: String::new(),
            markup_quote: None,
            raw_end_tag: last_start_tag.map(str::to_owned),
            raw_candidate: String::new(),
        }
    }

    /// Feed before `finish`, draining `next_token` between chunks to bound memory.
    pub fn feed(&mut self, chunk: &str) {
        assert!(!self.is_final, "cannot feed a finished tokenizer");
        self.input.feed(chunk);
    }

    /// Mark input complete. Drain `next_token` to receive recovery tokens and EOF.
    pub fn finish(&mut self) {
        self.is_final = true;
    }

    /// Returns `None` when more input is needed, or after the single EOF token.
    pub fn next_token(&mut self) -> Option<Token> {
        loop {
            if let Some(token) = self.output.pop_front() {
                return Some(token);
            }
            if self.emitted_eof {
                return None;
            }
            let character = if let Some(ch) = self.reconsume.take() {
                Some(ch)
            } else if let Some((ch, position)) = self.input.next() {
                self.current_position = position;
                self.validate_input(ch);
                Some(ch)
            } else {
                None
            };
            if let Some(ch) = character {
                self.step(ch);
            } else if self.is_final {
                self.current_position = self.input.position();
                self.eof();
            } else {
                self.flush_characters();
                return self.output.pop_front();
            }
        }
    }

    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub fn dropped_errors(&self) -> usize {
        self.dropped_errors
    }

    /// Instrumentation for verifying that split tokens are consumed only once.
    pub fn consumed_characters(&self) -> usize {
        self.input.consumed_characters
    }

    pub(crate) fn enter_rawtext(&mut self, end_tag: &str) {
        self.state = State::Rawtext;
        self.raw_end_tag = Some(end_tag.to_owned());
    }

    fn error(&mut self, code: &'static str) {
        if self.errors.len() < MAX_PARSE_ERRORS {
            self.errors.push(ParseError {
                code,
                position: self.current_position,
            });
        } else {
            self.dropped_errors += 1;
        }
    }

    fn reconsume_in(&mut self, ch: char, state: State) {
        self.state = state;
        self.reconsume = Some(ch);
    }

    fn flush_characters(&mut self) {
        if !self.characters.is_empty() {
            self.output
                .push_back(Token::Character(std::mem::take(&mut self.characters)));
        }
    }

    fn emit(&mut self, token: Token) {
        self.flush_characters();
        self.output.push_back(token);
    }

    fn emit_tag(&mut self) {
        self.tag.finish_attribute();
        let tag = std::mem::take(&mut self.tag);
        self.state = State::Data;
        if tag.is_end {
            if !tag.token.attributes.is_empty() {
                self.error("end-tag-with-attributes");
            }
            if tag.token.self_closing {
                self.error("end-tag-with-trailing-solidus");
            }
            self.emit(Token::EndTag(tag.token));
        } else {
            self.raw_end_tag = Some(tag.token.name.clone());
            self.emit(Token::StartTag(tag.token));
        }
    }

    fn finish_attribute_name(&mut self) {
        if self.tag.finish_name() {
            self.error("duplicate-attribute");
        }
    }

    fn validate_input(&mut self, ch: char) {
        let value = u32::from(ch);
        if (0xFDD0..=0xFDEF).contains(&value) || value & 0xFFFF >= 0xFFFE {
            self.error("noncharacter-in-input-stream");
        } else if matches!(value, 0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F..=0x9F) {
            self.error("control-character-in-input-stream");
        }
    }

    fn eof(&mut self) {
        match self.state {
            State::Data | State::Rawtext | State::Plaintext => {}
            State::TagOpen => {
                self.error("eof-before-tag-name");
                self.characters.push('<');
            }
            State::EndTagOpen => {
                self.error("eof-before-tag-name");
                self.characters.push_str("</");
            }
            State::RawLessThan => self.characters.push('<'),
            State::RawEndOpen => self.characters.push_str("</"),
            State::RawEndName => {
                self.characters.push_str("</");
                self.characters.push_str(&self.raw_candidate);
            }
            State::Markup => {
                self.error(if self.markup.starts_with("<!--") {
                    "eof-in-comment"
                } else {
                    "unsupported-declaration-at-eof"
                });
                let markup = std::mem::take(&mut self.markup);
                self.emit(Token::Markup(markup));
            }
            State::AttributeName => {
                self.finish_attribute_name();
                self.error("eof-in-tag");
            }
            _ => self.error("eof-in-tag"),
        }
        self.emit(Token::Eof);
        self.emitted_eof = true;
    }
}

fn whitespace(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\u{000C}' | ' ')
}
