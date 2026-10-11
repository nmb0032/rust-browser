use std::{path::Path, process::Command};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/grid.html"))
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
fn grid_fixture_cli_reports_actual_tracks_atomic_boxes_and_scroll_geometry_deterministically() {
    for format in ["layout", "values", "style", "display-list"] {
        assert_eq!(dump(format), dump(format), "{format}");
    }
    let layout = dump("layout");
    assert!(layout.contains("display=Grid columns="));
    assert!(layout.contains("display=InlineGrid columns="));
    assert!(layout.contains("grid-item=true"));
    assert!(layout.contains("AutoFit"));
    assert!(layout.contains("scroll-range=ScrollRange"));
    assert_eq!(
        layout
            .lines()
            .filter(|line| line.contains("<article> Block") && line.contains("width=180.0"))
            .count(),
        3,
        "{layout}"
    );
    let values = dump("values");
    assert!(values.contains("computed grid:"));
    assert!(values.contains("used grid-item: true"));
    let display = dump("display-list");
    assert!(display.contains("Text") && display.contains("SpatialState"));
}
