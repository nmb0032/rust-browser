mod support;

use std::{path::Path, process::Command};

use support::{Reply, Server};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/styles.html"))
        .arg(format!("--dump={format}"))
        .output()
        .expect("browser process should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

#[test]
fn stylesheet_dump_preserves_document_and_declaration_order() {
    assert_eq!(
        dump("stylesheets"),
        include_str!("snapshots/stylesheets.expected")
    );
}

#[test]
fn css_token_dump_includes_stylesheets_inline_values_and_source_positions() {
    let output = dump("css-tokens");
    assert!(output.contains("Stylesheet (<style>, node"));
    assert!(output.contains("Inline style (node"));
    assert!(output.contains("Ident(\"color\")"));
    assert!(output.contains("Dimension {"));
    assert!(output.contains("Hash {"));
    assert!(output.contains("(bytes "));
    assert!(output.contains("Eof"));
    assert!(!output.contains("error "));
}

#[test]
fn css_token_dump_reports_the_exact_omitted_diagnostic_count() {
    let limit = rust_browser::css::tokenizer::MAX_PARSE_ERRORS;
    let output = rust_browser::devtools::format_css_tokens(&"\\\n".repeat(limit + 2));
    assert_eq!(output.matches("  error ").count(), limit);
    assert!(output.contains("2 additional CSS tokenization errors omitted"));
    assert!(output.contains("Eof"));
}

#[test]
fn css_token_dump_retains_recovered_sources_redirect_urls_and_cached_sheets() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<style>p { color: red }</style>\
                    <link rel='stylesheet' href='sheet.css'>\
                    <link rel='stylesheet' href='sheet.css'>\
                    <link rel='stylesheet' href='broken.css'>\
                    <style>p { content: \"embedded\nrest }</style>\
                    <style type='text/plain'>not-css</style>\
                    <p style='width: 2px'>tokens</p>"
                        .to_vec(),
                ),
            ),
            ("/sheet.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok("text/css", b"\xef\xbb\xbfp { margin: 3px }".to_vec()),
            ),
            (
                "/broken.css",
                Reply::ok("text/css", b"a { content: \"external".to_vec()),
            ),
        ],
        4,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=css-tokens", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let external = format!(
        "Stylesheet ({}, node",
        server.url.join("final.css").unwrap()
    );
    assert_eq!(stdout.matches(&external).count(), 2);
    assert!(stdout.contains(&format!(
        "Stylesheet ({}, node",
        server.url.join("broken.css").unwrap()
    )));
    let embedded = stdout.find("Stylesheet (<style>, node").unwrap();
    let external = stdout.find(&external).unwrap();
    let rejected = stdout.find("eof-in-string").unwrap();
    let last_embedded = stdout.rfind("Stylesheet (<style>, node").unwrap();
    assert!(embedded < external && external < rejected && rejected < last_embedded);
    assert!(stdout.contains("String(\"external\")"));
    assert!(stdout.contains("BadString"));
    assert!(stdout.contains("newline-in-string"));
    assert!(stdout.contains("Inline style (node"));
    assert!(!stdout.contains("not-css"));
    assert!(!stdout.contains("\\u{feff}"));
    assert!(stderr.contains("CSS/resource"));
    assert!(stderr.contains("broken.css"));
    assert_eq!(
        server.finish(),
        ["/", "/sheet.css", "/final.css", "/broken.css"]
    );
}

#[test]
fn styled_layout_and_paint_are_wired_to_the_cli() {
    let layout = dump("layout");
    assert!(layout.contains("<span> Block"));
    assert!(!layout.contains("Hidden content."));
    let paint = dump("display-list");
    assert!(
        paint.contains("color: 13382400"),
        "inline #c30 color should reach paint"
    );
    assert!(
        paint.contains("size: 20.0"),
        "relative inline font size should reach paint"
    );
    assert!(
        paint.contains(&format!("color: {}", 0xEEEEFF)),
        "#eef background should reach paint"
    );
    assert!(
        paint.contains("color: 6697881"),
        "#639 border should reach paint"
    );
    assert!(!paint.contains("Hidden"));
}

#[test]
fn expanded_selectors_from_redirected_cached_sheets_reach_layout_and_paint() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<!doctype html><style>p {color:red}</style>\
                    <link rel='stylesheet' href='selectors.css'>\
                    <link rel='stylesheet' href='selectors.css'>\
                    <main><!-- comment --><p data-kind='Notice'>First</p>\
                    text <p>Next</p><p hidden>Hidden</p><span>Last</span></main>"
                        .to_vec(),
                ),
            ),
            ("/selectors.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    b"main > p[data-kind=notice i]:first-child {color:#123456}\
                    main > p + p:not([hidden], .skip) {color:#654321}\
                    main > p ~ span:last-child {background-color:#abcdef}\
                    main > [hidden] {display:none}"
                        .to_vec(),
                ),
            ),
        ],
        3,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=display-list", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paint = String::from_utf8(output.stdout).unwrap();
    for color in [0x123456, 0x654321, 0xABCDEF] {
        assert!(paint.contains(&format!("color: {color}")), "{paint}");
    }
    for text in ["First", "Next", "Last"] {
        assert!(paint.contains(text), "{paint}");
    }
    assert!(!paint.contains("Hidden"), "{paint}");
    assert_eq!(server.finish(), ["/", "/selectors.css", "/final.css"]);
}
