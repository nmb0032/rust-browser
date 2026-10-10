use super::{
    Lookahead, State, Step, TokenKind, Tokenizer,
    names::{Escape, Escaped},
    whitespace,
};

#[derive(Debug)]
pub(super) struct QuotedString {
    quote: char,
    value: String,
    escape: Option<Escape>,
}

impl QuotedString {
    pub fn new(quote: char) -> Self {
        Self {
            quote,
            value: String::new(),
            escape: None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Url {
    value: String,
    trailing_whitespace: bool,
    escape: Option<Escape>,
}

impl Tokenizer {
    pub(super) fn string(&mut self, mut string: QuotedString) -> Step {
        if let Some(escape) = &mut string.escape {
            match self.escape(escape) {
                Escaped::Pending => {
                    self.state = State::String(string);
                    return Step::Pending;
                }
                Escaped::Continue => {}
                Escaped::Character(ch) => {
                    string.value.push(ch);
                    string.escape = None;
                }
            }
        } else {
            match self.input.peek(0) {
                Lookahead::Pending => {
                    self.state = State::String(string);
                    return Step::Pending;
                }
                Lookahead::End => {
                    self.error("eof-in-string", self.input.position());
                    return Step::Emit(TokenKind::String(string.value));
                }
                Lookahead::Character(ch) if ch == string.quote => {
                    self.input.consume();
                    return Step::Emit(TokenKind::String(string.value));
                }
                Lookahead::Character('\n') => {
                    self.error("newline-in-string", self.input.position());
                    return Step::Emit(TokenKind::BadString);
                }
                Lookahead::Character('\\') => match self.input.peek(1) {
                    Lookahead::Pending => {
                        self.state = State::String(string);
                        return Step::Pending;
                    }
                    Lookahead::Character('\n') => {
                        self.input.consume();
                        self.input.consume();
                    }
                    Lookahead::End => {
                        self.input.consume();
                    }
                    _ => {
                        string.escape = Some(Escape::new(self.input.position()));
                        self.input.consume();
                    }
                },
                Lookahead::Character(ch) => {
                    self.input.consume();
                    string.value.push(ch);
                }
            }
        }
        self.state = State::String(string);
        Step::Continue
    }

    pub(super) fn url_start(&mut self, name: String) -> Step {
        let next = self.input.peek(0);
        let quoted = match next {
            Lookahead::Pending => {
                self.state = State::UrlStart(name);
                return Step::Pending;
            }
            Lookahead::Character('"' | '\'') => true,
            Lookahead::Character(ch) if whitespace(ch) => match self.input.peek(1) {
                Lookahead::Pending => {
                    self.state = State::UrlStart(name);
                    return Step::Pending;
                }
                Lookahead::Character(ch) if whitespace(ch) => {
                    self.input.consume();
                    self.state = State::UrlStart(name);
                    return Step::Continue;
                }
                Lookahead::Character('"' | '\'') => true,
                _ => false,
            },
            _ => false,
        };
        if quoted {
            Step::Emit(TokenKind::Function(name))
        } else {
            if matches!(next, Lookahead::Character(ch) if whitespace(ch)) {
                self.input.consume();
            }
            self.state = State::Url(Url::default());
            Step::Continue
        }
    }

    pub(super) fn url(&mut self, mut url: Url) -> Step {
        if let Some(escape) = &mut url.escape {
            match self.escape(escape) {
                Escaped::Pending => {
                    self.state = State::Url(url);
                    return Step::Pending;
                }
                Escaped::Continue => {}
                Escaped::Character(ch) => {
                    url.value.push(ch);
                    url.escape = None;
                }
            }
        } else {
            match self.input.peek(0) {
                Lookahead::Pending => {
                    self.state = State::Url(url);
                    return Step::Pending;
                }
                Lookahead::End => {
                    self.error("eof-in-url", self.input.position());
                    return Step::Emit(TokenKind::Url(url.value));
                }
                Lookahead::Character(')') => {
                    self.input.consume();
                    return Step::Emit(TokenKind::Url(url.value));
                }
                Lookahead::Character(ch) if whitespace(ch) => {
                    self.input.consume();
                    url.trailing_whitespace = true;
                }
                Lookahead::Character(ch)
                    if url.trailing_whitespace
                        || matches!(ch, '"' | '\'' | '(')
                        || matches!(ch, '\0'..='\x08' | '\x0b' | '\x0e'..='\x1f' | '\x7f') =>
                {
                    self.error("invalid-url", self.input.position());
                    self.state = State::BadUrl { escape: None };
                    return Step::Continue;
                }
                Lookahead::Character('\\') => match self.valid_escape(0) {
                    None => {
                        self.state = State::Url(url);
                        return Step::Pending;
                    }
                    Some(false) => {
                        self.error("invalid-url", self.input.position());
                        self.state = State::BadUrl { escape: None };
                        return Step::Continue;
                    }
                    Some(true) => {
                        url.escape = Some(Escape::new(self.input.position()));
                        self.input.consume();
                    }
                },
                Lookahead::Character(ch) => {
                    self.input.consume();
                    url.value.push(ch);
                }
            }
        }
        self.state = State::Url(url);
        Step::Continue
    }

    pub(super) fn bad_url(&mut self, mut escape: Option<Escape>) -> Step {
        if let Some(pending) = &mut escape {
            match self.escape(pending) {
                Escaped::Pending => {
                    self.state = State::BadUrl { escape };
                    return Step::Pending;
                }
                Escaped::Continue => {}
                Escaped::Character(_) => escape = None,
            }
        } else {
            match self.input.peek(0) {
                Lookahead::Pending => {
                    self.state = State::BadUrl { escape };
                    return Step::Pending;
                }
                Lookahead::End => return Step::Emit(TokenKind::BadUrl),
                Lookahead::Character(')') => {
                    self.input.consume();
                    return Step::Emit(TokenKind::BadUrl);
                }
                Lookahead::Character('\\') => match self.valid_escape(0) {
                    None => {
                        self.state = State::BadUrl { escape };
                        return Step::Pending;
                    }
                    Some(true) => {
                        escape = Some(Escape::new(self.input.position()));
                        self.input.consume();
                    }
                    Some(false) => {
                        self.input.consume();
                    }
                },
                _ => {
                    self.input.consume();
                }
            }
        }
        self.state = State::BadUrl { escape };
        Step::Continue
    }
}
