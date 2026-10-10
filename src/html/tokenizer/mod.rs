//! Resumable states from HTML 13.2.5.
//! https://html.spec.whatwg.org/multipage/parsing.html#tokenization

mod buffer;
mod comments;
mod core;
mod doctype;
mod entities;
mod input;
mod instructions;
mod references;
mod tag;
#[cfg(test)]
mod tests;
mod text;
#[cfg(test)]
mod utf16_tests;

use std::collections::VecDeque;

pub use input::SourcePosition;
pub use tag::{TagAttribute, TagToken};

use buffer::TextBuffer;
use comments::CommentState;
use doctype::DoctypeState;
use input::{Input, InputCharacter};
use instructions::InstructionState;
use references::ReferenceState;
use tag::PendingTag;
use text::ScriptState;

pub const MAX_PARSE_ERRORS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    pub position: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token<T = String> {
    StartTag(TagToken<T>),
    EndTag(TagToken<T>),
    Character(T),
    Comment(T),
    Doctype(DoctypeToken<T>),
    ProcessingInstruction { target: T, data: T },
    Eof,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DoctypeToken<T = String> {
    pub name: Option<T>,
    pub public_identifier: Option<T>,
    pub system_identifier: Option<T>,
    pub force_quirks: bool,
}

impl<T> Token<T> {
    fn map_text<U>(self, mut map: impl FnMut(T) -> U) -> Token<U> {
        match self {
            Self::StartTag(tag) => Token::StartTag(tag.map_text(&mut map)),
            Self::EndTag(tag) => Token::EndTag(tag.map_text(&mut map)),
            Self::Character(text) => Token::Character(map(text)),
            Self::Comment(text) => Token::Comment(map(text)),
            Self::Doctype(data) => Token::Doctype(DoctypeToken {
                name: data.name.map(&mut map),
                public_identifier: data.public_identifier.map(&mut map),
                system_identifier: data.system_identifier.map(&mut map),
                force_quirks: data.force_quirks,
            }),
            Self::ProcessingInstruction { target, data } => Token::ProcessingInstruction {
                target: map(target),
                data: map(data),
            },
            Self::Eof => Token::Eof,
        }
    }
}

/// Lossless DOMString tokenization, including lone UTF-16 surrogates.
/// Uses the same state machine and diagnostics as the UTF-8 tokenizer.
#[derive(Debug)]
pub struct Utf16Tokenizer {
    inner: Tokenizer,
}

impl Default for Utf16Tokenizer {
    fn default() -> Self {
        Self::with_state(InitialState::Data, None)
    }
}

impl Utf16Tokenizer {
    pub fn with_state(state: InitialState, last_start_tag: Option<&[u16]>) -> Self {
        let mut inner = Tokenizer::with_state(state, None);
        inner.raw_end_tag = last_start_tag.map(TextBuffer::from_units);
        Self { inner }
    }

    /// A surrogate pair may be split between feeds; lone surrogates are retained.
    pub fn feed(&mut self, chunk: &[u16]) {
        assert!(!self.inner.is_final, "cannot feed a finished tokenizer");
        self.inner.input.feed_utf16(chunk);
    }

    pub fn finish(&mut self) {
        self.inner.finish();
    }

    pub fn next_token(&mut self) -> Option<Token<Vec<u16>>> {
        self.inner
            .next_token_text()
            .map(|token| token.map_text(TextBuffer::into_units))
    }

    pub fn errors(&self) -> &[ParseError] {
        self.inner.errors()
    }

    pub fn dropped_errors(&self) -> usize {
        self.inner.dropped_errors()
    }

    pub fn position(&self) -> SourcePosition {
        self.inner.position()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InitialState {
    #[default]
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Cdata,
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
    Declaration,
    Comment(CommentState),
    Doctype(DoctypeState),
    Reference(ReferenceState),
    Instruction(InstructionState),
    Rawtext,
    RawLessThan,
    RawEndOpen,
    RawEndName,
    Script(ScriptState),
    Cdata,
    CdataBracket,
    CdataEnd,
    Plaintext,
}

#[derive(Debug)]
pub struct Tokenizer {
    input: Input,
    state: State,
    tag: PendingTag,
    characters: TextBuffer,
    output: VecDeque<(Token<TextBuffer>, SourcePosition, Vec<SourcePosition>)>,
    character_positions: Vec<SourcePosition>,
    emitted_character_positions: Vec<SourcePosition>,
    emitted_position: SourcePosition,
    errors: Vec<ParseError>,
    dropped_errors: usize,
    current_position: SourcePosition,
    lookahead_position: SourcePosition,
    reconsume: Option<InputCharacter>,
    current_character: InputCharacter,
    is_final: bool,
    emitted_eof: bool,
    declaration: TextBuffer,
    comment: TextBuffer,
    doctype: DoctypeToken<TextBuffer>,
    keyword: String,
    reference: String,
    reference_value: u32,
    reference_match: Option<(&'static str, usize)>,
    return_state: State,
    replay: VecDeque<(InputCharacter, SourcePosition)>,
    reference_positions: Vec<SourcePosition>,
    text_state: InitialState,
    pub(crate) allow_cdata: bool,
    raw_end_tag: Option<TextBuffer>,
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
            lookahead_position: input.position(),
            input,
            state: match state {
                InitialState::Data => State::Data,
                InitialState::Rcdata => State::Rawtext,
                InitialState::Rawtext => State::Rawtext,
                InitialState::ScriptData => State::Script(ScriptState::Data),
                InitialState::Cdata => State::Cdata,
                InitialState::Plaintext => State::Plaintext,
            },
            tag: PendingTag::default(),
            characters: TextBuffer::default(),
            output: VecDeque::new(),
            character_positions: Vec::new(),
            emitted_character_positions: Vec::new(),
            emitted_position: input::SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            errors: Vec::new(),
            dropped_errors: 0,
            reconsume: None,
            current_character: InputCharacter::Scalar('\0'),
            is_final: false,
            emitted_eof: false,
            declaration: TextBuffer::default(),
            comment: TextBuffer::default(),
            doctype: DoctypeToken::default(),
            keyword: String::new(),
            reference: String::new(),
            reference_value: 0,
            reference_match: None,
            return_state: State::Data,
            replay: VecDeque::new(),
            reference_positions: Vec::new(),
            text_state: state,
            allow_cdata: false,
            raw_end_tag: last_start_tag.map(TextBuffer::from),
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
        self.next_token_text()
            .map(|token| token.map_text(TextBuffer::into_string))
    }

    fn next_token_text(&mut self) -> Option<Token<TextBuffer>> {
        loop {
            if let Some((token, position, positions)) = self.output.pop_front() {
                self.emitted_position = position;
                self.emitted_character_positions = positions;
                return Some(token);
            }
            if self.emitted_eof {
                return None;
            }
            let character = if let Some(ch) = self.reconsume.take() {
                Some(ch)
            } else if let Some((ch, position)) = self.replay.pop_front() {
                self.current_position = position;
                Some(ch)
            } else if let Some((ch, position)) = self.input.next(self.is_final) {
                self.current_position = position;
                match ch {
                    InputCharacter::Scalar(ch) => self.validate_input(ch),
                    InputCharacter::Surrogate(_) => self.error("surrogate-in-input-stream"),
                }
                Some(ch)
            } else {
                None
            };
            if let Some(ch) = character {
                self.current_character = ch;
                self.step(ch.classification());
            } else if self.is_final {
                self.current_position = self.input.position();
                self.eof();
            } else {
                self.flush_characters();
                if let Some((token, position, positions)) = self.output.pop_front() {
                    self.emitted_position = position;
                    self.emitted_character_positions = positions;
                    return Some(token);
                }
                return None;
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

    pub(crate) fn enter_text(&mut self, state: InitialState, end_tag: &str) {
        self.text_state = state;
        self.state = match state {
            InitialState::ScriptData => State::Script(ScriptState::Data),
            InitialState::Plaintext => State::Plaintext,
            _ => State::Rawtext,
        };
        self.raw_end_tag = Some(end_tag.into());
    }

    pub fn position(&self) -> SourcePosition {
        self.emitted_position
    }

    pub(crate) fn emitted_character_positions(&self) -> &[SourcePosition] {
        &self.emitted_character_positions
    }

    fn error(&mut self, code: &'static str) {
        self.error_at(code, self.current_position);
    }

    fn error_at(&mut self, code: &'static str, position: SourcePosition) {
        if self.errors.len() < MAX_PARSE_ERRORS {
            let index = self
                .errors
                .partition_point(|error| error.position.byte_offset <= position.byte_offset);
            self.errors.insert(index, ParseError { code, position });
        } else {
            self.dropped_errors += 1;
        }
    }

    fn reconsume_in(&mut self, ch: char, state: State) {
        self.state = state;
        self.reconsume = Some(self.current_character.literal(ch));
    }

    fn flush_characters(&mut self) {
        if !self.characters.is_empty() {
            let positions = std::mem::take(&mut self.character_positions);
            self.output.push_back((
                Token::Character(std::mem::take(&mut self.characters)),
                positions[0],
                positions,
            ));
        }
    }

    fn character(&mut self, ch: char) {
        self.characters.push_input(ch, self.current_character);
        self.character_positions.push(self.current_position);
    }

    fn character_text(&mut self, text: &str) {
        self.characters.push_str(text);
        self.character_positions
            .extend(text.chars().map(|_| self.current_position));
    }

    fn emit(&mut self, token: Token<TextBuffer>) {
        self.flush_characters();
        self.output
            .push_back((token, self.current_position, Vec::new()));
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
                self.character('<');
            }
            State::EndTagOpen => {
                self.error("eof-before-tag-name");
                self.character_text("</");
            }
            State::RawLessThan => self.character('<'),
            State::RawEndOpen => self.character_text("</"),
            State::RawEndName => {
                self.character_text("</");
                self.character_text(&self.raw_candidate.clone());
            }
            State::Declaration => {
                self.error_at("incorrectly-opened-comment", self.lookahead_position);
                self.comment = std::mem::take(&mut self.declaration);
                self.emit_comment();
            }
            State::Comment(state) => self.comment_eof(state),
            State::Instruction(_) => self.error("eof-in-processing-instruction"),
            State::Doctype(state) => self.doctype_eof(state),
            State::Reference(state) => {
                self.reference_eof(state);
                return;
            }
            State::Script(state) => self.script_eof(state),
            State::Cdata => self.error("eof-in-cdata"),
            State::CdataBracket => {
                self.character(']');
                self.error("eof-in-cdata");
            }
            State::CdataEnd => {
                self.character_text("]]");
                self.error("eof-in-cdata");
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
