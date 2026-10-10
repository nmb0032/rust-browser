use super::{State, Tokenizer, entities::ENTITIES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReferenceState {
    Start,
    Named,
    Ambiguous,
    Numeric,
    HexStart,
    DecimalStart,
    Hex,
    Decimal,
}

impl Tokenizer {
    pub(super) fn begin_reference(&mut self, state: State) {
        self.return_state = state;
        self.reference.clear();
        self.reference_positions.clear();
        self.reference_match = None;
        self.reference_value = 0;
        self.state = State::Reference(ReferenceState::Start);
    }

    fn reference_output(&mut self, text: &str) {
        if matches!(
            self.return_state,
            State::DoubleQuoted | State::SingleQuoted | State::Unquoted
        ) {
            self.tag.value().push_str(text);
        } else {
            self.character_text(text);
        }
    }

    pub(super) fn reference_character(&mut self, state: ReferenceState, ch: char) {
        use ReferenceState::*;
        match state {
            Start => match ch {
                '#' => {
                    self.reference.push('#');
                    self.state = State::Reference(Numeric);
                }
                ch if ch.is_ascii_alphanumeric() => self.reconsume_in(ch, State::Reference(Named)),
                _ => {
                    self.reference_output("&");
                    self.reconsume_in(ch, self.return_state);
                }
            },
            Named => {
                self.reference.push(ch);
                self.reference_positions.push(self.current_position);
                let start = ENTITIES.partition_point(|(name, _)| *name < self.reference.as_str());
                if let Some((name, value)) = ENTITIES.get(start)
                    && name.starts_with(&self.reference)
                {
                    if *name == self.reference {
                        self.reference_match = Some((value, self.reference.len()));
                        if name.ends_with(';') {
                            self.finish_named(false);
                        }
                    }
                } else {
                    self.finish_named(false);
                }
            }
            Ambiguous => {
                if ch.is_ascii_alphanumeric() {
                    self.reference_output(&ch.to_string());
                } else {
                    if ch == ';' {
                        self.error("unknown-named-character-reference");
                    }
                    self.reconsume_in(ch, self.return_state);
                }
            }
            Numeric => {
                if matches!(ch, 'x' | 'X') {
                    self.reference.push(ch);
                    self.state = State::Reference(HexStart);
                } else {
                    self.reconsume_in(ch, State::Reference(DecimalStart));
                }
            }
            HexStart | DecimalStart => {
                let radix = if state == HexStart { 16 } else { 10 };
                if ch.is_digit(radix) && ch.is_ascii() {
                    self.reconsume_in(
                        ch,
                        State::Reference(if radix == 16 { Hex } else { Decimal }),
                    );
                } else {
                    self.error("absence-of-digits-in-numeric-character-reference");
                    let text = format!("&{}", self.reference);
                    self.reference_output(&text);
                    self.reconsume_in(ch, self.return_state);
                }
            }
            Hex | Decimal => {
                let radix = if state == Hex { 16 } else { 10 };
                if let Some(digit) = ch.to_digit(radix).filter(|_| ch.is_ascii()) {
                    self.reference_value = self
                        .reference_value
                        .saturating_mul(radix)
                        .saturating_add(digit);
                } else {
                    if ch != ';' {
                        self.error("missing-semicolon-after-character-reference");
                    }
                    let position = self.current_position;
                    if ch == ';' {
                        self.current_position.byte_offset += 1;
                        self.current_position.column += 1;
                    }
                    self.finish_numeric();
                    self.current_position = position;
                    if ch != ';' {
                        self.reconsume = Some(self.current_character.literal(ch));
                    }
                }
            }
        }
    }

    fn finish_named(&mut self, eof: bool) {
        if let Some((value, length)) = self.reference_match {
            let name = &self.reference[..length];
            let next = self.reference[length..].chars().next();
            let attribute = matches!(
                self.return_state,
                State::DoubleQuoted | State::SingleQuoted | State::Unquoted
            );
            if attribute
                && !name.ends_with(';')
                && next.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '=')
            {
                let text = format!("&{name}");
                self.reference_output(&text);
            } else {
                if !name.ends_with(';') {
                    let position = self
                        .reference_positions
                        .get(length)
                        .copied()
                        .unwrap_or(self.current_position);
                    self.error_at("missing-semicolon-after-character-reference", position);
                }
                self.reference_output(value);
            }
            let suffix = &self.reference[length..];
            let positions = &self.reference_positions[length..];
            let replay = suffix.chars().zip(positions).collect::<Vec<_>>();
            for (ch, position) in replay.into_iter().rev() {
                self.replay
                    .push_front((self.current_character.literal(ch), *position));
            }
            self.state = self.return_state;
        } else {
            self.reference_output("&");
            let buffered = std::mem::take(&mut self.reference);
            let replay = buffered
                .chars()
                .zip(&self.reference_positions)
                .collect::<Vec<_>>();
            for (ch, position) in replay.into_iter().rev() {
                self.replay
                    .push_front((self.current_character.literal(ch), *position));
            }
            self.state = State::Reference(ReferenceState::Ambiguous);
            if eof && self.replay.is_empty() {
                self.state = self.return_state;
            }
        }
    }

    fn finish_numeric(&mut self) {
        let mut value = self.reference_value;
        if value == 0 {
            self.error("null-character-reference");
            value = 0xFFFD;
        } else if value > 0x10FFFF {
            self.error("character-reference-outside-unicode-range");
            value = 0xFFFD;
        } else if (0xD800..=0xDFFF).contains(&value) {
            self.error("surrogate-character-reference");
            value = 0xFFFD;
        } else if (0xFDD0..=0xFDEF).contains(&value) || value & 0xFFFF >= 0xFFFE {
            self.error("noncharacter-character-reference");
        } else if matches!(value, 0x0D | 0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F..=0x9F) {
            self.error("control-character-reference");
            if (0x80..=0x9F).contains(&value) {
                value = WINDOWS_1252[(value - 0x80) as usize];
            }
        }
        self.reference_output(
            &char::from_u32(value)
                .expect("validated reference")
                .to_string(),
        );
        self.state = self.return_state;
    }

    pub(super) fn reference_eof(&mut self, state: ReferenceState) {
        use ReferenceState::*;
        match state {
            Named => self.finish_named(true),
            Hex | Decimal => {
                self.error("missing-semicolon-after-character-reference");
                self.finish_numeric();
            }
            Numeric | HexStart | DecimalStart => {
                self.error("absence-of-digits-in-numeric-character-reference");
                let text = format!("&{}", self.reference);
                self.reference_output(&text);
                self.state = self.return_state;
            }
            Start => {
                self.reference_output("&");
                self.state = self.return_state;
            }
            Ambiguous => self.state = self.return_state,
        }
    }
}

const WINDOWS_1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x192, 0x201E, 0x2026, 0x2020, 0x2021, 0x2C6, 0x2030, 0x160, 0x2039,
    0x152, 0x8D, 0x17D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x2DC,
    0x2122, 0x161, 0x203A, 0x153, 0x9D, 0x17E, 0x178,
];
