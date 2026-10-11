use std::{path::Path, process::Command};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/floats_tables.html"))
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
fn floats_tables_fixture_cli_reports_typed_geometry_deterministically() {
    for format in ["layout", "values", "style", "display-list"] {
        assert_eq!(dump(format), dump(format), "{format}");
    }
    let layout = dump("layout");
    assert!(layout.contains("float=Left") && layout.contains("float=Right"));
    assert!(layout.contains("clear=Both"));
    assert!(layout.contains("display=Table table-layout=Fixed"));
    assert!(layout.contains("display=InlineTable"));
    assert!(layout.contains("border-collapse=Collapse"));
    assert!(layout.contains("scroll-range=ScrollRange"));
    assert!(layout.contains("shared-borders="));
    let values = dump("values");
    assert!(values.contains("computed table:") && values.contains("computed float:"));
    assert!(dump("display-list").contains("SpatialState"));
}
