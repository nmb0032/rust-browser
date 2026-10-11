use rust_browser::{
    engine::PrintOptions,
    reftest,
    screenshot::{self, Options, PrintCapture},
};
use std::{path::PathBuf, process::Command};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn print_options(scale: f64, page: usize) -> Options {
    Options {
        width: 200,
        height: 120,
        scale,
        print: Some(PrintCapture {
            options: PrintOptions {
                width: 200.0,
                height: 120.0,
                margin: 0.0,
                ..PrintOptions::default()
            },
            page,
        }),
        ..Options::default()
    }
}
#[test]
fn independent_column_rectangles_lines_and_spanner_bands_match_at_both_scales() {
    for scale in [1.0, 2.0] {
        let options = Options {
            scale,
            ..Options::default()
        };
        let report = reftest::run(
            &root().join("tests/reftests-fragmentation"),
            &root(),
            &root().join(format!("target/reftests/fragmentation-scale{scale}")),
            &options,
        )
        .unwrap();
        assert_eq!(report.outcomes.len(), 4);
        assert!(report.passed(), "{:?}", report.outcomes);
    }
}
#[test]
fn every_physical_print_page_has_independent_exact_reference_and_correct_dimensions() {
    for scale in [1.0, 2.0] {
        let options = print_options(scale, 1);
        let actual = screenshot::capture_print_pages(
            &root().join("tests/reftests-print/pages.html"),
            &root(),
            &options,
            options.print.unwrap().options,
        )
        .unwrap();
        let expected = screenshot::capture_print_pages(
            &root().join("tests/reftests-print/support/pages-ref.html"),
            &root(),
            &options,
            options.print.unwrap().options,
        )
        .unwrap();
        assert_eq!(actual.len(), 2);
        assert_eq!(expected.len(), 2);
        for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(
                (a.width, a.height),
                ((200.0 * scale) as u32, (120.0 * scale) as u32)
            );
            assert!(a == b, "physical page {} at scale {scale}", i + 1);
            assert!(
                a.rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0..3] != [255, 255, 255])
            );
        }
        for page in 1..=2 {
            let report = reftest::run(
                &root().join("tests/reftests-print"),
                &root(),
                &root().join(format!("target/reftests/print-page{page}-scale{scale}")),
                &print_options(scale, page),
            )
            .unwrap();
            assert_eq!(report.outcomes.len(), 1);
            assert!(report.passed(), "{:?}", report.outcomes);
        }
    }
}
#[test]
fn real_cli_prints_selected_physical_sheet_and_rejects_missing_pages() {
    let output = root().join("target/print-cli-page2.png");
    let run = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .current_dir(root())
        .args(["tests/reftests-print/pages.html", "--screenshot"])
        .arg(&output)
        .args([
            "--print-page",
            "2",
            "--page-size",
            "200x120",
            "--resource-root",
            ".",
            "--device-scale",
            "2",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        run.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let actual = screenshot::Screenshot::read_png(&output).unwrap();
    let expected = screenshot::capture(
        &root().join("tests/reftests-print/support/pages-ref.html"),
        &root(),
        &print_options(2.0, 2),
    )
    .unwrap();
    assert!(actual == expected);
    std::fs::remove_file(output).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .current_dir(root())
        .args([
            "tests/reftests-print/pages.html",
            "--screenshot",
            "target/print-missing-page.png",
            "--print-page",
            "3",
            "--page-size",
            "200x120",
            "--resource-root",
            ".",
        ])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(!root().join("target/print-missing-page.png").exists());
}
