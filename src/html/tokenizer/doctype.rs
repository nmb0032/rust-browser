use super::{State, Token, Tokenizer, buffer::TextBuffer, whitespace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DoctypeState {
    BeforeNameSpace,
    BeforeName,
    Name,
    AfterName,
    Keyword,
    AfterPublic,
    BeforePublic,
    PublicQuoted(char),
    AfterPublicIdentifier,
    BetweenIdentifiers,
    AfterSystem,
    BeforeSystem,
    SystemQuoted(char),
    AfterSystemIdentifier,
    Bogus,
}

impl Tokenizer {
    fn emit_doctype(&mut self) {
        let token = std::mem::take(&mut self.doctype);
        self.emit(Token::Doctype(token));
        self.state = State::Data;
    }

    fn quirks(&mut self, error: &'static str) {
        self.error(error);
        self.doctype.force_quirks = true;
    }

    pub(super) fn doctype_character(&mut self, state: DoctypeState, ch: char) {
        use DoctypeState::*;
        let next = match state {
            BeforeNameSpace => {
                if whitespace(ch) {
                    BeforeName
                } else if ch == '>' {
                    self.reconsume_in(ch, State::Doctype(BeforeName));
                    return;
                } else {
                    self.error("missing-whitespace-before-doctype-name");
                    self.reconsume_in(ch, State::Doctype(BeforeName));
                    return;
                }
            }
            BeforeName => match ch {
                ch if whitespace(ch) => BeforeName,
                '>' => {
                    self.quirks("missing-doctype-name");
                    self.emit_doctype();
                    return;
                }
                _ => {
                    let ch = self.doctype_null(ch);
                    let mut name = TextBuffer::default();
                    name.push_input(ch.to_ascii_lowercase(), self.current_character);
                    self.doctype.name = Some(name);
                    Name
                }
            },
            Name => match ch {
                ch if whitespace(ch) => AfterName,
                '>' => {
                    self.emit_doctype();
                    return;
                }
                _ => {
                    let ch = self.doctype_null(ch);
                    self.doctype
                        .name
                        .as_mut()
                        .expect("doctype name")
                        .push_input(ch.to_ascii_lowercase(), self.current_character);
                    Name
                }
            },
            AfterName => match ch {
                ch if whitespace(ch) => AfterName,
                '>' => {
                    self.emit_doctype();
                    return;
                }
                _ => {
                    self.keyword.clear();
                    self.lookahead_position = self.current_position;
                    self.reconsume_in(ch, State::Doctype(Keyword));
                    return;
                }
            },
            Keyword => {
                self.keyword.push(ch.to_ascii_uppercase());
                if self.keyword == "PUBLIC" {
                    AfterPublic
                } else if self.keyword == "SYSTEM" {
                    AfterSystem
                } else if "PUBLIC".starts_with(&self.keyword) || "SYSTEM".starts_with(&self.keyword)
                {
                    Keyword
                } else {
                    self.error_at(
                        "invalid-character-sequence-after-doctype-name",
                        self.lookahead_position,
                    );
                    self.doctype.force_quirks = true;
                    self.reconsume_in(ch, State::Doctype(Bogus));
                    return;
                }
            }
            AfterPublic | BeforePublic | AfterSystem | BeforeSystem => {
                let public = matches!(state, AfterPublic | BeforePublic);
                let after = matches!(state, AfterPublic | AfterSystem);
                match ch {
                    ch if whitespace(ch) => {
                        if public {
                            BeforePublic
                        } else {
                            BeforeSystem
                        }
                    }
                    '"' | '\'' => {
                        if after {
                            self.error(if public {
                                "missing-whitespace-after-doctype-public-keyword"
                            } else {
                                "missing-whitespace-after-doctype-system-keyword"
                            });
                        }
                        if public {
                            self.doctype.public_identifier = Some(TextBuffer::default());
                            PublicQuoted(ch)
                        } else {
                            self.doctype.system_identifier = Some(TextBuffer::default());
                            SystemQuoted(ch)
                        }
                    }
                    '>' => {
                        self.quirks(if public {
                            "missing-doctype-public-identifier"
                        } else {
                            "missing-doctype-system-identifier"
                        });
                        self.emit_doctype();
                        return;
                    }
                    _ => {
                        self.quirks(if public {
                            "missing-quote-before-doctype-public-identifier"
                        } else {
                            "missing-quote-before-doctype-system-identifier"
                        });
                        self.reconsume_in(ch, State::Doctype(Bogus));
                        return;
                    }
                }
            }
            PublicQuoted(quote) | SystemQuoted(quote) => {
                let public = matches!(state, PublicQuoted(_));
                if ch == quote {
                    if public {
                        AfterPublicIdentifier
                    } else {
                        AfterSystemIdentifier
                    }
                } else if ch == '>' {
                    self.quirks(if public {
                        "abrupt-doctype-public-identifier"
                    } else {
                        "abrupt-doctype-system-identifier"
                    });
                    self.emit_doctype();
                    return;
                } else {
                    let ch = self.doctype_null(ch);
                    if public {
                        self.doctype.public_identifier.as_mut()
                    } else {
                        self.doctype.system_identifier.as_mut()
                    }
                    .expect("doctype identifier")
                    .push_input(ch, self.current_character);
                    state
                }
            }
            AfterPublicIdentifier | BetweenIdentifiers => match ch {
                ch if whitespace(ch) => BetweenIdentifiers,
                '>' => {
                    self.emit_doctype();
                    return;
                }
                '"' | '\'' => {
                    if state == AfterPublicIdentifier {
                        self.error(
                            "missing-whitespace-between-doctype-public-and-system-identifiers",
                        );
                    }
                    self.doctype.system_identifier = Some(TextBuffer::default());
                    SystemQuoted(ch)
                }
                _ => {
                    self.quirks("missing-quote-before-doctype-system-identifier");
                    self.reconsume_in(ch, State::Doctype(Bogus));
                    return;
                }
            },
            AfterSystemIdentifier => match ch {
                ch if whitespace(ch) => AfterSystemIdentifier,
                '>' => {
                    self.emit_doctype();
                    return;
                }
                _ => {
                    self.error("unexpected-character-after-doctype-system-identifier");
                    self.reconsume_in(ch, State::Doctype(Bogus));
                    return;
                }
            },
            Bogus => match ch {
                '>' => {
                    self.emit_doctype();
                    return;
                }
                '\0' => {
                    self.error("unexpected-null-character");
                    Bogus
                }
                _ => Bogus,
            },
        };
        self.state = State::Doctype(next);
    }

    fn doctype_null(&mut self, ch: char) -> char {
        if ch == '\0' {
            self.error("unexpected-null-character");
            '\u{FFFD}'
        } else {
            ch
        }
    }

    pub(super) fn doctype_eof(&mut self, state: DoctypeState) {
        if state == DoctypeState::Keyword {
            self.error_at(
                "invalid-character-sequence-after-doctype-name",
                self.lookahead_position,
            );
            self.doctype.force_quirks = true;
        } else if state != DoctypeState::Bogus {
            self.quirks("eof-in-doctype");
        }
        self.emit_doctype();
    }
}
