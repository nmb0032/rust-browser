use rust_browser::css::tokenizer::{HashKind, Numeric, NumericKind, Token, TokenKind, Tokenizer};
use serde_json::{Value, json};

fn numeric(name: &str, number: &Numeric) -> Value {
    json!([
        name,
        number.value().unwrap(),
        match number.kind {
            NumericKind::Integer => "integer",
            NumericKind::Number => "number",
        }
    ])
}

fn component_values(tokens: &[Token]) -> Value {
    let mut stack = vec![(Vec::new(), TokenKind::Eof)];
    for token in tokens {
        let value = match &token.kind {
            TokenKind::Function(name) => {
                stack.push((vec![json!("function"), json!(name)], TokenKind::CloseParen));
                continue;
            }
            TokenKind::OpenParen | TokenKind::OpenSquare | TokenKind::OpenCurly => {
                let (name, closing) = match token.kind {
                    TokenKind::OpenParen => ("()", TokenKind::CloseParen),
                    TokenKind::OpenSquare => ("[]", TokenKind::CloseSquare),
                    TokenKind::OpenCurly => ("{}", TokenKind::CloseCurly),
                    _ => unreachable!(),
                };
                stack.push((vec![json!(name)], closing));
                continue;
            }
            TokenKind::CloseParen | TokenKind::CloseSquare | TokenKind::CloseCurly => {
                if stack.last().unwrap().1 == token.kind {
                    let (values, _) = stack.pop().unwrap();
                    stack.last_mut().unwrap().0.push(Value::Array(values));
                    continue;
                }
                json!([
                    "error",
                    match token.kind {
                        TokenKind::CloseParen => ")",
                        TokenKind::CloseSquare => "]",
                        TokenKind::CloseCurly => "}",
                        _ => unreachable!(),
                    }
                ])
            }
            TokenKind::Ident(value) => json!(["ident", value]),
            TokenKind::AtKeyword(value) => json!(["at-keyword", value]),
            TokenKind::Hash { value, kind } => json!([
                "hash",
                value,
                match kind {
                    HashKind::Id => "id",
                    HashKind::Unrestricted => "unrestricted",
                }
            ]),
            TokenKind::String(value) => json!(["string", value]),
            TokenKind::Url(value) => json!(["url", value]),
            TokenKind::BadString => json!(["error", "bad-string"]),
            TokenKind::BadUrl => json!(["error", "bad-url"]),
            TokenKind::Number(number) => numeric("number", number),
            TokenKind::Percentage(number) => numeric("percentage", number),
            TokenKind::Dimension { number, unit } => {
                let mut value = numeric("dimension", number).as_array().unwrap().clone();
                value.push(json!(unit));
                Value::Array(value)
            }
            TokenKind::Delim(ch) => json!(ch.to_string()),
            TokenKind::Whitespace => json!(" "),
            TokenKind::Cdo => json!("<!--"),
            TokenKind::Cdc => json!("-->"),
            TokenKind::Colon => json!(":"),
            TokenKind::Semicolon => json!(";"),
            TokenKind::Comma => json!(","),
            TokenKind::Eof => break,
        };
        stack.last_mut().unwrap().0.push(value);
    }
    while stack.len() > 1 {
        let (values, _) = stack.pop().unwrap();
        stack.last_mut().unwrap().0.push(Value::Array(values));
    }
    Value::Array(stack.pop().unwrap().0)
}

fn normalize_list(values: &[Value]) -> Vec<Value> {
    let mut result = Vec::new();
    for value in values {
        if matches!(value.as_str(), Some("~=" | "|=" | "^=" | "$=" | "*=")) {
            result.extend(
                value
                    .as_str()
                    .unwrap()
                    .chars()
                    .map(|ch| json!(ch.to_string())),
            );
        } else if let Some(array) = value.as_array() {
            let first = array[0].as_str().unwrap();
            result.push(match first {
                "number" | "percentage" | "dimension" => {
                    let mut number = vec![
                        array[0].clone(),
                        json!(array[2].as_f64().unwrap()),
                        array[3].clone(),
                    ];
                    number.extend_from_slice(&array[4..]);
                    Value::Array(number)
                }
                "function" | "()" | "[]" | "{}" => {
                    let prefix = if first == "function" { 2 } else { 1 };
                    let mut values = array[..prefix].to_vec();
                    values.extend(normalize_list(&array[prefix..]));
                    Value::Array(values)
                }
                _ => value.clone(),
            });
        } else {
            result.push(value.clone());
        }
    }
    result
}

fn tokenize(source: &str) -> (Vec<Token>, Vec<rust_browser::css::tokenizer::ParseError>) {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let tokens = std::iter::from_fn(|| tokenizer.next_token()).collect();
    (tokens, tokenizer.errors().to_vec())
}

#[test]
fn all_upstream_token_cases_match_current_syntax_and_every_chunk_boundary() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/css-parsing/component_value_list.json"
    ))
    .unwrap();
    let (pairs, remainder) = fixtures.as_chunks::<2>();
    assert!(remainder.is_empty());
    assert_eq!(pairs.len(), 44);
    let mut updated_expectations = 0;
    for (index, pair) in pairs.iter().enumerate() {
        let source = pair[0].as_str().unwrap();
        let (tokens, errors) = tokenize(source);
        let raw_expected = Value::Array(normalize_list(pair[1].as_array().unwrap()));
        let mut expected = raw_expected.clone();
        if source.ends_with("\u{7f}\u{80}\u{81}") {
            let values = expected.as_array_mut().unwrap();
            assert_eq!(values.pop().unwrap(), json!(["ident", "\u{80}\u{81}"]));
            values.extend([json!("\u{80}"), json!("\u{81}")]);
            assert_ne!(component_values(&tokens), raw_expected);
            updated_expectations += 1;
        }
        assert_eq!(
            component_values(&tokens),
            expected,
            "fixture {index}: {source:?}"
        );
        for split in source
            .char_indices()
            .map(|(index, _)| index)
            .chain([source.len()])
        {
            let mut tokenizer = Tokenizer::default();
            let mut streamed = Vec::new();
            tokenizer.feed(&source[..split]);
            streamed.extend(std::iter::from_fn(|| tokenizer.next_token()));
            assert!(!streamed.iter().any(|token| token.kind == TokenKind::Eof));
            tokenizer.feed(&source[split..]);
            streamed.extend(std::iter::from_fn(|| tokenizer.next_token()));
            tokenizer.finish();
            streamed.extend(std::iter::from_fn(|| tokenizer.next_token()));
            assert_eq!(streamed, tokens, "fixture {index}, split {split}");
            assert_eq!(tokenizer.errors(), errors, "fixture {index}, split {split}");
        }
    }
    assert_eq!(updated_expectations, 1);
    println!(
        "{} upstream token cases passed; one audited current-spec expectation update",
        fixtures.len() / 2
    );
}
