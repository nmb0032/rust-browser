use super::{InitialState, PendingTag, State, Tokenizer, whitespace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScriptState {
    Data,
    LessThan,
    EndOpen,
    EndName,
    EscapeStart,
    EscapeStartDash,
    Escaped,
    EscapedDash,
    EscapedDashDash,
    EscapedLessThan,
    EscapedEndOpen,
    EscapedEndName,
    DoubleEscapeStart,
    DoubleEscaped,
    DoubleEscapedDash,
    DoubleEscapedDashDash,
    DoubleEscapedLessThan,
    DoubleEscapeEnd,
}

impl Tokenizer {
    pub(super) fn rawtext_character(&mut self, ch: char) {
        match self.state {
            State::Rawtext => match ch {
                '&' if self.text_state == InitialState::Rcdata => {
                    self.begin_reference(State::Rawtext)
                }
                '<' => self.state = State::RawLessThan,
                '\0' => {
                    self.error("unexpected-null-character");
                    self.character('\u{FFFD}');
                }
                _ => self.character(ch),
            },
            State::RawLessThan => {
                if ch == '/' {
                    self.raw_candidate.clear();
                    self.state = State::RawEndOpen;
                } else {
                    self.character('<');
                    self.reconsume_in(ch, State::Rawtext);
                }
            }
            State::RawEndOpen => self.text_end_open(ch, State::RawEndName, State::Rawtext),
            State::RawEndName => self.text_end_name(ch, State::Rawtext),
            _ => unreachable!("text state dispatch"),
        }
    }

    fn text_end_open(&mut self, ch: char, next: State, fallback: State) {
        if ch.is_ascii_alphabetic() {
            self.tag = PendingTag::default();
            self.tag.is_end = true;
            self.reconsume_in(ch, next);
        } else {
            self.character_text("</");
            self.reconsume_in(ch, fallback);
        }
    }

    fn text_end_name(&mut self, ch: char, fallback: State) {
        if ch.is_ascii_alphabetic() {
            self.raw_candidate.push(ch);
            self.tag.token.name.push(ch.to_ascii_lowercase());
        } else if self.raw_end_tag.as_deref() == Some(&self.tag.token.name)
            && (whitespace(ch) || matches!(ch, '/' | '>'))
        {
            match ch {
                '/' => self.state = State::SelfClosing,
                '>' => self.emit_tag(),
                _ => self.state = State::BeforeAttributeName,
            }
        } else {
            self.character_text("</");
            self.character_text(&self.raw_candidate.clone());
            self.reconsume_in(ch, fallback);
        }
    }

    pub(super) fn script_character(&mut self, state: ScriptState, ch: char) {
        use ScriptState::*;
        let next = match state {
            Data => match ch {
                '<' => LessThan,
                _ => {
                    self.script_text(ch);
                    Data
                }
            },
            LessThan => match ch {
                '/' => {
                    self.raw_candidate.clear();
                    EndOpen
                }
                '!' => {
                    self.character_text("<!");
                    EscapeStart
                }
                _ => {
                    self.character('<');
                    self.reconsume_in(ch, State::Script(Data));
                    return;
                }
            },
            EndOpen | EscapedEndOpen => {
                self.text_end_open(
                    ch,
                    State::Script(if state == EndOpen {
                        EndName
                    } else {
                        EscapedEndName
                    }),
                    State::Script(if state == EndOpen { Data } else { Escaped }),
                );
                return;
            }
            EndName | EscapedEndName => {
                self.text_end_name(
                    ch,
                    State::Script(if state == EndName { Data } else { Escaped }),
                );
                return;
            }
            EscapeStart => {
                if ch == '-' {
                    self.character('-');
                    EscapeStartDash
                } else {
                    self.reconsume_in(ch, State::Script(Data));
                    return;
                }
            }
            EscapeStartDash => {
                if ch == '-' {
                    self.character('-');
                    EscapedDashDash
                } else {
                    self.reconsume_in(ch, State::Script(Data));
                    return;
                }
            }
            Escaped | EscapedDash | EscapedDashDash => match ch {
                '-' => {
                    self.character('-');
                    if state == Escaped {
                        EscapedDash
                    } else {
                        EscapedDashDash
                    }
                }
                '<' => EscapedLessThan,
                '>' if state == EscapedDashDash => {
                    self.character('>');
                    Data
                }
                _ => {
                    self.script_text(ch);
                    Escaped
                }
            },
            EscapedLessThan => match ch {
                '/' => {
                    self.raw_candidate.clear();
                    EscapedEndOpen
                }
                ch if ch.is_ascii_alphabetic() => {
                    self.keyword.clear();
                    self.character('<');
                    self.reconsume_in(ch, State::Script(DoubleEscapeStart));
                    return;
                }
                _ => {
                    self.character('<');
                    self.reconsume_in(ch, State::Script(Escaped));
                    return;
                }
            },
            DoubleEscapeStart | DoubleEscapeEnd => {
                if whitespace(ch) || matches!(ch, '/' | '>') {
                    self.character(ch);
                    if (self.keyword == "script") == (state == DoubleEscapeStart) {
                        DoubleEscaped
                    } else {
                        Escaped
                    }
                } else if ch.is_ascii_alphabetic() {
                    self.keyword.push(ch.to_ascii_lowercase());
                    self.character(ch);
                    state
                } else {
                    self.reconsume_in(
                        ch,
                        State::Script(if state == DoubleEscapeStart {
                            Escaped
                        } else {
                            DoubleEscaped
                        }),
                    );
                    return;
                }
            }
            DoubleEscaped | DoubleEscapedDash | DoubleEscapedDashDash => match ch {
                '-' => {
                    self.character('-');
                    if state == DoubleEscaped {
                        DoubleEscapedDash
                    } else {
                        DoubleEscapedDashDash
                    }
                }
                '<' => {
                    self.character('<');
                    DoubleEscapedLessThan
                }
                '>' if state == DoubleEscapedDashDash => {
                    self.character('>');
                    Data
                }
                _ => {
                    self.script_text(ch);
                    DoubleEscaped
                }
            },
            DoubleEscapedLessThan => {
                if ch == '/' {
                    self.character('/');
                    self.keyword.clear();
                    DoubleEscapeEnd
                } else {
                    self.reconsume_in(ch, State::Script(DoubleEscaped));
                    return;
                }
            }
        };
        self.state = State::Script(next);
    }

    fn script_text(&mut self, ch: char) {
        if ch == '\0' {
            self.error("unexpected-null-character");
            self.character('\u{FFFD}');
        } else {
            self.character(ch);
        }
    }

    pub(super) fn script_eof(&mut self, state: ScriptState) {
        use ScriptState::*;
        match state {
            LessThan => self.character('<'),
            EndOpen => self.character_text("</"),
            EndName => {
                self.character_text("</");
                self.character_text(&self.raw_candidate.clone());
            }
            Data | EscapeStart | EscapeStartDash => {}
            _ => {
                if state == EscapedLessThan {
                    self.character('<');
                }
                if matches!(state, EscapedEndOpen | EscapedEndName) {
                    self.character_text("</");
                    if state == EscapedEndName {
                        self.character_text(&self.raw_candidate.clone());
                    }
                }
                self.error("eof-in-script-html-comment-like-text");
            }
        }
    }

    pub(super) fn cdata_character(&mut self, ch: char) {
        match self.state {
            State::Cdata => {
                if ch == ']' {
                    self.state = State::CdataBracket;
                } else {
                    self.character(ch);
                }
            }
            State::CdataBracket => {
                if ch == ']' {
                    self.state = State::CdataEnd;
                } else {
                    self.character(']');
                    self.reconsume_in(ch, State::Cdata);
                }
            }
            State::CdataEnd => match ch {
                ']' => self.character(']'),
                '>' => self.state = State::Data,
                _ => {
                    self.character_text("]]");
                    self.reconsume_in(ch, State::Cdata);
                }
            },
            _ => unreachable!("CDATA state dispatch"),
        }
    }
}
