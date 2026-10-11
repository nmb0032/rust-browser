use std::{path::Path, process::Command};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/positioning_stacking.html"))
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
fn positioned_fixture_exposes_geometry_stacking_and_coordinate_spaces() {
    let layout = dump("layout");
    assert_eq!(layout, dump("layout"));
    assert!(layout.contains("<header> Block x=0.0 y=0.0 width=800.0 height=48.0"));
    assert!(layout.contains("position=Fixed") && layout.contains("coordinate-space=Viewport"));
    assert!(layout.contains("z-index=Integer(-1)"));
    assert!(layout.contains("position=Absolute") && layout.contains("position=Relative"));
    let display = dump("display-list");
    assert!(display.contains("CoordinateSpace(\n        Viewport"));
    assert!(display.contains("CoordinateSpace(\n        Document"));
    let values = dump("values");
    assert!(values.contains("computed position: Fixed"));
    assert!(values.contains("z-index=Integer(3)"));
    assert!(values.contains("used coordinate-space: Viewport"));
}
