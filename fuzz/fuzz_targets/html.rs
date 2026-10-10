#![no_main]
use libfuzzer_sys::fuzz_target;
use rust_browser::html::{
    encoding::HtmlByteParser,
    parser::{FragmentContext, HtmlParser},
};

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
});
