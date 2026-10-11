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

#[test]
fn dumps_anonymous_blocks_and_split_inline_ancestors() {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/anonymous_boxes.html"))
        .arg("--dump=layout")
        .output()
        .expect("browser process should start");
    assert!(
        output.status.success(),
        "browser failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).expect("layout dump should be UTF-8");
    assert_eq!(
        actual.matches("#anonymous-block Block").count(),
        7,
        "{actual}"
    );
    assert_eq!(actual.matches("<em> Inline").count(), 3, "{actual}");
    assert_eq!(actual.matches("::before Block").count(), 1, "{actual}");
    assert!(actual.contains("<img> Inline"));
    // The nested real block and both anonymous runs are section children;
    // the inline continuations occur one level beneath their wrappers.
    let nested = actual.split("<section> Block").nth(2).unwrap();
    assert!(nested.contains("\n        <span> Block"));
    assert!(nested.contains("\n          <em> Inline"));
    assert!(!nested.contains("\n              <span> Block"));
    assert!(!actual.contains("Anonymous block boxes — Milestone 8.1"));
}

#[test]
fn dumps_centered_page_with_distinct_computed_auto_and_used_margins() {
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/auto_margins.html");
    for mode in ["layout", "values", "style"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(&page)
            .arg(format!("--dump={mode}"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        match mode {
            "layout" => assert!(
                actual.contains("<main> Block x=98.0 y=44.0 width=604.0"),
                "{actual}"
            ),
            "values" => {
                assert!(actual.contains("specified Margin(Left): Auto"), "{actual}");
                assert!(actual.contains("used margins: Edges { top: UsedLength(20.0), right: UsedLength(74.0), bottom: UsedLength(20.0), left: UsedLength(74.0) }"), "{actual}");
            }

            "style" => assert!(actual.contains("margin-left: Auto"), "{actual}"),
            _ => unreachable!(),
        }
    }
}

#[test]
fn dumps_collapsed_paragraph_empty_chain_parent_overlap_and_containment_geometry() {
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/collapsing_margins.html");
    for mode in ["layout", "values"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(&page)
            .arg(format!("--dump={mode}"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        let actual = String::from_utf8(output.stdout).unwrap();
        if mode == "layout" {
            // Paragraph y=133 + height=20 + collapsed 16px => y=169.
            // The nested empty chain contributes max(50,40,20,15)-30=20.
            for line in [
                "<main> Block x=98.0 y=44.0 width=604.0 height=617.0",
                "<p> Block x=120.0 y=133.0 width=560.0 height=20.0",
                "<p> Block x=120.0 y=169.0 width=560.0 height=20.0",
                "<p> Block x=120.0 y=227.0 width=560.0 height=20.0",
                "<p> Block x=120.0 y=267.0 width=560.0 height=20.0",
                "<p> Block x=120.0 y=436.0 width=560.0 height=20.0",
                "<p> Block x=120.0 y=446.0 width=560.0 height=20.0",
                "<div> Block x=120.0 y=498.0 width=560.0 height=85.0",
                "<div> Block x=120.0 y=528.0 width=560.0 height=20.0",
            ] {
                assert!(actual.contains(line), "{line}\n{actual}");
            }
            assert_eq!(
                actual
                    .matches("<div> Block x=120.0 y=257.0 width=560.0 height=0.0")
                    .count(),
                2,
                "{actual}"
            );
            assert_eq!(
                actual
                    .matches("<div> Block x=120.0 y=339.0 width=560.0 height=20.0")
                    .count(),
                2,
                "{actual}"
            );
        } else {
            assert!(
                actual.contains("used margins: Edges { top: UsedLength(-10.0), right: UsedLength(0.0), bottom: UsedLength(50.0), left: UsedLength(0.0) }"),
                "{actual}"
            );
            assert!(
                actual.contains("used margins: Edges { top: UsedLength(-30.0), right: UsedLength(0.0), bottom: UsedLength(40.0), left: UsedLength(0.0) }"),
                "{actual}"
            );
        }
    }
}
