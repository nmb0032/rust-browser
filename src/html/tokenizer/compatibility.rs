use super::{PendingTag, State, Token, Tokenizer, whitespace};

impl Tokenizer {
    pub(super) fn begin_markup(&mut self, prefix: &str) {
        self.markup.clear();
        self.markup.push_str(prefix);
        self.markup_quote = None;
        self.state = State::Markup;
    }

    pub(super) fn markup_character(&mut self, ch: char) {
        self.markup.push(ch);
        let complete = if self.markup.starts_with("<!--") {
            self.markup.ends_with("-->")
        } else {
            match self.markup_quote {
                Some(quote) if ch == quote => self.markup_quote = None,
                Some(_) => {}
                None if matches!(ch, '"' | '\'') => self.markup_quote = Some(ch),
                None => {}
            }
            ch == '>' && self.markup_quote.is_none()
        };
        if complete {
            let markup = std::mem::take(&mut self.markup);
            self.emit(Token::Markup(markup));
            self.state = State::Data;
        }
    }

    pub(super) fn rawtext_character(&mut self, ch: char) {
        match self.state {
            State::Rawtext => match ch {
                '<' => self.state = State::RawLessThan,
                '\0' => {
                    self.error("unexpected-null-character");
                    self.characters.push('\u{FFFD}');
                }
                _ => self.characters.push(ch),
            },
            State::RawLessThan => {
                if ch == '/' {
                    self.raw_candidate.clear();
                    self.state = State::RawEndOpen;
                } else {
                    self.characters.push('<');
                    self.reconsume_in(ch, State::Rawtext);
                }
            }
            State::RawEndOpen => {
                if ch.is_ascii_alphabetic() {
                    self.tag = PendingTag::default();
                    self.tag.is_end = true;
                    self.reconsume_in(ch, State::RawEndName);
                } else {
                    self.characters.push_str("</");
                    self.reconsume_in(ch, State::Rawtext);
                }
            }
            State::RawEndName => {
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
                    self.characters.push_str("</");
                    self.characters.push_str(&self.raw_candidate);
                    self.reconsume_in(ch, State::Rawtext);
                }
            }
            _ => unreachable!("RAWTEXT dispatch"),
        }
    }
}
