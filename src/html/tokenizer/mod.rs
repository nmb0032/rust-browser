//! Resumable states from HTML 13.2.5.
//! https://html.spec.whatwg.org/multipage/parsing.html#tokenization

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

use std::collections::VecDeque;

pub use input::SourcePosition;
pub use tag::{TagAttribute, TagToken};

use comments::CommentState;
use doctype::DoctypeState;
use input::Input;
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
pub enum Token {
    StartTag(TagToken),
    EndTag(TagToken),
    Character(String),
    Comment(String),
    Doctype(DoctypeToken),
    ProcessingInstruction { target: String, data: String },
    Eof,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DoctypeToken {
    pub name: Option<String>,
    pub public_identifier: Option<String>,
    pub system_identifier: Option<String>,
    pub force_quirks: bool,
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
    characters: String,
    output: VecDeque<(Token, SourcePosition, Vec<SourcePosition>)>,
    character_positions: Vec<SourcePosition>,
    emitted_character_positions: Vec<SourcePosition>,
    emitted_position: SourcePosition,
    errors: Vec<ParseError>,
    dropped_errors: usize,
    current_position: SourcePosition,
    lookahead_position: SourcePosition,
    reconsume: Option<char>,
    is_final: bool,
    emitted_eof: bool,
    declaration: String,
    comment: String,
    doctype: DoctypeToken,
    keyword: String,
    reference: String,
    reference_value: u32,
    reference_match: Option<(&'static str, usize)>,
    return_state: State,
    replay: VecDeque<(char, SourcePosition)>,
    reference_positions: Vec<SourcePosition>,
    text_state: InitialState,
    pub(crate) allow_cdata: bool,
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
            characters: String::new(),
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
            is_final: false,
            emitted_eof: false,
            declaration: String::new(),
            comment: String::new(),
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
        self.raw_end_tag = Some(end_tag.to_owned());
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
        self.reconsume = Some(ch);
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
        self.characters.push(ch);
        self.character_positions.push(self.current_position);
    }

    fn character_text(&mut self, text: &str) {
        self.characters.push_str(text);
        self.character_positions
            .extend(text.chars().map(|_| self.current_position));
    }

    fn emit(&mut self, token: Token) {
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
