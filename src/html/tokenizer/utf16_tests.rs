use super::*;

fn tokenize(
    source: &[u16],
    state: InitialState,
    size: usize,
) -> (Vec<Token<Vec<u16>>>, Vec<ParseError>) {
    let mut tokenizer = Utf16Tokenizer::with_state(state, None);
    let mut tokens = Vec::new();
    for chunk in source.chunks(size) {
        tokenizer.feed(chunk);
        drain(&mut tokenizer, &mut tokens);
    }
    tokenizer.finish();
    drain(&mut tokenizer, &mut tokens);
    (tokens, tokenizer.errors().to_vec())
}

fn drain(tokenizer: &mut Utf16Tokenizer, tokens: &mut Vec<Token<Vec<u16>>>) {
    while let Some(token) = tokenizer.next_token() {
        if let Token::Character(text) = token {
            if let Some(Token::Character(previous)) = tokens.last_mut() {
                previous.extend(text);
            } else {
                tokens.push(Token::Character(text));
            }
        } else {
            tokens.push(token);
        }
    }
}

#[test]
fn pairs_and_lone_surrogates_are_lossless_at_every_boundary() {
    for units in [
        vec![0xD800],
        vec![0xDFFF],
        vec![0x61, 0xDFFF],
        vec![0xD800, 0x61],
        vec![0xD800, 0xDC00],
        vec![0xD800, 0xD800, 0xDC00, 0xDFFF],
        vec![0xDFFF, 0xD800, 0x61, 0xFFFD],
    ] {
        for state in [
            InitialState::Data,
            InitialState::Rawtext,
            InitialState::Rcdata,
            InitialState::ScriptData,
            InitialState::Plaintext,
            InitialState::Cdata,
        ] {
            let whole = tokenize(&units, state, usize::MAX);
            assert_eq!(whole.0, vec![Token::Character(units.clone()), Token::Eof]);
            let expected_surrogates = char::decode_utf16(units.iter().copied())
                .filter(Result::is_err)
                .count();
            assert_eq!(
                whole
                    .1
                    .iter()
                    .filter(|error| error.code == "surrogate-in-input-stream")
                    .count(),
                expected_surrogates
            );
            for size in 1..=units.len() {
                assert_eq!(tokenize(&units, state, size), whole);
            }
        }
    }
}

#[test]
fn non_scalar_names_values_comments_and_instructions_retain_their_code_units() {
    for (prefix, suffix, expected) in [
        ("<p", " a", "name"),
        ("<p a", "=x>", "attribute-name"),
        ("<p a='", "'>", "attribute-value"),
        ("<!--", "-->", "comment"),
        ("<!-", ">", "comment"),
        ("</", ">", "comment"),
        ("<?build ", "?>", "instruction"),
        ("<!DOCTYPE h", ">", "doctype-name"),
        ("<!DOCTYPE html PUBLIC '", "' 's'>", "public"),
        ("<!DOCTYPE html SYSTEM '", "'>", "system"),
    ] {
        for surrogate in [0xD800, 0xDFFF] {
            let mut source = prefix.encode_utf16().collect::<Vec<_>>();
            source.push(surrogate);
            source.extend(suffix.encode_utf16());
            // Complete the first tag-name test after the inserted attribute prefix.
            if expected == "name" {
                source.push(u16::from(b'>'));
            }
            let whole = tokenize(&source, InitialState::Data, usize::MAX);
            let units = match (&whole.0[0], expected) {
                (Token::StartTag(tag), "name") => &tag.name,
                (Token::StartTag(tag), "attribute-name") => &tag.attributes[0].name,
                (Token::StartTag(tag), "attribute-value") => &tag.attributes[0].value,
                (Token::Comment(text), "comment") => text,
                (Token::ProcessingInstruction { data, .. }, "instruction") => data,
                (Token::Doctype(data), "doctype-name") => data.name.as_ref().unwrap(),
                (Token::Doctype(data), "public") => data.public_identifier.as_ref().unwrap(),
                (Token::Doctype(data), "system") => data.system_identifier.as_ref().unwrap(),
                _ => panic!("unexpected tokens: {:?}", whole.0),
            };
            assert!(units.contains(&surrogate), "{prefix:?}");
            assert!(!units.contains(&0xFFFD), "{prefix:?}");
            assert_eq!(
                whole
                    .1
                    .iter()
                    .filter(|error| error.code == "surrogate-in-input-stream")
                    .count(),
                1
            );
            for size in 1..=source.len() {
                assert_eq!(
                    tokenize(&source, InitialState::Data, size),
                    whole,
                    "{prefix:?}"
                );
            }
        }
    }
}

#[test]
fn references_and_reconsumption_do_not_replace_or_revalidate_surrogates() {
    for prefix in [
        "<", "&", "&amp", "&not", "&#", "&#x", "&#xD800", "<?a", "<!-",
    ] {
        let mut source = prefix.encode_utf16().collect::<Vec<_>>();
        source.push(0xD800);
        source.extend("x>".encode_utf16());
        let whole = tokenize(&source, InitialState::Data, usize::MAX);
        assert_eq!(
            whole
                .1
                .iter()
                .filter(|error| error.code == "surrogate-in-input-stream")
                .count(),
            1
        );
        let mut output = Vec::new();
        for token in whole.0.clone() {
            token.map_text(|text| {
                output.extend(text);
            });
        }
        assert_eq!(
            output.iter().filter(|unit| **unit == 0xD800).count(),
            1,
            "{prefix:?}"
        );
        for size in 1..=source.len() {
            assert_eq!(
                tokenize(&source, InitialState::Data, size),
                whole,
                "{prefix:?}"
            );
        }
    }
}

#[test]
fn surrogate_positions_follow_utf16_columns_and_normalized_newlines() {
    let units = [0xD800, 0xDC00, 0xDFFF, 0x0D, 0x0A, 0x61, 0xD800];
    let (_, errors) = tokenize(&units, InitialState::Data, 1);
    assert_eq!(
        errors,
        vec![
            ParseError {
                code: "surrogate-in-input-stream",
                position: SourcePosition {
                    byte_offset: 4,
                    line: 1,
                    column: 3,
                }
            },
            ParseError {
                code: "surrogate-in-input-stream",
                position: SourcePosition {
                    byte_offset: 10,
                    line: 2,
                    column: 2,
                }
            },
        ]
    );
}
