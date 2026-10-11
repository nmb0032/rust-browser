use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use rust_browser::{
    engine::document_base_url,
    html::parser,
    layout::ScrollOffset,
    reftest::{self, Fuzzy, Metrics, Relation},
    screenshot::{self, Options, PanelScroll, Screenshot},
};
use url::Url;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Workspace(PathBuf);
impl Workspace {
    fn new(name: &str) -> Self {
        let path = root()
            .join("target/reftest-contracts")
            .join(format!("{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        path
    }
    fn report(&self) -> screenshot::Result<reftest::Report> {
        reftest::run(
            &self.0.join("suite"),
            &self.0,
            &self.0.join("target/artifacts"),
            &Options::default(),
        )
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn pixels(values: &[[u8; 3]]) -> Screenshot {
    Screenshot {
        width: values.len() as u32,
        height: 1,
        rgba: values
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
    }
}

#[test]
fn fuzzy_metrics_use_max_rgb_and_count_pixels_once_with_inclusive_boundaries() {
    let actual = pixels(&[[10, 20, 30], [0, 1, 2], [10, 20, 30]]);
    let expected = pixels(&[[15, 24, 35], [0, 1, 2], [9, 20, 30]]);
    let (metrics, diff) = reftest::compare(&actual, &expected).unwrap();
    assert_eq!(
        metrics,
        Metrics {
            max_difference: 5,
            total_pixels: 2
        }
    );
    assert_eq!(&diff.rgba[..4], &[5, 4, 5, 255]);
    for value in [
        "maxDifference=5;totalPixels=2",
        "5;2",
        "maxDifference=0-5;totalPixels=0-2",
        "totalPixels=2;maxDifference=5",
    ] {
        assert!(Fuzzy::parse(value).unwrap().accepts(metrics), "{value}");
    }
    for value in ["4;2", "5;1", "6-7;2", "5;3-4", "5;0"] {
        assert!(!Fuzzy::parse(value).unwrap().accepts(metrics), "{value}");
    }
    assert!(!Fuzzy::parse("5;2").unwrap().accepts(Metrics::default()));
    assert!(Fuzzy::default().accepts(Metrics::default()));
    for value in [
        "5",
        "5;2;3",
        "256;1",
        "-1;1",
        "8-2;1",
        "x=1;2",
        "maxDifference=1;maxDifference=2",
        "0;16777217",
    ] {
        assert!(Fuzzy::parse(value).is_err(), "{value}");
    }
    let mut nonopaque = actual.clone();
    nonopaque.rgba[3] = 0;
    assert!(reftest::compare(&nonopaque, &expected).is_err());
    assert!(reftest::compare(&pixels(&[[0, 0, 0]]), &expected).is_err());
}

#[test]
fn parsed_dom_metadata_resolves_first_base_entities_percent_paths_and_specific_fuzzy() {
    let url = Url::parse("file:///local/suite/a.html").unwrap();
    let document = parser::parse(
        r#"<!doctype html><base href="refs/"><base href="ignored/">
        <link rel="HELP" href="https://www.w3.org/TR/CSS22/">
        <link rel="MaTcH" href="a%20b.html#part">
        <link rel="mismatch" href="c&amp;d.html">
        <meta name="fuzzy" content="0-2;0-3">
        <meta name="fuzzy" content="a%20b.html#part:maxDifference=1;totalPixels=2">"#,
    )
    .unwrap();
    assert_eq!(
        document_base_url(&document, &url).as_str(),
        "file:///local/suite/refs/"
    );
    let metadata = reftest::metadata(&document, &url).unwrap();
    assert_eq!(metadata.references.len(), 2);
    assert_eq!(
        metadata.references[0].url.as_str(),
        "file:///local/suite/refs/a%20b.html#part"
    );
    assert_eq!(metadata.references[0].relation, Relation::Match);
    assert_eq!(metadata.references[0].fuzzy, Fuzzy::parse("1;2").unwrap());
    assert_eq!(
        metadata.references[1].url.as_str(),
        "file:///local/suite/refs/c&d.html"
    );
    assert_eq!(
        metadata.references[1].fuzzy,
        Fuzzy::parse("0-2;0-3").unwrap()
    );
    for source in [
        "<link rel=match>",
        "<link rel=match href=''>",
        "<link rel='match mismatch' href='a.html'>",
        "<meta name=fuzzy content='0;0'><meta name=fuzzy content='0;0'>",
        "<meta name=fuzzy content='missing.html:0;0'>",
    ] {
        assert!(
            reftest::metadata(&parser::parse(source).unwrap(), &url).is_err(),
            "{source}"
        );
    }
}

#[test]
fn complete_native_suite_is_exact_and_mismatch_has_nonzero_pixels_at_two_scales() {
    for scale in [1.0, 2.0] {
        let options = Options {
            scale,
            ..Options::default()
        };
        let report = reftest::run(
            &root().join("tests/reftests"),
            &root(),
            &root().join(format!("target/reftests/scale-{scale}")),
            &options,
        )
        .unwrap();
        assert_eq!(report.outcomes.len(), 29);
        assert!(report.passed(), "{report:#?}");
        let mismatch = report
            .outcomes
            .iter()
            .find(|test| test.path.ends_with("mismatch.html"))
            .unwrap();
        assert!(mismatch.comparisons[0].contains(&format!(
            "totalPixels={}",
            10_000 * scale as u32 * scale as u32
        )));
        let multiple = report
            .outcomes
            .iter()
            .find(|test| test.path.ends_with("white-space-shaping.html"))
            .unwrap();
        assert_eq!(multiple.comparisons.len(), 3);
        assert!(multiple.comparisons[0].contains("=> FAIL"));
        assert!(multiple.comparisons[1].contains("=> PASS maxDifference=0 totalPixels=0"));
        assert!(multiple.comparisons[2].contains("=> PASS"));
        assert!(report.manifest.is_file());
    }

    let options = Options::default();
    let mut frame = screenshot::load(
        &root().join("tests/reftests/white-space-shaping.html"),
        &root(),
        &options,
    )
    .unwrap();
    screenshot::render(&mut frame, &options).unwrap();
    let node = frame
        .document
        .preorder()
        .find(|&id| {
            frame
                .document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some("shaped"))
        })
        .unwrap();
    let shaped = frame
        .font_context()
        .shape(frame.styles.get(node).unwrap(), "ffi");
    assert_eq!(
        shaped.glyphs.len(),
        1,
        "the test must actually use a ligature"
    );
    assert!(
        shaped.glyphs[0].face.italic,
        "must use the loaded static URL face, not bundled fallback"
    );
}

#[test]
fn mask_resources_refresh_on_viewport_change_and_remain_shared_in_frame_clones() {
    let workspace = Workspace::new("effects-refresh");
    let url = Url::from_file_path(root().join("tests/reftests/support/stripes.png")).unwrap();
    let path=workspace.write("page.html",&format!("<!doctype html><style>#a{{position:absolute;left:10px;top:10px;width:30px;height:30px;background:red}}@media(max-width:400px){{#a{{mask-image:url('{url}')}}}}</style><div id=a></div>"));
    let options = Options::default();
    let mut frame = screenshot::load(&path, &root(), &options).unwrap();
    assert!(frame.effect_images.is_empty());
    let options = Options {
        width: 200,
        height: 150,
        ..Options::default()
    };
    frame.set_media_environment(options.environment());
    assert!(frame.has_pending_effect_images());
    let mut navigation = rust_browser::engine::Navigation::new(
        frame.url.clone(),
        rust_browser::engine::LoadOptions::default(),
    );
    navigation
        .start_font_load(std::sync::Arc::new(frame.clone()))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut published = None;
    loop {
        match navigation.try_recv() {
            Ok(rust_browser::engine::LoadEvent::Progress(frame)) => published = Some(frame),
            Ok(rust_browser::engine::LoadEvent::Finished) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "native mask worker did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => panic!("native mask worker failed: {error}"),
        }
    }
    frame = (*published.unwrap()).clone();
    assert!(!frame.has_pending_effect_images());
    let image = screenshot::render(&mut frame, &options).unwrap();
    assert_eq!(
        &image.rgba[(20 * 200 + 20) * 4..(20 * 200 + 20) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(frame.effect_images.len(), 1);
    let clone = frame.clone();
    assert!(std::sync::Arc::ptr_eq(
        frame.effect_images.values().next().unwrap(),
        clone.effect_images.values().next().unwrap()
    ));
    screenshot::render(&mut frame, &options).unwrap();
    assert_eq!(frame.effect_images.len(), 1);
}

#[test]
fn headless_effect_resource_and_surface_budget_failures_are_not_silent_noops() {
    let workspace = Workspace::new("effects-limits");
    let path=workspace.write("remote.html","<!doctype html><style>div{mask-image:url('https://invalid.invalid/mask.png')}</style><div>mask</div>");
    let mut frame = screenshot::load(&path, &root(), &Options::default()).unwrap();
    assert!(
        !frame.has_pending_effect_images(),
        "restricted sources must not retry every idle update"
    );
    assert!(screenshot::render(&mut frame, &Options::default()).is_err());
    let path=workspace.write("large.html","<!doctype html><style>div{width:2000px;height:2000px;transform:rotate(5deg)}</style><div></div>");
    let options = Options {
        width: 4096,
        height: 4096,
        ..Options::default()
    };
    let error = screenshot::capture(&path, &root(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("effects framebuffer"), "{error}");
}

#[test]
fn explicit_fuzzy_metadata_is_applied_without_hidden_tolerance() {
    let workspace = Workspace::new("fuzzy-runner");
    workspace.write(
        "suite/ref.html",
        "<!doctype html><style>body{background:rgb(15,24,35)}</style>",
    );
    for (value, passed) in [("5;480000", true), ("4;480000", false), ("5;479999", false)] {
        workspace.write("suite/test.html", &format!(
            "<!doctype html><link rel=match href=ref.html><meta name=fuzzy content='ref.html:{value}'><style>body{{background:rgb(10,20,30)}}</style>"
        ));
        let report = workspace.report().unwrap();
        assert_eq!(report.passed(), passed, "{value}: {report:#?}");
        assert!(report.outcomes[0].comparisons[0].contains("maxDifference=5 totalPixels=480000"));
    }
}

#[test]
fn writing_references_use_real_declared_arabic_hebrew_and_vertical_japanese_faces() {
    let options = Options::default();
    for (path, id, source) in [
        ("bidi.html", "hebrew", "אבג"),
        ("bidi.html", "arabic", "سلام"),
        ("vertical-japanese.html", "mixed", "水0水"),
    ] {
        let mut frame =
            screenshot::load(&root().join("tests/reftests").join(path), &root(), &options).unwrap();
        screenshot::render(&mut frame, &options).unwrap();
        let node = frame
            .document
            .preorder()
            .find(|&node| {
                frame
                    .document
                    .node(node)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.attribute("id") == Some(id))
            })
            .unwrap();
        let run = frame
            .font_context()
            .shape(frame.styles.get(node).unwrap(), source);
        assert!(!run.glyphs.is_empty());
        assert!(
            run.glyphs
                .iter()
                .all(|g| g.id != 0 && g.face.metrics_indexed(g.id, g.size).width > 0)
        );
        if id == "mixed" {
            assert!(run.glyphs[0].upright && !run.glyphs[1].upright && run.glyphs[2].upright);
            assert_ne!(
                run.glyphs[0].face.name, run.glyphs[1].face.name,
                "declared Japanese fallback must be selected after Latin"
            );
        }
    }
}

#[test]
fn wrong_reference_exits_nonzero_and_preserves_actual_expected_diff_and_manifest() {
    let workspace = Workspace::new("wrong-reference");
    workspace.write(
        "suite/test.html",
        "<!doctype html><link rel=match href=reference.html><style>body {background:green}</style>",
    );
    workspace.write(
        "suite/reference.html",
        "<!doctype html><style>body {background:red}</style>",
    );
    let artifacts = root().join("target/reftests/intentional-failure");
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--reftests"])
        .arg(workspace.0.join("suite"))
        .arg("--resource-root")
        .arg(&workspace.0)
        .arg("--artifacts")
        .arg(&artifacts)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("totalPixels=480000") && stderr.contains("evidence="),
        "{stderr}"
    );
    for suffix in ["actual.png", "expected.png", "diff.png"] {
        assert!(
            artifacts
                .join(format!("test-000/comparison-000.{suffix}"))
                .is_file()
        );
    }
    let diff = Screenshot::read_png(&artifacts.join("test-000/comparison-000.diff.png")).unwrap();
    assert!(
        diff.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[..3] != [0, 0, 0])
    );
    assert!(
        fs::read_to_string(artifacts.join("manifest.txt"))
            .unwrap()
            .contains("FAIL")
    );
}

#[test]
fn multiple_matches_are_or_but_every_mismatch_must_pass_and_chains_are_checked() {
    let workspace = Workspace::new("relations");
    workspace.write("suite/test.html", "<!doctype html><link rel=match href=red.html><link rel=match href=white.html><link rel=mismatch href=red.html>");
    workspace.write(
        "suite/red.html",
        "<!doctype html><style>body{background:red}</style>",
    );
    workspace.write(
        "suite/white.html",
        "<!doctype html><link rel=match href=terminal.html>",
    );
    workspace.write("suite/terminal.html", "<!doctype html>");
    assert!(workspace.report().unwrap().passed());
    workspace.write("suite/test.html", "<!doctype html><link rel=match href=white.html><link rel=mismatch href=red.html><link rel=mismatch href=terminal.html>");
    let report = workspace.report().unwrap();
    assert!(!report.passed());
    assert!(
        report
            .outcomes
            .iter()
            .find(|t| t.path.ends_with("test.html"))
            .unwrap()
            .comparisons
            .iter()
            .any(|c| c.contains("Mismatch") && c.contains("=> FAIL"))
    );
}

#[test]
fn missing_cycles_readiness_remote_and_escaping_references_are_explicit_failures() {
    for (name, reference, ref_source, expected) in [
        ("missing", "missing.html", None, "No such file"),
        ("cycle", "test.html", None, "reference cycle"),
        (
            "wait",
            "ref.html",
            Some("<!doctype html><html class=reftest-wait>"),
            "unsupported readiness",
        ),
        (
            "script",
            "ref.html",
            Some("<!doctype html><script src='/resources/testdriver.js'></script>"),
            "unsupported readiness",
        ),
        (
            "remote",
            "https://example.invalid/ref.html",
            None,
            "local-only",
        ),
        ("data", "data:text/html,hello", None, "local-only"),
        (
            "query",
            "ref.html?q=1",
            Some("<!doctype html>"),
            "local-only",
        ),
        ("escape", "../../../../Cargo.toml", None, "escapes root"),
    ] {
        let workspace = Workspace::new(name);
        workspace.write(
            "suite/test.html",
            &format!("<!doctype html><link rel=match href='{reference}'>"),
        );
        if let Some(source) = ref_source {
            workspace.write("suite/ref.html", source);
        }
        let report = workspace.report().unwrap();
        assert!(!report.passed(), "{name}");
        assert!(
            report.outcomes[0]
                .error
                .as_ref()
                .unwrap()
                .contains(expected),
            "{name}: {report:#?}"
        );
    }
}

#[test]
fn local_resource_errors_css_imports_fonts_images_and_scripts_do_not_silently_fallback() {
    let workspace = Workspace::new("resource-errors");
    let options = Options {
        width: 100,
        height: 80,
        ..Options::default()
    };
    for source in [
        "<link rel=stylesheet href='https://example.invalid/a.css'>",
        "<style>@import 'https://example.invalid/a.css';</style>",
        "<img src='missing.png'>",
        "<style>@font-face{font-family:X;src:url(missing.ttf)}body{font-family:X}</style>text",
        "<style>@font-face{font-family:X;src:local('Noto Sans')}body{font-family:X}</style>text",
        "<style>@font-face{font-family:X;src:url(https://example.invalid/a.ttf)}body{font-family:X}</style>text",
        "<body onload='ready()'>",
        "<html class=reftest-wait>",
        "<script>ready()</script>",
    ] {
        let path = workspace.write("test.html", &format!("<!doctype html>{source}"));
        assert!(
            screenshot::capture(&path, &workspace.0, &options).is_err(),
            "{source}"
        );
    }
    workspace.write("a.css", "@import 'b.css';");
    workspace.write("b.css", "@import 'a.css';");
    let path = workspace.write(
        "test.html",
        "<!doctype html><link rel=stylesheet href=a.css>",
    );
    assert!(
        screenshot::capture(&path, &workspace.0, &options)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    let path = workspace.write(
        "escaped page.html",
        "<!doctype html><style>body{background:green}</style>",
    );
    assert!(screenshot::capture(&path, &workspace.0, &options).is_ok());
    let path = workspace.write("recovered.html", "<!doctype html><p><b>AVffi</p>water");
    assert!(screenshot::capture(&path, &workspace.0, &options).is_ok());
}

#[test]
fn png_references_decode_with_bounds_and_normalize_alpha_and_report_size_errors() {
    let workspace = Workspace::new("png");
    let page = workspace.write(
        "suite/test.html",
        "<!doctype html><link rel=match href=reference.png><style>body{background:green}</style>",
    );
    let actual = screenshot::capture(&page, &workspace.0, &Options::default()).unwrap();
    actual
        .save(&workspace.0.join("suite/reference.png"))
        .unwrap();
    assert!(workspace.report().unwrap().passed());
    pixels(&[[0, 128, 0]])
        .save(&workspace.0.join("suite/reference.png"))
        .unwrap();
    let report = workspace.report().unwrap();
    assert!(!report.passed());
    assert!(report.outcomes[0].comparisons[0].contains("reference size mismatch"));
    let transparent = Screenshot {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 0],
    };
    let path = workspace.0.join("alpha.png");
    transparent.save(&path).unwrap();
    assert_eq!(
        Screenshot::read_png(&path).unwrap().rgba,
        [255, 255, 255, 255]
    );
    Screenshot {
        width: 1,
        height: 1,
        rgba: vec![10, 20, 30, 128],
    }
    .save(&path)
    .unwrap();
    let normalized = Screenshot::read_png(&path).unwrap();
    assert_eq!(normalized.rgba, [132, 137, 142, 255]);
    let page = workspace.write(
        "image.html",
        "<!doctype html><style>img{position:fixed;left:0;top:0}</style><img src=alpha.png>",
    );
    let image = screenshot::capture(
        &page,
        &workspace.0,
        &Options {
            width: 2,
            height: 1,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(
        at(&image, 0, 0),
        normalized.rgba.as_slice(),
        "PNG normalization must agree with production alpha compositing"
    );
    fs::write(&path, b"not png").unwrap();
    assert!(Screenshot::read_png(&path).is_err());
    let path = workspace.0.join("large.png");
    image::save_buffer_with_format(
        &path,
        &vec![0; 4097 * 4],
        4097,
        1,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
    assert!(Screenshot::read_png(&path).is_err());
}

fn at(image: &Screenshot, x: u32, y: u32) -> &[u8] {
    let start = ((y * image.width + x) * 4) as usize;
    &image.rgba[start..start + 4]
}

#[test]
fn scale_root_scroll_panel_scroll_and_fixed_pixels_use_native_spatial_states() {
    let workspace = Workspace::new("scroll");
    let path = workspace.write("page.html", r#"<!doctype html><style>
      body {margin:0;height:1000px}
      #root {position:absolute;left:0;top:100px;width:20px;height:20px;background:red}
      #fixed {position:fixed;left:40px;top:10px;width:20px;height:20px;background:blue}
      #panel {position:absolute;left:80px;top:120px;width:40px;height:40px;overflow:hidden}
      #spacer {height:30px} #content {height:50px;background:green}
      </style><div id=root></div><div id=fixed></div><div id=panel><div id=spacer></div><div id=content></div></div>"#);
    let options = Options {
        width: 200,
        height: 160,
        scale: 2.0,
        scroll: ScrollOffset { x: 0.0, y: 100.0 },
        panels: vec![PanelScroll {
            id: "panel".into(),
            offset: ScrollOffset { x: 0.0, y: 30.0 },
        }],
    };
    let actual = screenshot::capture(&path, &workspace.0, &options).unwrap();
    assert_eq!((actual.width, actual.height), (400, 320));
    assert_eq!(at(&actual, 2, 2), [255, 0, 0, 255]);
    assert_eq!(at(&actual, 82, 22), [0, 0, 255, 255]);
    assert_eq!(at(&actual, 162, 42), [0, 128, 0, 255]);
    assert_eq!(at(&actual, 162, 122), [255, 255, 255, 255]);
    assert_eq!(
        actual,
        screenshot::capture(&path, &workspace.0, &options).unwrap()
    );
    let unscrolled = screenshot::capture(
        &path,
        &workspace.0,
        &Options {
            scroll: ScrollOffset::default(),
            panels: vec![],
            ..options.clone()
        },
    )
    .unwrap();
    assert_eq!(at(&unscrolled, 2, 202), [255, 0, 0, 255]);
    assert_eq!(at(&unscrolled, 82, 22), [0, 0, 255, 255]);
    assert_eq!(at(&unscrolled, 162, 242), [255, 255, 255, 255]);
    assert_eq!(at(&unscrolled, 162, 302), [0, 128, 0, 255]);
    let path = workspace.write("horizontal.html", r#"<!doctype html><style>
      body {margin:0;width:1000px;height:1000px}
      #root {position:absolute;left:100px;top:100px;width:20px;height:20px;background:red}
      #fixed {position:fixed;left:40px;top:10px;width:20px;height:20px;background:blue}
      #panel {position:absolute;left:180px;top:120px;width:40px;height:40px;overflow:hidden}
      #content {width:80px;height:80px;position:relative}
      #ink {position:absolute;left:50px;top:30px;width:10px;height:10px;background:green}
      </style><div id=root></div><div id=fixed></div><div id=panel><div id=content><div id=ink></div></div></div>"#);
    let options = Options {
        scroll: ScrollOffset { x: 100.0, y: 100.0 },
        panels: vec![PanelScroll {
            id: "panel".into(),
            offset: ScrollOffset { x: 30.0, y: 30.0 },
        }],
        ..options
    };
    let horizontal = screenshot::capture(&path, &workspace.0, &options).unwrap();
    assert_eq!(at(&horizontal, 2, 2), [255, 0, 0, 255]);
    assert_eq!(at(&horizontal, 82, 22), [0, 0, 255, 255]);
    assert_eq!(at(&horizontal, 202, 42), [0, 128, 0, 255]);
}

#[test]
fn cli_png_capture_is_headless_reproducible_and_bad_size_exits_nonzero() {
    let workspace = Workspace::new("cli");
    let page = root().join("tests/reftests/white-space-shaping.html");
    let capture = |path: &Path| {
        Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(&page)
            .arg("--screenshot")
            .arg(path)
            .args(["--viewport", "320x160", "--device-scale", "2"])
            .arg("--resource-root")
            .arg(root())
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap()
    };
    let first = workspace.0.join("first.png");
    let second = workspace.0.join("second.png");
    for path in [&first, &second] {
        let output = capture(path);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("640x320 PNG"));
    }
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
    for options in [
        Options {
            width: 0,
            ..Options::default()
        },
        Options {
            width: 4097,
            ..Options::default()
        },
        Options {
            scale: f64::NAN,
            ..Options::default()
        },
        Options {
            scale: -1.0,
            ..Options::default()
        },
        Options {
            width: 801,
            scale: 0.5,
            ..Options::default()
        },
        Options {
            scroll: ScrollOffset {
                x: 0.0,
                y: f32::INFINITY,
            },
            ..Options::default()
        },
    ] {
        assert!(options.dimensions().is_err());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(page)
        .arg("--screenshot")
        .arg(workspace.0.join("invalid.png"))
        .args(["--viewport", "0x600"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn enumeration_artifact_boundaries_and_symlink_escapes_are_rejected() {
    let workspace = Workspace::new("boundaries");
    workspace.write("suite/empty.html", "<!doctype html>");
    assert!(
        workspace
            .report()
            .unwrap_err()
            .to_string()
            .contains("no reftests")
    );
    workspace.write(
        "suite/test.html",
        "<!doctype html><link rel=match href=empty.html>",
    );
    assert!(
        reftest::run(
            &workspace.0.join("suite"),
            &workspace.0,
            &workspace.0.join("suite/artifacts"),
            &Options::default()
        )
        .is_err()
    );
    assert!(
        reftest::run(
            &workspace.0.join("suite"),
            &workspace.0,
            &workspace.0.join("suite/new-dir/artifacts"),
            &Options::default()
        )
        .is_err()
    );
    assert!(
        !workspace.0.join("suite/new-dir").exists(),
        "rejected artifacts must not create source directories"
    );
    let error = reftest::run(
        &workspace.0.join("suite"),
        &workspace.0,
        &workspace.0,
        &Options::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("outside the suite"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root().join("Cargo.toml"), workspace.0.join("outside.html"))
            .unwrap();
        let url = Url::from_file_path(workspace.0.join("outside.html")).unwrap();
        assert!(screenshot::load_url(&url, &workspace.0, &Options::default()).is_err());
        std::os::unix::fs::symlink(&workspace.0, workspace.0.join("suite/loop")).unwrap();
        assert!(
            workspace
                .report()
                .unwrap_err()
                .to_string()
                .contains("symlinks")
        );
    }
}
