#[path = "html5lib/mod.rs"]
mod html5lib;

#[test]
fn html5lib_strict_conformance_and_chunk_invariance() {
    html5lib::run().expect("html5lib fixtures must load and chunked results must agree");
}
