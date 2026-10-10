use super::*;

fn lex(source: &str) -> (Vec<Token>, Vec<ParseError>, usize) {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let tokens = std::iter::from_fn(|| tokenizer.next_token()).collect();
    (
        tokens,
        tokenizer.errors().to_vec(),
        tokenizer.dropped_errors(),
    )
}

fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source).0.into_iter().map(|token| token.kind).collect()
}

fn numeric(source: &str, kind: NumericKind) -> Numeric {
    Numeric {
        representation: source.into(),
        kind,
    }
}

#[test]
fn emits_all_default_syntax_token_kinds_without_grouping_blocks() {
    assert_eq!(
        kinds("a f( @m #id #12 's' url(x) 1 +2.5% -3e2px \t<!-- -->:;,[](){}!"),
        vec![
            TokenKind::Ident("a".into()),
            TokenKind::Whitespace,
            TokenKind::Function("f".into()),
            TokenKind::Whitespace,
            TokenKind::AtKeyword("m".into()),
            TokenKind::Whitespace,
            TokenKind::Hash {
                value: "id".into(),
                kind: HashKind::Id
            },
            TokenKind::Whitespace,
            TokenKind::Hash {
                value: "12".into(),
                kind: HashKind::Unrestricted
            },
            TokenKind::Whitespace,
            TokenKind::String("s".into()),
            TokenKind::Whitespace,
            TokenKind::Url("x".into()),
            TokenKind::Whitespace,
            TokenKind::Number(numeric("1", NumericKind::Integer)),
            TokenKind::Whitespace,
            TokenKind::Percentage(numeric("+2.5", NumericKind::Number)),
            TokenKind::Whitespace,
            TokenKind::Dimension {
                number: numeric("-3e2", NumericKind::Number),
                unit: "px".into()
            },
            TokenKind::Whitespace,
            TokenKind::Cdo,
            TokenKind::Whitespace,
            TokenKind::Cdc,
            TokenKind::Colon,
            TokenKind::Semicolon,
            TokenKind::Comma,
            TokenKind::OpenSquare,
            TokenKind::CloseSquare,
            TokenKind::OpenParen,
            TokenKind::CloseParen,
            TokenKind::OpenCurly,
            TokenKind::CloseCurly,
            TokenKind::Delim('!'),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn comments_disappear_without_joining_adjacent_tokens() {
    assert_eq!(
        kinds("a/**/b 1/**/px #/**/id /****/"),
        vec![
            TokenKind::Ident("a".into()),
            TokenKind::Ident("b".into()),
            TokenKind::Whitespace,
            TokenKind::Number(numeric("1", NumericKind::Integer)),
            TokenKind::Ident("px".into()),
            TokenKind::Whitespace,
            TokenKind::Delim('#'),
            TokenKind::Ident("id".into()),
            TokenKind::Whitespace,
            TokenKind::Eof,
        ]
    );
    assert_eq!(lex("/*").1[0].code, "eof-in-comment");
    assert_eq!(lex("/*").1[0].position.byte_offset, 2);
}

#[test]
fn preprocessing_preserves_original_utf8_offsets_and_normalized_positions() {
    let (tokens, errors, _) = lex("\u{e9}\r\n\0\x0cX\rY");
    assert!(errors.is_empty());
    assert_eq!(
        tokens[0].span.start,
        SourcePosition {
            byte_offset: 0,
            line: 1,
            column: 1
        }
    );
    assert_eq!(
        tokens[0].span.end,
        SourcePosition {
            byte_offset: 2,
            line: 1,
            column: 2
        }
    );
    assert_eq!(
        tokens[1].span.end,
        SourcePosition {
            byte_offset: 4,
            line: 2,
            column: 1
        }
    );
    assert_eq!(tokens[2].kind, TokenKind::Ident("\u{fffd}".into()));
    assert_eq!(
        tokens[2].span.end,
        SourcePosition {
            byte_offset: 5,
            line: 2,
            column: 2
        }
    );
    assert_eq!(
        tokens[4].span.start,
        SourcePosition {
            byte_offset: 6,
            line: 3,
            column: 1
        }
    );
    assert_eq!(
        tokens.last().unwrap().span.end,
        SourcePosition {
            byte_offset: 9,
            line: 4,
            column: 2
        }
    );
}

#[test]
fn escape_decoding_handles_six_digits_invalid_scalars_and_eof() {
    for (source, value) in [
        ("\\30 red", "0red"),
        ("\\000030red", "0red"),
        ("\\30\r\nred", "0red"),
        ("\\000000x", "\u{fffd}x"),
        ("\\d800x", "\u{fffd}x"),
        ("\\110000x", "\u{fffd}x"),
        ("\\1f600 ", "\u{1f600}"),
        ("\\+x", "+x"),
        ("a\\", "a\u{fffd}"),
        ("\\", "\u{fffd}"),
    ] {
        assert_eq!(
            kinds(source),
            [TokenKind::Ident(value.into()), TokenKind::Eof],
            "{source:?}"
        );
    }
    assert_eq!(lex("a\\").1[0].code, "eof-in-escape");
    assert_eq!(lex("\\\nx").1[0].code, "invalid-escape");
}

#[test]
fn identifiers_use_current_non_ascii_ranges_but_allow_escaped_code_points() {
    for ch in [
        '\u{b7}',
        '\u{c0}',
        '\u{d6}',
        '\u{d8}',
        '\u{f6}',
        '\u{f8}',
        '\u{37d}',
        '\u{37f}',
        '\u{1fff}',
        '\u{200c}',
        '\u{200d}',
        '\u{203f}',
        '\u{2040}',
        '\u{2070}',
        '\u{218f}',
        '\u{2c00}',
        '\u{2fef}',
        '\u{3001}',
        '\u{d7ff}',
        '\u{f900}',
        '\u{fdcf}',
        '\u{fdf0}',
        '\u{fffd}',
        '\u{10000}',
        '\u{10ffff}',
    ] {
        assert_eq!(kinds(&ch.to_string())[0], TokenKind::Ident(ch.to_string()));
    }
    for ch in [
        '\u{80}', '\u{81}', '\u{a0}', '\u{bf}', '\u{d7}', '\u{f7}', '\u{37e}', '\u{2000}',
        '\u{200b}', '\u{200e}', '\u{203e}', '\u{2041}', '\u{206f}', '\u{2190}', '\u{2bff}',
        '\u{2ff0}', '\u{3000}', '\u{e000}', '\u{f8ff}', '\u{fdd0}', '\u{fdef}', '\u{fffe}',
        '\u{ffff}',
    ] {
        assert_eq!(kinds(&ch.to_string())[0], TokenKind::Delim(ch));
    }
    assert_eq!(kinds("\\80 ")[0], TokenKind::Ident("\u{80}".into()));
}

#[test]
fn numeric_spelling_type_sign_and_suffix_are_preserved() {
    for (source, value, kind, sign) in [
        ("12", 12.0_f64, NumericKind::Integer, None),
        ("+0012", 12.0, NumericKind::Integer, Some('+')),
        ("-0", -0.0, NumericKind::Integer, Some('-')),
        (".67", 0.67, NumericKind::Number, None),
        ("+.89", 0.89, NumericKind::Number, Some('+')),
        ("12.0", 12.0, NumericKind::Number, None),
        ("-45E-0", -45.0, NumericKind::Number, Some('-')),
        ("12e2", 1200.0, NumericKind::Number, None),
    ] {
        let TokenKind::Number(number) = &kinds(source)[0] else {
            panic!("{source}")
        };
        assert_eq!(number.representation, source);
        assert_eq!(number.kind, kind);
        assert_eq!(number.sign(), sign);
        assert_eq!(number.value().unwrap().to_bits(), value.to_bits());
    }
    assert!(
        matches!(&kinds("0e999999")[0], TokenKind::Number(number) if number.value().unwrap() == 0.0)
    );
    assert!(
        matches!(&kinds("1e999999")[0], TokenKind::Number(number) if number.value().unwrap() == f64::INFINITY)
    );
    assert!(
        matches!(&kinds("3\\65-2")[0], TokenKind::Dimension { number, unit } if number.representation == "3" && unit == "e-2")
    );
    assert!(matches!(&kinds("12\\%")[0], TokenKind::Dimension { unit, .. } if unit == "%"));
    assert_eq!(
        kinds("3. 3e+"),
        [
            TokenKind::Number(numeric("3", NumericKind::Integer)),
            TokenKind::Delim('.'),
            TokenKind::Whitespace,
            TokenKind::Dimension {
                number: numeric("3", NumericKind::Integer),
                unit: "e".into()
            },
            TokenKind::Delim('+'),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn string_newlines_recover_without_consuming_the_newline() {
    let (tokens, errors, _) = lex("'a\r\nb");
    assert_eq!(tokens[0].kind, TokenKind::BadString);
    assert_eq!(tokens[0].span.end.byte_offset, 2);
    assert_eq!(tokens[1].kind, TokenKind::Whitespace);
    assert_eq!(tokens[2].kind, TokenKind::Ident("b".into()));
    assert_eq!(errors[0].code, "newline-in-string");
    assert_eq!(errors[0].position.byte_offset, 2);
    assert_eq!(kinds("'a\\\r\nb'")[0], TokenKind::String("ab".into()));
    assert_eq!(kinds("\"a\\")[0], TokenKind::String("a".into()));
    assert_eq!(lex("\"a\\").1[0].code, "eof-in-string");
}

#[test]
fn quoted_url_detection_preserves_one_whitespace_token() {
    assert_eq!(
        kinds("URL( \t 'x')"),
        [
            TokenKind::Function("URL".into()),
            TokenKind::Whitespace,
            TokenKind::String("x".into()),
            TokenKind::CloseParen,
            TokenKind::Eof,
        ]
    );
    assert_eq!(kinds("u\\72l(a\\ b)")[0], TokenKind::Url("a b".into()));
    assert_eq!(kinds("url(\t a \t)")[0], TokenKind::Url("a".into()));
    assert_eq!(kinds("url(")[0], TokenKind::Url(String::new()));
    assert_eq!(lex("url(").1[0].code, "eof-in-url");
}

#[test]
fn bad_url_remnants_skip_escaped_closing_parentheses() {
    for source in [
        "url(a b\\)c) after",
        "url(a\"b\\29 c) after",
        "url(a\\\nb) after",
    ] {
        let (tokens, errors, _) = lex(source);
        assert_eq!(tokens[0].kind, TokenKind::BadUrl, "{source:?}");
        assert_eq!(tokens[1].kind, TokenKind::Whitespace);
        assert_eq!(tokens[2].kind, TokenKind::Ident("after".into()));
        assert_eq!(errors[0].code, "invalid-url");
    }
}

#[test]
fn eof_is_emitted_once_and_finish_is_idempotent() {
    let mut tokenizer = Tokenizer::default();
    assert!(tokenizer.next_token().is_none());
    tokenizer.feed("{");
    assert_eq!(tokenizer.next_token().unwrap().kind, TokenKind::OpenCurly);
    assert!(tokenizer.next_token().is_none());
    tokenizer.finish();
    tokenizer.finish();
    assert_eq!(tokenizer.next_token().unwrap().kind, TokenKind::Eof);
    assert!(tokenizer.next_token().is_none());
}

#[test]
fn every_lexical_state_survives_every_utf8_chunk_boundary() {
    for source in [
        "/*/**/*/ --><!-- @\\30 a #0id -\\31 .5e+2rem ~=|=^=$=*=||",
        "\"a\\\r\nb\\000041 \" 'bad\ntext url(foo\\29 bar) url(a b\\)c)",
        "url( \t 'quoted') URL( ) url(a\\)b) url(a\\\nb) '\\",
        "\\\r\n \\abcdef0 \\110000x \u{e9}\r\n\0\u{1f600}\x0c",
        "-",
        "--",
        "<!-",
        "/",
        "#",
        "@",
        "1.",
        "1e",
        "1e+",
        "a\\",
        "url(",
    ] {
        let expected = lex(source);
        for split in source
            .char_indices()
            .map(|(index, _)| index)
            .chain([source.len()])
        {
            let mut tokenizer = Tokenizer::default();
            let mut tokens = Vec::new();
            tokenizer.feed(&source[..split]);
            tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
            assert!(!tokens.iter().any(|token| token.kind == TokenKind::Eof));
            tokenizer.feed(&source[split..]);
            tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
            tokenizer.finish();
            tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
            assert_eq!(
                (
                    tokens,
                    tokenizer.errors().to_vec(),
                    tokenizer.dropped_errors()
                ),
                expected,
                "{source:?} at {split}"
            );
        }
    }
}

#[test]
fn long_tokens_comments_and_compacted_input_remain_chunk_invariant() {
    let source = format!(
        "{} /*{}*/ \"{}\" {}px",
        "a".repeat(10000),
        "*".repeat(10000),
        "x".repeat(10000),
        "1".repeat(10000)
    );
    let expected = lex(&source);
    let mut tokenizer = Tokenizer::default();
    let mut tokens = Vec::new();
    for chunk in source.as_bytes().chunks(7) {
        tokenizer.feed(std::str::from_utf8(chunk).unwrap());
        tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
    }
    tokenizer.finish();
    tokens.extend(std::iter::from_fn(|| tokenizer.next_token()));
    assert_eq!(
        (
            tokens,
            tokenizer.errors().to_vec(),
            tokenizer.dropped_errors()
        ),
        expected
    );
}

#[test]
fn diagnostics_are_bounded_without_truncating_the_token_stream() {
    let source = "\\\n".repeat(MAX_PARSE_ERRORS + 10);
    let (tokens, errors, dropped) = lex(&source);
    assert_eq!(errors.len(), MAX_PARSE_ERRORS);
    assert_eq!(dropped, 10);
    assert_eq!(tokens.len(), (MAX_PARSE_ERRORS + 10) * 2 + 1);
    assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
}
