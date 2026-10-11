use rust_browser::{
    reftest,
    screenshot::{self, Options},
};
use std::{path::PathBuf, process::Command};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn six_independent_native_animation_references_are_exact_at_nonzero_time_and_two_scales() {
    for scale in [1.0, 2.0] {
        let options = Options {
            scale,
            animation_time: 1.0,
            ..Options::default()
        };
        let report = reftest::run(
            &root().join("tests/reftests-animations"),
            &root(),
            &root().join(format!("target/reftests/animations-scale-{scale}")),
            &options,
        )
        .unwrap();
        assert_eq!(report.outcomes.len(), 6);
        assert!(report.passed(), "{:?}", report.outcomes);
    }
}
#[test]
fn native_cli_freezes_declarative_css_with_units_and_records_real_pixels() {
    let output = root().join("target/animation-cli.png");
    let status = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .current_dir(root())
        .args(["tests/reftests-animations/moving-box.html", "--screenshot"])
        .arg(&output)
        .args([
            "--animation-time",
            "1000ms",
            "--resource-root",
            ".",
            "--viewport",
            "100x80",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let actual = screenshot::Screenshot::read_png(&output).unwrap();
    let options = Options {
        width: 100,
        height: 80,
        animation_time: 1.0,
        ..Options::default()
    };
    let expected = screenshot::capture(
        &root().join("tests/reftests-animations/support/moving-box-ref.html"),
        &root(),
        &options,
    )
    .unwrap();
    assert!(
        actual == expected,
        "CLI pixels differ from the independent static reference"
    );
    let zero = screenshot::capture(
        &root().join("tests/reftests-animations/moving-box.html"),
        &root(),
        &Options {
            animation_time: 0.0,
            ..options.clone()
        },
    )
    .unwrap();
    assert!(zero != actual);
    std::fs::remove_file(output).unwrap();
}
#[test]
fn screenshot_time_is_bounded_and_sequential_snapshots_retain_the_effect_clock() {
    for time in [-1.0, f64::NAN, f64::INFINITY, 1_000_000_001.0] {
        assert!(
            Options {
                animation_time: time,
                ..Options::default()
            }
            .dimensions()
            .is_err()
        );
    }
    let path = root().join("tests/reftests-animations/moving-box.html");
    let mut frame = screenshot::load(&path, &root(), &Options::default()).unwrap();
    let zero = screenshot::render(&mut frame, &Options::default()).unwrap();
    let one = screenshot::render(
        &mut frame,
        &Options {
            animation_time: 1.0,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(zero != one);
    assert!(zero == screenshot::render(&mut frame, &Options::default()).unwrap());
}
