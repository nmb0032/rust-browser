use super::{PendingTag, State, Tokenizer, whitespace};

impl Tokenizer {
    pub(super) fn step(&mut self, ch: char) {
        match self.state {
            State::Data => match ch {
                '<' => {
                    self.state = State::TagOpen;
                    self.flush_characters();
                }
                '\0' => {
                    self.error("unexpected-null-character");
                    self.characters.push(ch);
                }
                _ => self.characters.push(ch),
            },
            State::TagOpen => match ch {
                '!' => self.begin_markup("<!"),
                '/' => self.state = State::EndTagOpen,
                ch if ch.is_ascii_alphabetic() => {
                    self.tag = PendingTag::default();
                    self.reconsume_in(ch, State::TagName);
                }
                '?' => {
                    self.error("unexpected-question-mark-instead-of-tag-name");
                    self.begin_markup("<");
                    self.reconsume = Some(ch);
                }
                _ => {
                    self.error("invalid-first-character-of-tag-name");
                    self.characters.push('<');
                    self.reconsume_in(ch, State::Data);
                }
            },
            State::EndTagOpen => match ch {
                ch if ch.is_ascii_alphabetic() => {
                    self.tag = PendingTag::default();
                    self.tag.is_end = true;
                    self.reconsume_in(ch, State::TagName);
                }
                '>' => {
                    self.error("missing-end-tag-name");
                    self.state = State::Data;
                }
                _ => {
                    self.error("invalid-first-character-of-tag-name");
                    self.begin_markup("</");
                    self.reconsume = Some(ch);
                }
            },
            State::TagName => match ch {
                ch if whitespace(ch) => self.state = State::BeforeAttributeName,
                '/' => self.state = State::SelfClosing,
                '>' => self.emit_tag(),
                '\0' => {
                    self.error("unexpected-null-character");
                    self.tag.token.name.push('\u{FFFD}');
                }
                _ => self.tag.token.name.push(ch.to_ascii_lowercase()),
            },
            State::BeforeAttributeName => match ch {
                ch if whitespace(ch) => {}
                '/' | '>' => self.reconsume_in(ch, State::AfterAttributeName),
                '=' => {
                    self.error("unexpected-equals-sign-before-attribute-name");
                    self.tag.start_attribute();
                    self.tag.name().push(ch);
                    self.state = State::AttributeName;
                }
                _ => {
                    self.tag.start_attribute();
                    self.reconsume_in(ch, State::AttributeName);
                }
            },
            State::AttributeName => match ch {
                ch if whitespace(ch) || matches!(ch, '/' | '>') => {
                    self.finish_attribute_name();
                    self.reconsume_in(ch, State::AfterAttributeName);
                }
                '=' => {
                    self.finish_attribute_name();
                    self.state = State::BeforeAttributeValue;
                }
                '\0' => {
                    self.error("unexpected-null-character");
                    self.tag.name().push('\u{FFFD}');
                }
                '"' | '\'' | '<' => {
                    self.error("unexpected-character-in-attribute-name");
                    self.tag.name().push(ch);
                }
                _ => self.tag.name().push(ch.to_ascii_lowercase()),
            },
            State::AfterAttributeName => match ch {
                ch if whitespace(ch) => {}
                '/' => self.state = State::SelfClosing,
                '=' => self.state = State::BeforeAttributeValue,
                '>' => self.emit_tag(),
                _ => {
                    self.tag.start_attribute();
                    self.reconsume_in(ch, State::AttributeName);
                }
            },
            State::BeforeAttributeValue => match ch {
                ch if whitespace(ch) => {}
                '"' => self.state = State::DoubleQuoted,
                '\'' => self.state = State::SingleQuoted,
                '>' => {
                    self.error("missing-attribute-value");
                    self.emit_tag();
                }
                _ => self.reconsume_in(ch, State::Unquoted),
            },
            State::DoubleQuoted | State::SingleQuoted => {
                let quote = if self.state == State::DoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                if ch == quote {
                    self.state = State::AfterQuoted;
                } else if ch == '\0' {
                    self.error("unexpected-null-character");
                    self.tag.value().push('\u{FFFD}');
                } else {
                    self.tag.value().push(ch);
                }
            }
            State::Unquoted => match ch {
                ch if whitespace(ch) => {
                    self.tag.finish_attribute();
                    self.state = State::BeforeAttributeName;
                }
                '>' => self.emit_tag(),
                '\0' => {
                    self.error("unexpected-null-character");
                    self.tag.value().push('\u{FFFD}');
                }
                '"' | '\'' | '<' | '=' | '`' => {
                    self.error("unexpected-character-in-unquoted-attribute-value");
                    self.tag.value().push(ch);
                }
                _ => self.tag.value().push(ch),
            },
            State::AfterQuoted => match ch {
                ch if whitespace(ch) => {
                    self.tag.finish_attribute();
                    self.state = State::BeforeAttributeName;
                }
                '/' => self.state = State::SelfClosing,
                '>' => self.emit_tag(),
                _ => {
                    self.error("missing-whitespace-between-attributes");
                    self.reconsume_in(ch, State::BeforeAttributeName);
                }
            },
            State::SelfClosing => {
                if ch == '>' {
                    self.tag.token.self_closing = true;
                    self.emit_tag();
                } else {
                    self.error("unexpected-solidus-in-tag");
                    self.reconsume_in(ch, State::BeforeAttributeName);
                }
            }
            State::Markup => self.markup_character(ch),
            State::Rawtext | State::RawLessThan | State::RawEndOpen | State::RawEndName => {
                self.rawtext_character(ch);
            }
            State::Plaintext => {
                if ch == '\0' {
                    self.error("unexpected-null-character");
                    self.characters.push('\u{FFFD}');
                } else {
                    self.characters.push(ch);
                }
            }
        }
    }
}
