use super::{DoctypeToken, State, Token, Tokenizer, doctype::DoctypeState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommentState {
    Bogus,
    Start,
    StartDash,
    Data,
    LessThan,
    LessThanBang,
    LessThanBangDash,
    LessThanBangDashDash,
    EndDash,
    End,
    EndBang,
}

impl Tokenizer {
    pub(super) fn declaration_character(&mut self, ch: char) {
        self.declaration.push(ch);
        let text = &self.declaration;
        if text == "--" {
            self.comment.clear();
            self.state = State::Comment(CommentState::Start);
        } else if text.eq_ignore_ascii_case("DOCTYPE") {
            self.doctype = DoctypeToken::default();
            self.state = State::Doctype(DoctypeState::BeforeNameSpace);
        } else if text == "[CDATA[" {
            if self.allow_cdata {
                self.state = State::Cdata;
            } else {
                self.error("cdata-in-html-content");
                self.comment = std::mem::take(&mut self.declaration);
                self.state = State::Comment(CommentState::Bogus);
            }
        } else if !"--".starts_with(text)
            && !"DOCTYPE".starts_with(&text.to_ascii_uppercase())
            && !"[CDATA[".starts_with(text)
        {
            self.error_at("incorrectly-opened-comment", self.lookahead_position);
            self.comment = std::mem::take(&mut self.declaration);
            // The mismatching character is consumed by the bogus-comment state.
            self.comment.pop();
            self.reconsume_in(ch, State::Comment(CommentState::Bogus));
        }
    }

    pub(super) fn emit_comment(&mut self) {
        let comment = std::mem::take(&mut self.comment);
        self.emit(Token::Comment(comment));
        self.state = State::Data;
    }

    pub(super) fn comment_character(&mut self, state: CommentState, ch: char) {
        use CommentState::*;
        let next = match state {
            Bogus => match ch {
                '>' => {
                    self.emit_comment();
                    return;
                }
                '\0' => {
                    self.error("unexpected-null-character");
                    self.comment.push('\u{FFFD}');
                    Bogus
                }
                _ => {
                    self.comment.push(ch);
                    Bogus
                }
            },
            Start => match ch {
                '-' => StartDash,
                '>' => {
                    self.error("abrupt-closing-of-empty-comment");
                    self.emit_comment();
                    return;
                }
                _ => {
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            },
            StartDash => match ch {
                '-' => End,
                '>' => {
                    self.error("abrupt-closing-of-empty-comment");
                    self.emit_comment();
                    return;
                }
                _ => {
                    self.comment.push('-');
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            },
            Data => match ch {
                '<' => {
                    self.comment.push('<');
                    LessThan
                }
                '-' => EndDash,
                '\0' => {
                    self.error("unexpected-null-character");
                    self.comment.push('\u{FFFD}');
                    Data
                }
                _ => {
                    self.comment.push(ch);
                    Data
                }
            },
            LessThan => match ch {
                '!' => {
                    self.comment.push('!');
                    LessThanBang
                }
                '<' => {
                    self.comment.push('<');
                    LessThan
                }
                _ => {
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            },
            LessThanBang => {
                if ch == '-' {
                    LessThanBangDash
                } else {
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            }
            LessThanBangDash => {
                if ch == '-' {
                    LessThanBangDashDash
                } else {
                    self.reconsume_in(ch, State::Comment(EndDash));
                    return;
                }
            }
            LessThanBangDashDash => {
                if ch != '>' {
                    self.error("nested-comment");
                }
                self.reconsume_in(ch, State::Comment(End));
                return;
            }
            EndDash => {
                if ch == '-' {
                    End
                } else {
                    self.comment.push('-');
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            }
            End => match ch {
                '>' => {
                    self.emit_comment();
                    return;
                }
                '!' => EndBang,
                '-' => {
                    self.comment.push('-');
                    End
                }
                _ => {
                    self.comment.push_str("--");
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            },
            EndBang => match ch {
                '-' => {
                    self.comment.push_str("--!");
                    EndDash
                }
                '>' => {
                    self.error("incorrectly-closed-comment");
                    self.emit_comment();
                    return;
                }
                _ => {
                    self.comment.push_str("--!");
                    self.reconsume_in(ch, State::Comment(Data));
                    return;
                }
            },
        };
        self.state = State::Comment(next);
    }

    pub(super) fn comment_eof(&mut self, state: CommentState) {
        if state != CommentState::Bogus {
            self.error("eof-in-comment");
        }
        self.emit_comment();
    }
}
