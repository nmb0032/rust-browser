mod support;

use rust_browser::{
    dom::{Namespace, NodeKind},
    engine::{Frame, LoadOptions, load_page},
};
use support::{Reply, Server};

fn text(frame: &Frame) -> String {
    frame
        .document
        .preorder()
        .filter_map(|id| match frame.document.node(id)?.kind() {
            NodeKind::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn html_loading_obeys_transport_meta_bom_and_replacement_decoding() {
    for (mime, bytes, expected, encoding) in [
        (
            "text/html",
            b"<!doctype html><p>caf\xe9".to_vec(),
            "café",
            "windows-1252",
        ),
        (
            "text/html",
            b"<!doctype html><meta charset=utf-8><p>caf\xc3\xa9".to_vec(),
            "café",
            "UTF-8",
        ),
        (
            "text/html; charset=windows-1252",
            b"<!doctype html><meta charset=utf-8><p>caf\xe9".to_vec(),
            "café",
            "windows-1252",
        ),
        (
            "text/html; charset=windows-1252",
            b"\xef\xbb\xbf<!doctype html><p>caf\xc3\xa9".to_vec(),
            "café",
            "UTF-8",
        ),
        (
            "text/html; charset=utf-8",
            b"<!doctype html><p>\xff".to_vec(),
            "\u{FFFD}",
            "UTF-8",
        ),
    ] {
        let server = Server::new(vec![("/", Reply::ok(mime, bytes))], 1);
        let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
        assert_eq!(text(&frame), expected);
        assert_eq!(frame.document.encoding(), encoding);
        frame.document.validate().unwrap();
        server.finish();
    }
}

#[test]
fn xml_mime_preserves_namespaces_and_malformed_documents_show_errors() {
    for (mime, source, namespace) in [
        (
            "application/xhtml+xml",
            "<html xmlns='http://www.w3.org/1999/xhtml'><body><p>XML</p></body></html>",
            Namespace::Html,
        ),
        (
            "image/svg+xml",
            "<svg xmlns='http://www.w3.org/2000/svg'><text>XML</text></svg>",
            Namespace::Svg,
        ),
        (
            "application/example+xml; charset=utf-8",
            "<Root xmlns='urn:example'>XML</Root>",
            Namespace::Other("urn:example".into()),
        ),
    ] {
        let server = Server::new(
            vec![
                ("/", Reply::redirect("/document")),
                ("/document", Reply::ok(mime, source.as_bytes())),
            ],
            2,
        );
        let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
        let root = frame
            .document
            .node(
                frame
                    .document
                    .node(frame.document.root_id())
                    .unwrap()
                    .children()[0],
            )
            .unwrap()
            .as_element()
            .unwrap();
        assert_eq!(root.namespace(), &namespace);
        assert_eq!(text(&frame), "XML");
        assert!(frame.error.is_none());
        server.finish();
    }
    let server = Server::new(
        vec![("/", Reply::ok("image/svg+xml", b"<svg><g></svg>".to_vec()))],
        1,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(frame.error.is_some());
    assert!(text(&frame).contains("XML parsing error"));
    assert_eq!(frame.parse_errors[0].code, "xml-well-formedness-error");
    frame.document.validate().unwrap();
    server.finish();
}

#[test]
fn local_xml_extension_dispatch_and_parse_error_cli_are_wired() {
    let path = std::env::temp_dir().join(format!("rust-browser-xml-{}.xml", std::process::id()));
    std::fs::write(&path, "<Root xmlns='urn:test'>local</Root>").unwrap();
    let url = url::Url::from_file_path(&path).unwrap();
    let frame = load_page(&url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(text(&frame), "local");
    let server = Server::new(
        vec![("/", Reply::ok("text/html", b"<p><div/>".to_vec()))],
        1,
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=parse-errors", server.url.as_str()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("missing-doctype"));
    assert!(output.contains("non-void-html-element-start-tag-with-trailing-solidus"));
    assert!(output.contains("byte"));
    server.finish();
}
