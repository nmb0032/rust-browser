use super::*;

fn tokenize(source: &str, chunk_chars: usize) -> (Vec<Token>, Vec<ParseError>, usize) {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = Vec::new();
    let mut chunk = String::new();
    for (index, ch) in source.chars().enumerate() {
        chunk.push(ch);
        if (index + 1) % chunk_chars == 0 {
            tokenizer.feed(&chunk);
            chunk.clear();
            tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
        }
    }
    tokenizer.feed(&chunk);
    tokenizer.finish();
    tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
    let mut coalesced: Vec<Token> = Vec::new();
    for token in tokens {
        if let Token::Character(text) = &token
            && let Some(Token::Character(previous)) = coalesced.last_mut()
        {
            previous.push_str(text);
        } else {
            coalesced.push(token);
        }
    }
    (
        coalesced,
        tokenizer.errors().to_vec(),
        tokenizer.consumed_characters(),
    )
}

fn start_tag(tokens: &[Token]) -> &TagToken {
    let Token::StartTag(tag) = &tokens[0] else {
        panic!("expected start tag, got {tokens:?}");
    };
    tag
}

#[test]
fn core_states_preserve_tag_attributes_and_self_closing_metadata() {
    let (tokens, errors, _) = tokenize("<X-Ü A=one b='two >' C=\"世界\" disabled />", 1);
    let tag = start_tag(&tokens);
    assert_eq!(tag.name, "x-Ü");
    assert_eq!(
        tag.attributes,
        [
            TagAttribute {
                name: "a".into(),
                value: "one".into()
            },
            TagAttribute {
                name: "b".into(),
                value: "two >".into()
            },
            TagAttribute {
                name: "c".into(),
                value: "世界".into()
            },
            TagAttribute {
                name: "disabled".into(),
                value: String::new()
            },
        ]
    );
    assert!(tag.self_closing);
    assert!(errors.is_empty());
    assert_eq!(tokens.last(), Some(&Token::Eof));
    let (tokens, errors, _) = tokenize("<img src=x/>", 1);
    assert!(!start_tag(&tokens).self_closing);
    assert_eq!(start_tag(&tokens).attributes[0].value, "x/");
    assert!(errors.is_empty());
}

#[test]
fn duplicate_attributes_keep_the_first_value_with_normalized_names() {
    let (tokens, errors, _) = tokenize("<p A=first a='ignored' B=second>", 1);
    let tag = start_tag(&tokens);
    assert_eq!(tag.attributes.len(), 2);
    assert_eq!(tag.attributes[0].value, "first");
    assert_eq!(tag.attributes[1].value, "second");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "duplicate-attribute");
    assert_eq!(errors[0].position.column, 13);
    let (_, errors, _) = tokenize("<p a a", 1);
    assert_eq!(
        errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["duplicate-attribute", "eof-in-tag"]
    );
}

#[test]
fn malformed_attribute_characters_are_recovered_not_rejected() {
    let (tokens, errors, _) = tokenize("<p =x \"a\"='v' b=`foo` c=>", 1);
    let tag = start_tag(&tokens);
    assert_eq!(tag.attributes[0].name, "=x");
    assert_eq!(tag.attributes[1].name, "\"a\"");
    assert_eq!(tag.attributes[2].value, "`foo`");
    assert_eq!(tag.attributes[3].value, "");
    assert_eq!(
        errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [
            "unexpected-equals-sign-before-attribute-name",
            "unexpected-character-in-attribute-name",
            "unexpected-character-in-attribute-name",
            "unexpected-character-in-unquoted-attribute-value",
            "unexpected-character-in-unquoted-attribute-value",
            "missing-attribute-value",
        ]
    );
}

#[test]
fn reconsumption_and_errors_are_identical_for_all_splits() {
    for source in [
        "<",
        "</",
        "<>",
        "</>",
        "<<x a='b'c=d / q>",
        "<p a b",
        "<p a=",
        "<p a='",
        "<p a=\"",
        "<p a=x",
        "<p a='x'",
        "<p a='x'/",
        "</x a=b/>",
        "a\0b\r\n< X>\u{FFFF}",
        "<p\0 A\0='x\0'>",
        "<p = x /x>",
        "<!DOCTYPE html><!--ignored-->text",
    ] {
        let expected = tokenize(source, usize::MAX);
        for chunk_size in 1..=source.chars().count().max(1) {
            assert_eq!(
                tokenize(source, chunk_size),
                expected,
                "{source:?}, size {chunk_size}"
            );
        }
        assert_eq!(expected.2, source.chars().count());
        assert_eq!(
            expected
                .0
                .iter()
                .filter(|token| **token == Token::Eof)
                .count(),
            1
        );
    }
}

