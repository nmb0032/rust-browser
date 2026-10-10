use super::{State, Token, Tokenizer, comments::CommentState, whitespace};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn living_standard_targets_data_and_eof_work_at_every_boundary() {
        for (source, expected, errors) in [
            (
                "<?build?>",
                Some(Token::ProcessingInstruction {
                    target: "build".into(),
                    data: "".into(),
                }),
                vec![],
            ),
            (
                "<?_build-1 \tready?>",
                Some(Token::ProcessingInstruction {
                    target: "_build-1".into(),
                    data: "ready".into(),
                }),
                vec![],
            ),
            (
                "<?build ready>",
                Some(Token::ProcessingInstruction {
                    target: "build".into(),
                    data: "ready".into(),
                }),
                vec![],
            ),
            (
                "<?build a??b?>",
                Some(Token::ProcessingInstruction {
                    target: "build".into(),
                    data: "a??b".into(),
                }),
                vec![],
            ),
            (
                "<?build <&amp;>",
                Some(Token::ProcessingInstruction {
                    target: "build".into(),
                    data: "<&amp;".into(),
                }),
                vec![],
            ),
            (
                "<?xml?>",
                Some(Token::Comment("?xml?".into())),
                vec!["disallowed-processing-instruction-target"],
            ),
            (
                "<?XML-stylesheet href='x'?>",
                Some(Token::Comment("?XML-stylesheet href='x'?".into())),
                vec!["disallowed-processing-instruction-target"],
            ),
            (
                "<?1bad>",
                Some(Token::Comment("?1bad".into())),
                vec!["invalid-first-character-of-processing-instruction-target"],
            ),
            (
                "<?a:b>",
                Some(Token::Comment("?a:b".into())),
                vec!["invalid-processing-instruction-target"],
            ),
            ("<?", None, vec!["eof-in-processing-instruction"]),
            ("<?build data?", None, vec!["eof-in-processing-instruction"]),
        ] {
            for size in 1..=source.len() {
                let mut tokenizer = Tokenizer::new();
                let mut tokens = Vec::new();
                for bytes in source.as_bytes().chunks(size) {
                    tokenizer.feed(std::str::from_utf8(bytes).unwrap());
                    while let Some(token) = tokenizer.next_token() {
                        tokens.push(token);
                    }
                }
                tokenizer.finish();
                while let Some(token) = tokenizer.next_token() {
                    tokens.push(token);
                }
                let mut expected = expected.clone().into_iter().collect::<Vec<_>>();
                expected.push(Token::Eof);
                assert_eq!(tokens, expected, "{source:?}, size {size}");
                assert_eq!(
                    tokenizer
                        .errors()
                        .iter()
                        .map(|error| error.code)
                        .collect::<Vec<_>>(),
                    errors,
                    "{source:?}"
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InstructionState {
    Open,
    Target,
    AfterTarget,
    Data,
    Questionable,
}

impl Tokenizer {
    pub(super) fn instruction_character(&mut self, state: InstructionState, ch: char) {
        use InstructionState::*;
        match state {
            Open => {
                if ch.is_ascii_alphabetic() || ch == '_' {
                    self.reconsume_in(ch, State::Instruction(Target));
                } else {
                    self.invalid_instruction(
                        "invalid-first-character-of-processing-instruction-target",
                        ch,
                    );
                }
            }
            Target => {
                if whitespace(ch) || matches!(ch, '?' | '>') {
                    if self.keyword.eq_ignore_ascii_case("xml")
                        || self.keyword.eq_ignore_ascii_case("xml-stylesheet")
                    {
                        self.invalid_instruction("disallowed-processing-instruction-target", ch);
                    } else {
                        self.comment.clear();
                        self.reconsume_in(ch, State::Instruction(AfterTarget));
                    }
                } else if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                    self.keyword.push(ch);
                } else {
                    self.invalid_instruction("invalid-processing-instruction-target", ch);
                }
            }
            AfterTarget => {
                if !whitespace(ch) {
                    self.reconsume_in(ch, State::Instruction(Data));
                }
            }
            Data => match ch {
                '?' => self.state = State::Instruction(Questionable),
                '>' => self.emit_instruction(),
                _ => self.comment.push(ch),
            },
            Questionable => {
                if ch == '>' {
                    self.emit_instruction();
                } else {
                    self.comment.push('?');
                    self.reconsume_in(ch, State::Instruction(Data));
                }
            }
        }
    }
    fn invalid_instruction(&mut self, error: &'static str, ch: char) {
        self.error(error);
        self.comment = format!("?{}", self.keyword);
        self.reconsume_in(ch, State::Comment(CommentState::Bogus));
    }
    fn emit_instruction(&mut self) {
        let target = std::mem::take(&mut self.keyword);
        let data = std::mem::take(&mut self.comment);
        self.emit(Token::ProcessingInstruction { target, data });
        self.state = State::Data;
    }
}
