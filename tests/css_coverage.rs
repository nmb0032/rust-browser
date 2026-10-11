use std::{path::Path, process::Command};

#[test]
fn css_coverage_cli_is_offline_and_exact_even_with_an_unloadable_location() {
    for location in [None, Some("https://127.0.0.1:1/never-load-this")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rust-browser"));
        if let Some(location) = location {
            command.arg(location);
        }
        let output = command.arg("--dump=css-coverage").output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            include_str!("../docs/css-coverage.md")
        );
    }
}

#[test]
fn generated_css_registry_and_report_pass_offline_check_without_writes() {
    let output = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/generate_css_properties.py"))
        .arg("--check")
        .output()
        .expect("python3 is required to verify frozen CSS data generation");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, b"CSS generation verified\n");
}
