use std::{error::Error, fs, path::Path};

use rust_browser::html::tokenizer::{InitialState, Token, Tokenizer};
use serde_json::{Value, json};

use super::{Counts, chunks, files};

#[derive(Debug, PartialEq)]
struct Outcome {
    tokens: Vec<Value>,
    errors: Vec<Value>,
    dropped_errors: usize,
}

pub(super) fn run(path: &Path) -> Result<(Counts, Counts), Box<dyn Error>> {
    let mut counts = Counts::default();
    let mut core = Counts::default();
    let mut scalar_gate = 0;
    let mut obsolete_pi = 0;
    for file in files(path, "test")? {
        if file
            .file_name()
            .is_some_and(|name| name == "xmlViolation.test")
        {
            continue;
        }
        let fixture: Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
        let cases = fixture["tests"].as_array().ok_or("missing tests array")?;
        for case in cases {
            let states = case["initialStates"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![json!("Data state")]);
            for initial in states {
                counts.total += 1;
                let state = match initial.as_str().ok_or("invalid initial state")? {
                    "Data state" => InitialState::Data,
                    "RAWTEXT state" => InitialState::Rawtext,
                    "PLAINTEXT state" => InitialState::Plaintext,
                    "RCDATA state" => InitialState::Rcdata,
                    "Script data state" => InitialState::ScriptData,
                    "CDATA section state" => InitialState::Cdata,
                    state => return Err(format!("unknown initial state: {state}").into()),
                };
                let mut source = case["input"]
                    .as_str()
                    .ok_or("missing tokenizer input")?
                    .to_owned();
                let mut expected = case["output"].clone();
                if !expected.is_array() {
                    return Err("missing tokenizer output".into());
                }
                if case["doubleEscaped"].as_bool() == Some(true) {
                    match unescape(&source).and_then(|input| {
                        unescape_value(&mut expected)?;
                        Ok(input)
                    }) {
                        Ok(input) => source = input,
                        Err(UnescapeError::NonScalar) => {
                            counts.unsupported += 1;
                            continue;
                        }
                        Err(UnescapeError::Malformed) => {
                            return Err("malformed double escape".into());
                        }
                    }
                }
                let last_tag = case["lastStartTag"].as_str();
                let whole = tokenize(&source, state, last_tag, usize::MAX);
                for size in [1, 7, 64] {
                    let chunked = tokenize(&source, state, last_tag, size);
                    assert_eq!(
                        chunked,
                        whole,
                        "{}: {} ({initial}, chunk {size})",
                        file.display(),
                        case["description"]
                    );
                }
                let shape_matches = Value::Array(whole.tokens.clone()) == expected;
                counts.shape_passed += usize::from(shape_matches);
                let errors = case["errors"].as_array().cloned().unwrap_or_default();
                let matches = shape_matches && whole.errors == errors && whole.dropped_errors == 0;
                if errors
                    .iter()
                    .any(|error| error["code"] == "unexpected-question-mark-instead-of-tag-name")
                {
                    obsolete_pi += 1;
                } else {
                    assert!(
                        matches,
                        "scalar tokenizer regression: {}: {}\nexpected {expected}, errors {errors:?}\nactual {whole:?}",
                        file.display(),
                        case["description"]
                    );
                    scalar_gate += 1;
                }
                if matches {
                    counts.passed += 1;
                }
                if !matches && std::env::var_os("HTML5LIB_FAILURES").is_some() {
                    eprintln!(
                        "{}: {} ({initial})\nexpected {expected}, errors {errors:?}\nactual {whole:?}",
                        file.display(),
                        case["description"]
                    );
                }
                let core_case = state == InitialState::Data
                    && !source.contains('&')
                    && !source.contains("<!")
                    && expected
                        .as_array()
                        .expect("checked array")
                        .iter()
                        .all(|token| {
                            matches!(token[0].as_str(), Some("StartTag" | "EndTag" | "Character"))
                        });
                if core_case {
                    core.total += 1;
                    core.shape_passed += usize::from(shape_matches);
                    core.passed += usize::from(matches);
                    assert!(
                        matches,
                        "core tokenizer regression: {}: {}\nexpected {expected}, errors {errors:?}\nactual {whole:?}",
                        file.display(),
                        case["description"]
                    );
                }
            }
        }
    }
    if counts.total == 0 {
        return Err("no tokenizer cases ran".into());
    }
    assert!(core.total > 0, "no core tokenizer cases ran");
    println!(
        "Exact scalar tokenizer regression gate: {scalar_gate} cases; {obsolete_pi} old PI expectations retained in raw baseline."
    );
    Ok((counts, core))
}

