#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    match rust_browser::xml::parse_bytes(bytes, None) {
        Ok(document) => document.validate().unwrap(),
        Err(failure) => rust_browser::xml::error_document(&failure)
            .validate()
            .unwrap(),
    }
});
