use rust_browser::{
    engine::PrintOptions,
    reftest,
    screenshot::{self, Options, PrintCapture},
};
use std::{path::PathBuf, process::Command};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn independent_native_vector_and_math_references_match_at_scales_one_and_two() {
    for scale in [1.0, 2.0] {
        let report = reftest::run(
            &root().join("tests/reftests-vectors"),
            &root(),
            &root().join(format!("target/reftests/vectors-scale{scale}")),
            &Options {
                scale,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(report.outcomes.len(), 8);
        assert!(report.passed(), "{:?}", report.outcomes);
    }
}

#[test]
fn real_cli_vector_demo_renders_and_prints_equations_without_an_alternate_renderer() {
    let run = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .current_dir(root())
        .args([
            "tests/pages/vectors.html",
            "--screenshot",
            "target/vectors-demo.png",
            "--viewport",
            "800x720",
            "--resource-root",
            ".",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let png = image::open(root().join("target/vectors-demo.png"))
        .unwrap()
        .to_rgba8();
    assert!(png.pixels().filter(|p| p.0[..3] != [255, 255, 255]).count() > 3000);
    let standalone = screenshot::capture(
        &root().join("tests/reftests-vectors/support/icon.svg"),
        &root(),
        &Options::default(),
    )
    .unwrap();
    let standalone_ref = screenshot::capture(
        &root().join("tests/reftests-vectors/support/icon-document-ref.html"),
        &root(),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(standalone, standalone_ref);
    for scale in [1.0, 2.0] {
        let options = PrintOptions {
            width: 400.0,
            height: 220.0,
            margin: 12.0,
            ..PrintOptions::default()
        };
        let pages = screenshot::capture_print_pages(
            &root().join("tests/pages/vectors.html"),
            &root(),
            &Options {
                scale,
                print: Some(PrintCapture { options, page: 1 }),
                ..Options::default()
            },
            options,
        )
        .unwrap();
        assert!(!pages.is_empty());
        assert!(pages.iter().all(|p| {
            p.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[..3] != [255, 255, 255])
        }));
    }
}

#[test]
fn physical_print_pages_preserve_vectors_and_equations_with_independent_pixels() {
    for scale in [1.0, 2.0] {
        let options = PrintOptions {
            width: 120.0,
            height: 80.0,
            margin: 0.0,
            ..PrintOptions::default()
        };
        let capture = Options {
            scale,
            print: Some(PrintCapture { options, page: 1 }),
            ..Options::default()
        };
        let actual = screenshot::capture_print_pages(
            &root().join("tests/reftests-vectors/vector-pages.html"),
            &root(),
            &capture,
            options,
        )
        .unwrap();
        let expected = screenshot::capture_print_pages(
            &root().join("tests/reftests-vectors/references/vector-pages-ref.html"),
            &root(),
            &capture,
            options,
        )
        .unwrap();
        assert_eq!(actual.len(), 2);
        assert_eq!(actual, expected, "scale {scale}");
    }
}

#[test]
fn headless_diagnoses_restricted_nested_svg_resources_and_vector_device_budgets() {
    let directory = root().join(format!("target/vector-negative-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, html, expected) in [
        (
            "network",
            r#"<svg width=40 height=40><image href="https://bad.test/x.png" width=40 height=40/></svg>"#,
            "local-only",
        ),
        (
            "external-use",
            r##"<svg width=40 height=40><use href="elsewhere.svg#x"/></svg>"##,
            "within this document",
        ),
        (
            "huge",
            r#"<svg width=4096 height=4096><rect width=4096 height=4096/></svg>"#,
            "vector",
        ),
    ] {
        let path = directory.join(format!("{name}.html"));
        std::fs::write(&path, html).unwrap();
        let error = screenshot::capture(&path, &directory, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}