fn tokenize(source: &str, state: InitialState, last_tag: Option<&str>, size: usize) -> Outcome {
    let mut tokenizer = Tokenizer::with_state(state, last_tag);
    let mut tokens = Vec::new();
    for chunk in chunks(source, size) {
        tokenizer.feed(chunk);
        drain(&mut tokenizer, &mut tokens);
    }
    tokenizer.finish();
    drain(&mut tokenizer, &mut tokens);
    Outcome {
        tokens,
        errors: tokenizer
            .errors()
            .iter()
            .map(|error| {
                json!({
                    "code": error.code,
                    "line": error.position.line,
                    "col": error.position.column,
                })
            })
            .collect(),
        dropped_errors: tokenizer.dropped_errors(),
    }
}

fn drain(tokenizer: &mut Tokenizer, tokens: &mut Vec<Value>) {
    while let Some(token) = tokenizer.next_token() {
        let output = match token {
            Token::StartTag(tag) => {
                let attributes = tag
                    .attributes
                    .into_iter()
                    .map(|attribute| (attribute.name, Value::String(attribute.value)))
                    .collect::<serde_json::Map<_, _>>();
                if tag.self_closing {
                    json!(["StartTag", tag.name, attributes, true])
                } else {
                    json!(["StartTag", tag.name, attributes])
                }
            }
            Token::EndTag(tag) => json!(["EndTag", tag.name]),
            Token::Character(text) => {
                if let Some(Value::Array(last)) = tokens.last_mut()
                    && last.first() == Some(&json!("Character"))
                {
                    let Value::String(previous) = &mut last[1] else {
                        panic!("character token");
                    };
                    previous.push_str(&text);
                    continue;
                }
                json!(["Character", text])
            }
            Token::Comment(text) => json!(["Comment", text]),
            Token::ProcessingInstruction { target, data } => {
                json!(["ProcessingInstruction", target, data])
            }
            Token::Doctype(data) => json!([
                "DOCTYPE",
                data.name,
                data.public_identifier,
                data.system_identifier,
                !data.force_quirks
            ]),
            Token::Eof => continue,
        };
        tokens.push(output);
    }
}

#[derive(Debug)]
enum UnescapeError {
    NonScalar,
    Malformed,
}

fn unescape(source: &str) -> Result<String, UnescapeError> {
    let mut output = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("\\u") {
        output.push_str(&rest[..start]);
        let digits = rest
            .get(start + 2..start + 6)
            .ok_or(UnescapeError::Malformed)?;
        let value = u32::from_str_radix(digits, 16).map_err(|_| UnescapeError::Malformed)?;
        output.push(char::from_u32(value).ok_or(UnescapeError::NonScalar)?);
        rest = &rest[start + 6..];
    }
    output.push_str(rest);
    Ok(output)
}

fn unescape_value(value: &mut Value) -> Result<(), UnescapeError> {
    match value {
        Value::String(text) => *text = unescape(text)?,
        Value::Array(items) => {
            for item in items {
                unescape_value(item)?;
            }
        }
        Value::Object(items) => {
            let mut unescaped = serde_json::Map::new();
            for (name, mut value) in std::mem::take(items) {
                unescape_value(&mut value)?;
                unescaped.insert(unescape(&name)?, value);
            }
            *items = unescaped;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_escapes_do_not_silently_replace_surrogates() {
        assert_eq!(unescape("x\\u0000\\u00E9").unwrap(), "x\0é");
        assert!(matches!(unescape("\\uD800"), Err(UnescapeError::NonScalar)));
        assert!(matches!(unescape("\\u12"), Err(UnescapeError::Malformed)));
        let mut output = json!(["StartTag", "p", {"\\u00E9": "\\uFFFF"}]);
        unescape_value(&mut output).unwrap();
        assert_eq!(output, json!(["StartTag", "p", {"é": "\u{FFFF}"}]));
    }
}
