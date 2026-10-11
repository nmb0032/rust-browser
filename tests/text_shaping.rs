use std::{path::Path, process::Command};

fn dump(format: &str, bundled: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-browser"));
    command.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/text_shaping.html"));
    command.arg(format!("--dump={format}"));
    if bundled {
        command.arg("--bundled-fonts");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn deterministic_cli_exposes_actual_faces_glyph_clusters_and_offsets() {
    let layout = dump("layout", true);
    assert_eq!(layout, dump("layout", true));
    assert!(layout.contains("face=\"Noto Sans\"[0] glyph=1656 cluster=1..4"));
    assert!(layout.contains("offset=("));
    let display = dump("display-list", true);
    assert!(display.contains("ShapedGlyph") && display.contains("cluster: 1..4"));
    let values = dump("values", true);
    assert!(values.contains("used shaped-glyphs") && values.contains("Noto Sans"));
}

#[test]
fn production_cli_uses_the_discovered_font_environment_by_default() {
    let expected = rust_browser::text::FontContext::system()
        .select(&rust_browser::style::ComputedStyle::default());
    let layout = dump("layout", false);
    assert!(
        layout.contains(&format!("face={:?}", expected.name)),
        "default CSS family did not use its selected face"
    );
    assert!(layout.contains("rtl=true"));
    assert!(layout.contains("glyph="));
}
