use std::{path::Path, process::Command};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/flexbox.html"))
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
fn flex_fixture_cli_exposes_real_rows_atomic_boxes_and_retained_scroll_geometry() {
    for format in ["layout", "values", "style", "display-list"] {
        assert_eq!(dump(format), dump(format), "{format}");
    }
    let layout = dump("layout");
    assert!(layout.contains("display=Flex flex-direction=Row"));
    assert!(layout.contains("display=Flex flex-direction=Column"));
    assert!(layout.contains("display=InlineFlex"));
    assert!(layout.contains("flex-item=true") && layout.contains("scroll-range=ScrollRange"));
    for width in ["222.7", "262.7", "182.7"] {
        assert!(
            layout
                .lines()
                .any(|line| line.contains("<article> Block")
                    && line.contains(&format!("width={width}"))),
            "{layout}"
        );
    }
    let values = dump("values");
    assert!(values.contains("computed flex:") && values.contains("used flex-item: true"));
    assert!(values.contains("specified FlexBasis"));
    let display = dump("display-list");
    assert!(display.contains("Text") && display.contains("SpatialState"));
}
