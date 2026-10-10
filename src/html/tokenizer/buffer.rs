use super::input::InputCharacter;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct TextBuffer(Vec<u16>);

impl TextBuffer {
    pub fn push(&mut self, ch: char) {
        self.0.extend(ch.encode_utf16(&mut [0; 2]).iter().copied());
    }

    pub fn push_input(&mut self, ch: char, input: InputCharacter) {
        match input.literal(ch) {
            InputCharacter::Scalar(ch) => self.push(ch),
            InputCharacter::Surrogate(unit) => self.0.push(unit),
        }
    }

    pub fn push_str(&mut self, text: &str) {
        self.0.extend(text.encode_utf16());
    }

    pub fn pop(&mut self) {
        if let Some(last) = self.0.pop()
            && (0xDC00..=0xDFFF).contains(&last)
            && self
                .0
                .last()
                .is_some_and(|unit| (0xD800..=0xDBFF).contains(unit))
        {
            self.0.pop();
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn equals(&self, text: &str) -> bool {
        self.0.iter().copied().eq(text.encode_utf16())
    }

    pub fn eq_ignore_ascii_case(&self, text: &str) -> bool {
        self.0
            .iter()
            .copied()
            .map(ascii_uppercase)
            .eq(text.encode_utf16().map(ascii_uppercase))
    }

    pub fn is_prefix_of(&self, text: &str, ignore_ascii_case: bool) -> bool {
        let mut expected = text.encode_utf16();
        self.0.iter().all(|unit| {
            expected.next().is_some_and(|next| {
                if ignore_ascii_case {
                    ascii_uppercase(*unit) == ascii_uppercase(next)
                } else {
                    *unit == next
                }
            })
        })
    }

    pub fn into_string(self) -> String {
        String::from_utf16(&self.0).expect("UTF-8 tokenizer cannot emit lone surrogates")
    }

    pub fn into_units(self) -> Vec<u16> {
        self.0
    }

    pub fn from_units(units: &[u16]) -> Self {
        Self(units.to_vec())
    }
}

fn ascii_uppercase(unit: u16) -> u16 {
    if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
        unit - 32
    } else {
        unit
    }
}

impl From<&str> for TextBuffer {
    fn from(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }
}

impl From<String> for TextBuffer {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}
