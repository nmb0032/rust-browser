use std::{path::Path, process::Command};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/overflow_scrolling.html"))
        .arg("--bundled-fonts")
        .arg(format!("--dump={format}"))
        .output()
        .unwrap();
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
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn overflow_fixture_dumps_clips_ranges_offsets_and_deterministic_geometry() {
    for format in ["layout", "values", "display-list"] {
        assert_eq!(dump(format), dump(format), "{format}");
    }
    let layout = dump("layout");
    assert!(layout.contains("overflow-x=Hidden overflow-y=Hidden"));
    assert!(layout.contains("overflow-x=Auto overflow-y=Auto"));
    assert!(layout.contains("overflow-x=Scroll overflow-y=Scroll"));
    assert!(layout.contains("clip-port=Rect") && layout.contains("scroll-range=ScrollRange"));
    assert!(layout.contains("scroll-offset=ScrollOffset { x: 0.0, y: 0.0 }"));
    assert!(layout.contains("position=Fixed") && layout.contains("coordinate-space=Viewport"));
    let values = dump("values");
    assert!(values.contains("used scroll-port") && values.contains("used scroll-extent"));
    assert!(values.contains("used scroll-range") && values.contains("visual clipped-border"));
    let display = dump("display-list");
    assert!(display.contains("SpatialState") && display.contains("ClipRect"));
    assert!(display.contains("color: 7566195")); // Basic overlay thumb.
}
