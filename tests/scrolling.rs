use rust_browser::{
    layout::ScrollOffset,
    reftest,
    screenshot::{self, Options, PanelScroll},
};
use std::{path::PathBuf, process::Command};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn options(kind: &str, scale: f64) -> Options {
    let mut o = Options {
        width: 300,
        height: 200,
        scale,
        ..Options::default()
    };
    match kind {
        "root" => o.scroll.y = 40.0,
        "panel" => o.panels.push(PanelScroll {
            id: "p".into(),
            offset: ScrollOffset { x: 0.0, y: 40.0 },
        }),
        "snap" => {
            o.snap_scroll = true;
            o.panels.push(PanelScroll {
                id: "p".into(),
                offset: ScrollOffset { x: 0.0, y: 64.0 },
            })
        }
        "bars" => o.panels.push(PanelScroll {
            id: "p".into(),
            offset: ScrollOffset { x: 0.0, y: 80.0 },
        }),
        "fragment" => o.fragment = Some("target".into()),
        _ => unreachable!(),
    }
    o
}
#[test]
fn independent_sticky_snap_styled_bars_and_fragment_references_are_exact_at_both_scales() {
    for scale in [1.0, 2.0] {
        for kind in ["root", "panel", "snap", "bars", "fragment"] {
            let report = reftest::run(
                &root().join(format!("tests/reftests-scrolling/{kind}")),
                &root(),
                &root().join(format!("target/reftests/scrolling-{kind}-scale-{scale}")),
                &options(kind, scale),
            )
            .unwrap();
            assert_eq!(report.outcomes.len(), 1);
            assert!(report.passed(), "{kind}/{scale}: {:?}", report.outcomes);
        }
    }
}
#[test]
fn real_cli_programmatic_snap_and_fragment_options_use_native_production_path() {
    for (kind, file, args) in [
        (
            "snap",
            "snap",
            vec!["--snap-scroll", "--panel-scroll", "p:0:64"],
        ),
        ("fragment", "fragment", vec!["--fragment", "target"]),
    ] {
        let output = root().join(format!("target/scrolling-{kind}-cli.png"));
        let result = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .current_dir(root())
            .arg(format!("tests/reftests-scrolling/{kind}/{file}.html"))
            .arg("--screenshot")
            .arg(&output)
            .args(["--resource-root", ".", "--viewport", "300x200"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            result.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual = screenshot::Screenshot::read_png(&output).unwrap();
        let expected = screenshot::capture(
            &root().join(format!(
                "tests/reftests-scrolling/{kind}/support/{file}-ref.html"
            )),
            &root(),
            &options(kind, 1.0),
        )
        .unwrap();
        assert!(actual == expected);
        std::fs::remove_file(output).unwrap();
    }
}
