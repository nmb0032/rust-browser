use std::{path::Path, process::Command};

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