#[test]
fn null_handling_depends_on_the_state() {
    let (tokens, errors, _) = tokenize("\0<X\0 A\0='b\0'>", 1);
    assert_eq!(tokens[0], Token::Character("\0".into()));
    let Token::StartTag(tag) = &tokens[1] else {
        panic!("expected tag");
    };
    assert_eq!(tag.name, "x\u{FFFD}");
    assert_eq!(tag.attributes[0].name, "a\u{FFFD}");
    assert_eq!(tag.attributes[0].value, "b\u{FFFD}");
    assert_eq!(errors.len(), 4);
}

#[test]
fn positions_use_normalized_lines_and_original_utf8_byte_offsets() {
    let (_, errors, _) = tokenize("é\r\n<\0", 1);
    assert_eq!(
        errors[0].position,
        SourcePosition {
            byte_offset: 5,
            line: 2,
            column: 2
        }
    );
    assert_eq!(errors[0].code, "invalid-first-character-of-tag-name");
    assert_eq!(errors[1].position, errors[0].position);
    let (_, errors, _) = tokenize("é\r<p a='x", 1);
    assert_eq!(
        errors[0].position,
        SourcePosition {
            byte_offset: 10,
            line: 2,
            column: 8
        }
    );
}

#[test]
fn split_large_attribute_and_name_are_consumed_once() {
    for (prefix, suffix) in [("<p title=\"", "\">"), ("<p ", "=value>")] {
        let source = format!("{prefix}{}{suffix}", "x".repeat(1024 * 1024));
        let mut tokenizer = Tokenizer::new();
        let mut tokens = Vec::new();
        for chunk in source.as_bytes().chunks(127) {
            tokenizer.feed(std::str::from_utf8(chunk).unwrap());
            tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
        }
        tokenizer.finish();
        tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
        assert_eq!(tokenizer.consumed_characters(), source.len());
        assert_eq!(tokens.len(), 2);
        assert!(tokenizer.errors().is_empty());
        let tag = start_tag(&tokens);
        if prefix.ends_with('"') {
            assert_eq!(tag.attributes[0].value.len(), 1024 * 1024);
        } else {
            assert_eq!(tag.attributes[0].name.len(), 1024 * 1024);
        }
    }
}

#[test]
fn error_budget_is_bounded_and_truncation_is_explicit() {
    let mut tokenizer = Tokenizer::new();
    tokenizer.feed(&"\0".repeat(MAX_PARSE_ERRORS + 10));
    tokenizer.finish();
    while tokenizer.next_token().is_some() {}
    assert_eq!(tokenizer.errors().len(), MAX_PARSE_ERRORS);
    assert_eq!(tokenizer.dropped_errors(), 10);
}

#[test]
fn rawtext_switching_preserves_style_content_at_all_boundaries() {
    let source = "a < b </stylex> </STYLE attr='x'>tail";
    for size in 1..=source.len() {
        let mut tokenizer = Tokenizer::with_state(InitialState::Rawtext, Some("style"));
        let mut text = String::new();
        let mut end = None;
        for chunk in source.as_bytes().chunks(size) {
            tokenizer.feed(std::str::from_utf8(chunk).unwrap());
            while let Some(token) = tokenizer.next_token() {
                match token {
                    Token::Character(part) => text.push_str(&part),
                    Token::EndTag(tag) => end = Some(tag),
                    _ => panic!("unexpected token"),
                }
            }
        }
        tokenizer.finish();
        while let Some(token) = tokenizer.next_token() {
            match token {
                Token::Character(part) => text.push_str(&part),
                Token::Eof => {}
                _ => panic!("unexpected EOF recovery"),
            }
        }
        assert_eq!(text, "a < b </stylex> tail");
        assert_eq!(end.unwrap().name, "style");
    }
}
