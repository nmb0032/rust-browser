use std::{path::Path, process::Command};

fn assert_dom_snapshot(page: &str, expected: &str) {
    let page_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("pages")
        .join(page);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(page_path)
        .arg("--dump=dom")
        .output()
        .expect("browser process should start");

    assert!(
        output.status.success(),
        "browser failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).expect("DOM dump should be UTF-8");
    assert_eq!(actual.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
}

#[test]
fn dumps_basic_page() {
    assert_dom_snapshot("basic.html", include_str!("snapshots/basic.expected"));
}

#[test]
fn dumps_void_elements_and_boolean_attributes() {
    assert_dom_snapshot("dom_dump.html", include_str!("snapshots/dom_dump.expected"));
}

#[test]
fn dumps_the_layout_tree_with_geometry() {
    let page_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("pages")
        .join("basic.html");
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(page_path)
        .arg("--dump=layout")
        .output()
        .expect("browser process should start");

    assert!(
        output.status.success(),
        "browser failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).expect("layout dump should be UTF-8");
    assert!(actual.contains("<h1> Block"));
    assert!(actual.contains("<li> Block"));
    assert!(actual.contains("width=752.0"));
    assert!(!actual.contains("My Simple Page"));
}
