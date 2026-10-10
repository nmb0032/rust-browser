#![no_main]
use libfuzzer_sys::fuzz_target;
use rust_browser::html::{
    encoding::HtmlByteParser,
    parser::{FragmentContext, HtmlParser},
    tokenizer::{InitialState, ParseError, Token, Utf16Tokenizer},
};

fn tokenize_utf16(
    units: &[u16],
    state: InitialState,
    size: usize,
) -> (Vec<Token<Vec<u16>>>, Vec<ParseError>, usize) {
    let mut tokenizer = Utf16Tokenizer::with_state(state, None);
    let mut tokens = Vec::new();
    for chunk in units.chunks(size) {
        tokenizer.feed(chunk);
        drain(&mut tokenizer, &mut tokens);
    }
    tokenizer.finish();
    drain(&mut tokenizer, &mut tokens);
    (
        tokens,
        tokenizer.errors().to_vec(),
        tokenizer.dropped_errors(),
    )
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

fuzz_target!(|bytes: &[u8]| {
    let mut whole = HtmlByteParser::new(None);
    whole.feed(bytes).unwrap();
    let whole = whole.finish().unwrap();
    let mut chunked = HtmlByteParser::new(None);
    let size = bytes.first().map_or(1, |byte| usize::from(*byte) + 1);
    for chunk in bytes.chunks(size) {
        chunked.feed(chunk).unwrap();
        chunked.document().validate().unwrap();
    }
    let chunked = chunked.finish().unwrap();
    whole.parsed.document.validate().unwrap();
    chunked.parsed.document.validate().unwrap();
    assert_eq!(
        format!("{:?}", whole.parsed.document),
        format!("{:?}", chunked.parsed.document)
    );
    assert_eq!(whole.parsed.errors, chunked.parsed.errors);
    assert_eq!(whole.parsed.dropped_errors, chunked.parsed.dropped_errors);
    assert_eq!(whole.had_replacements, chunked.had_replacements);
    let text = String::from_utf8_lossy(bytes);
    for name in ["div", "table", "textarea", "template", "select"] {
        let mut fragment = HtmlParser::fragment(FragmentContext::html(name)).unwrap();
        fragment.feed(&text).unwrap();
        fragment.finish().unwrap().validate().unwrap();
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let states = [
        InitialState::Data,
        InitialState::Rcdata,
        InitialState::Rawtext,
        InitialState::ScriptData,
        InitialState::Plaintext,
        InitialState::Cdata,
    ];
    let state = states[bytes.first().map_or(0, |byte| usize::from(*byte)) % states.len()];
    assert_eq!(
        tokenize_utf16(&units, state, usize::MAX),
        tokenize_utf16(&units, state, size)
    );
});
