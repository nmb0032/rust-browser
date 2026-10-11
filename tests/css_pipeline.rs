mod support;

use std::{path::Path, process::Command};

use support::{Reply, Server};

fn dump(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg("--bundled-fonts")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/styles.html"))
        .arg(format!("--dump={format}"))
        .output()
        .expect("browser process should start");
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
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

#[test]
fn unicode_line_breaking_demo_exposes_url_cjk_pre_modes_and_geometry_in_cli() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/line_breaking.html");
    let run = |format: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg("--bundled-fonts")
            .arg(&path)
            .arg(format!("--dump={format}"))
            .output()
            .unwrap();
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "{output:?}"
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let layout = run("layout");
    assert_eq!(layout, run("layout"));
    for text in [
        "https://",
        "example.com/a-",
        "你好，",
        "first",
        "second",
        "last",
        "\\t",
    ] {
        assert!(
            layout.contains(&format!("text \"{text}\"")),
            "{text}: {layout}"
        );
    }
    assert!(layout.contains("first=true last=false"));
    assert!(layout.contains("first=false last=true"));
    assert!(layout.contains("content-width=28.800"));
    let values = run("values");
    assert_eq!(values, run("values"));
    for value in [
        "computed white-space-collapse: Collapse",
        "computed white-space-collapse: Preserve",
        "computed white-space-collapse: PreserveBreaks",
        "computed white-space-collapse: BreakSpaces",
        "computed text-wrap-mode: Nowrap",
        "computed text-wrap-mode: Wrap",
        "used line: LineBox",
        "used inline-fragment: InlineFragment",
    ] {
        assert!(values.contains(value), "{value}: {values}");
    }
    let display = run("display-list");
    assert!(!display.contains("co\\u{ad}operate"));
    assert!(!display.contains("\\u{200b}"));
}
#[test]
fn inline_layout_demo_exposes_shared_baselines_sliced_fragments_and_alignment_in_cli() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/inline_layout.html");
    let run = |format: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg("--bundled-fonts")
            .arg(&path)
            .arg(format!("--dump={format}"))
            .output()
            .unwrap();
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "{output:?}"
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let layout = run("layout");
    assert_eq!(layout, run("layout"));
    assert_eq!(
        layout.matches("text \"Normal\" baseline=150.416").count(),
        2
    );
    assert!(layout.contains("text \"Large\" baseline=150.416"));
    assert!(layout.contains("text \"Small\" baseline=150.416"));
    for value in [
        "height=40.000 baseline=150.416 ascent=32.416 descent=7.584",
        "width: 158.81598, height: 31.792 } baseline=200.208 first=true last=false",
        "width: 173.13602, height: 31.792 } baseline=240.208 first=false last=true",
        "line x=156.000 y=276.000 width=300.000 height=24.000 baseline=294.208 ascent=18.208 descent=5.792 content-x=255.072 content-width=101.856",
        "line x=156.000 y=300.000 width=300.000 height=24.000 baseline=318.208 ascent=18.208 descent=5.792 content-x=272.624 content-width=66.752",
        "line x=156.000 y=352.000 width=300.000 height=24.000 baseline=370.208 ascent=18.208 descent=5.792 content-x=323.552 content-width=132.448",
        "line x=156.000 y=376.000 width=300.000 height=24.000 baseline=394.208 ascent=18.208 descent=5.792 content-x=389.248 content-width=66.752",
        "line x=156.000 y=428.000 width=300.000 height=24.000 baseline=446.208 ascent=18.208 descent=5.792 content-x=202.368 content-width=253.632",
    ] {
        assert!(layout.contains(value), "{value}: {layout}");
    }
    let display = run("display-list");
    for text in ["Normal", "Large", "Small"] {
        assert!(
            display.contains(&format!("baseline: 150.416,\n        text: \"{text}\"")),
            "{display}"
        );
    }
    assert!(
        !display.contains("h: 71.792"),
        "wrapped link must not paint its union rectangle"
    );
    let values = run("values");
    for value in [
        "used line: LineBox",
        "used inline-fragment: InlineFragment",
        "computed text-align: Center",
        "computed text-align: Right",
        "computed text-align: Start",
        "computed direction: Rtl",
        "computed vertical-align: Baseline",
    ] {
        assert!(values.contains(value), "{value}: {values}");
    }
}

#[test]
fn stylesheet_dump_preserves_document_and_declaration_order() {
    assert_eq!(
        dump("stylesheets"),
        include_str!("snapshots/stylesheets.expected")
    );
}

#[test]
fn typed_style_dump_is_wired_to_cli_and_deterministic_without_rasterization() {
    let output = dump("style");
    assert_eq!(output, dump("style"));
    assert!(output.contains("font-family: FontFamily([Generic(\"sans-serif\")])"));
    assert!(output.contains("width: Percent(80.0)"));
    assert!(output.contains("border-left-width: Px(3.0)"));
    assert!(output.contains("border-left-style: Solid"));
    assert!(output.contains("font-size: Px(20.0)"));
    assert!(output.contains("display: None"));
    assert!(!output.contains("#text"));
}

#[test]
fn modern_color_cli_dump_retains_color_spaces_and_is_deterministic() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/colors.html");
    let run = || {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(&path)
            .arg("--dump=style")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let dump = run();
    assert_eq!(dump, run());
    for value in [
        "space: DisplayP3",
        "space: Oklab",
        "space: Oklch",
        "space: Lab",
        "alpha: 0.6",
        "alpha: 0.2",
        "color: Rgb(0)",
        "background-color: Rgb(16777215)",
    ] {
        assert!(dump.contains(value), "{value}: {dump}");
    }
    let mix = &dump[dump.find("id=Some(\"mix\")").unwrap()..];
    let background = &mix[mix.find("background-color:").unwrap()..];
    let alpha = background
        .split("alpha: ")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!((alpha - 0.88).abs() < 1e-12);
    let values = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(&path)
        .arg("--dump=values")
        .output()
        .unwrap();
    assert!(
        values.status.success() && values.stderr.is_empty(),
        "{values:?}"
    );
    let values = String::from_utf8(values.stdout).unwrap();
    assert!(values.contains("specified Color: Relative("), "{values}");
    assert!(
        values.contains("computed color: ResolvedColor { space: Oklch"),
        "{values}"
    );
}

#[test]
fn values_dump_separates_specified_computed_used_and_actual_stages() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/values.html");
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(&path)
        .arg("--dump=values")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    for value in [
        "specified Width: SpecifiedLength(Math(",
        "computed width: Some(Math(",
        "used content-width: UsedLength(712.0)",
        "used content-width: UsedLength(300.0)",
        "used content-width: UsedLength(180.0)",
        "actual border-rect @ 1x: Some(ActualRect",
    ] {
        assert!(stdout.contains(value), "{value}: {stdout}");
    }
    let style = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .arg(&path)
        .arg("--dump=style")
        .output()
        .unwrap();
    assert!(style.status.success());
    assert!(
        String::from_utf8(style.stdout)
            .unwrap()
            .contains("width: Math(")
    );
}

#[test]
fn container_fixture_cli_completes_real_geometry_before_style_and_paint_dumps() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/containers.html");
    let run = |format: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(&path)
            .arg(format!("--dump={format}"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let style = run("style");
    assert_eq!(style, run("style"));
    assert!(style.contains("container-type: InlineSize"), "{style}");
    assert!(style.contains("width: Some(Px(32.0))"), "{style}");
    assert!(style.contains("width: Some(Px(18.0))"), "{style}");
    let display = run("display-list");
    assert!(display.contains("text: \"Roomy\""), "{display}");
    assert!(display.contains("text: \"Compact\""), "{display}");
    let values = run("values");
    assert!(
        values.contains("used content-width: UsedLength(320.0)"),
        "{values}"
    );
    assert!(
        values.contains("used content-width: UsedLength(180.0)"),
        "{values}"
    );
}

#[test]
fn used_percentage_math_failure_is_logged_to_stderr_without_claiming_a_typed_success() {
    let server = Server::new(
        vec![(
            "/",
            Reply::ok(
                "text/html",
                br#"<!doctype html><style>body{margin:0}</style>
                <div style='width:200px'>
                  <div style='width:calc(1000px / (100% - 200px) * 1px)'></div>
                </div>"#
                    .to_vec(),
            ),
        )],
        1,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=values", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    for context in [
        "CSS at Some(",
        "width: invalid at used-value time",
        "non-finite CSS length result",
        "value=Math(",
        "font-size=16px",
        "reference=200px",
        "compatibility fallback is 0px",
    ] {
        assert!(stderr.contains(context), "{stderr}");
    }
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("computed width: Some(Math("), "{stdout}");
    assert!(
        stdout.contains("used content-width: UsedLength(0.0)"),
        "{stdout}"
    );
    server.finish();
}

#[test]
fn math_and_unit_variables_from_redirected_cached_external_css_reach_geometry() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<!doctype html>
        <style>:root{font-size:20px;--g:2rem} body{margin:0}</style>
        <link rel='stylesheet' href='math.css'><link rel='stylesheet' href='math.css'>
        <div id='target'>calc</div><div id='clamp'>clamp</div>"#
                        .to_vec(),
                ),
            ),
            ("/math.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    br#":root{--w:calc(100% - var(--g))}
        #target{width:var(--w);background:red}
        #clamp{width:clamp(100px,50%,300px);background:blue}"#
                        .to_vec(),
                ),
            ),
        ],
        3,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=values", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("used content-width: UsedLength(712.0)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("used content-width: UsedLength(300.0)"),
        "{stdout}"
    );
    assert!(stdout.contains("computed width: Some(Math("), "{stdout}");
    assert!(
        stdout.contains("actual border-rect @ 1x: Some(ActualRect"),
        "{stdout}"
    );
}

#[test]
fn custom_properties_from_redirected_cached_and_inline_sources_reach_cli_style_dump() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<!doctype html><style>
            :root {--theme:red; --size:20px} p {color:var(--theme); width:var(--size)}
            </style><link rel='stylesheet' href='theme.css'><link rel='stylesheet' href='theme.css'>
            <p id='target' style='--theme:purple !important; --empty:; --invalid:initial;
            margin:var(--empty) 3px; background:var(--invalid,blue)'>theme</p>"#
                        .to_vec(),
                ),
            ),
            ("/theme.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    br#"@media (min-width:600px) {:root {--size:40px}}
            p {--theme:green !important}"#
                        .to_vec(),
                ),
            ),
        ],
        3,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=style", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let target = &stdout[stdout.find("id=Some(\"target\")").unwrap()..];
    for value in [
        "color: Rgb(8388736)",
        "background-color: Rgb(255)",
        "width: Px(40.0)",
        "margin-top: Px(3.0)",
        "--empty: Tokens([])",
        "--invalid: GuaranteedInvalid",
        "--theme: Tokens([Ident(\"purple\")",
    ] {
        assert!(target.contains(value), "{value}: {target}");
    }
    assert_eq!(server.finish(), ["/", "/theme.css", "/final.css"]);
}

#[test]
fn typed_shorthands_and_diagnostics_from_redirected_cached_and_inline_sources_reach_style_dump() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<!doctype html><style>\
                main {color:navy; font:20px/2 serif} #target {border:8px dashed green}</style>\
                <link rel='stylesheet' href='typed.css'><link rel='stylesheet' href='typed.css'>\
                <link rel='stylesheet' href='missing.css'>\
                <style>#target {width:40%; margin-left:9px !important;\
                border-right-color:purple !important}</style><main id='parent'>\
                <p id='target' style='font-weight:550.5; background:none; background:url(bad);\
                border-left:hidden !important; margin-top:4px'>target</p>\
                <span id='hidden' style='display:none; font:inherit'>hidden</span></main>"
                        .to_vec(),
                ),
            ),
            ("/typed.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    br#"#target {border:4px solid rgb(1, 2, 3);
                font:oblique 650 150%/1.25 "Noto Sans", serif; margin:1em 2%; padding:3px;
                background:red; background:red, blue; broken;}
                #hidden {font:40px monospace}"#
                        .to_vec(),
                ),
            ),
            (
                "/missing.css",
                Reply {
                    status: 404,
                    ..Reply::ok("text/css", Vec::new())
                },
            ),
        ],
        4,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=style", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let target_start = stdout.find("id=Some(\"target\")").unwrap();
    let hidden_start = stdout.find("id=Some(\"hidden\")").unwrap();
    assert!(stdout.find("id=Some(\"parent\")").unwrap() < target_start);
    assert!(target_start < hidden_start);
    let target = &stdout[target_start..hidden_start];
    for value in [
        "font-size: Px(30.0)",
        "font-weight: FontWeight(550.5)",
        "font-style: Oblique",
        "line-height: Number(1.25)",
        "Named(\"Noto Sans\")",
        "Generic(\"serif\")",
        "width: Percent(40.0)",
        "margin-top: Px(4.0)",
        "margin-right: Percent(2.0)",
        "margin-bottom: Px(30.0)",
        "margin-left: Px(9.0)",
        "padding-left: Px(3.0)",
        "border-top-width: Px(4.0)",
        "border-top-style: Solid",
        "border-top-color: Rgb(66051)",
        "border-right-color: Rgb(8388736)",
        "border-left-width: Px(0.0)",
        "border-left-style: Hidden",
        "background-color: Transparent",
        "background-image: None",
    ] {
        assert!(target.contains(value), "{value}: {target}");
    }
    let hidden = &stdout[hidden_start..];
    assert!(hidden.contains("display: None"));
    assert!(hidden.contains("font-size: Px(20.0)"));
    assert!(hidden.contains("line-height: Number(2.0)"));
    assert!(stderr.contains("CSS/resource"));
    assert!(stderr.contains("missing.css"));
    assert!(stderr.contains("final.css: invalid or unsupported CSS value: background: red, blue"));
    assert!(stderr.contains("CSS parse error"));
    assert!(stderr.contains("background: url(bad)"));
    assert!(stderr.contains("images, layers, and positioning are deferred"));
    assert_eq!(stderr.matches("background: red, blue").count(), 2);
    assert_eq!(
        server.finish(),
        ["/", "/typed.css", "/final.css", "/missing.css"]
    );
}

#[test]
fn css_token_dump_includes_stylesheets_inline_values_and_source_positions() {
    let output = dump("css-tokens");
    assert!(output.contains("Stylesheet (<style>, node"));
    assert!(output.contains("Inline style (node"));
    assert!(output.contains("Ident(\"color\")"));
    assert!(output.contains("Dimension {"));
    assert!(output.contains("Hash {"));
    assert!(output.contains("(bytes "));
    assert!(output.contains("Eof"));
    assert!(!output.contains("error "));
}

#[test]
fn css_token_dump_reports_the_exact_omitted_diagnostic_count() {
    let limit = rust_browser::css::tokenizer::MAX_PARSE_ERRORS;
    let output = rust_browser::devtools::format_css_tokens(&"\\\n".repeat(limit + 2));
    assert_eq!(output.matches("  error ").count(), limit);
    assert!(output.contains("2 additional CSS tokenization errors omitted"));
    assert!(output.contains("Eof"));
}

#[test]
fn css_token_dump_retains_recovered_sources_redirect_urls_and_cached_sheets() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<style>p { color: red }</style>\
                    <link rel='stylesheet' href='sheet.css'>\
                    <link rel='stylesheet' href='sheet.css'>\
                    <link rel='stylesheet' href='broken.css'>\
                    <style>p { content: \"embedded\nrest }</style>\
                    <style type='text/plain'>not-css</style>\
                    <p style='width: 2px'>tokens</p>"
                        .to_vec(),
                ),
            ),
            ("/sheet.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok("text/css", b"\xef\xbb\xbfp { margin: 3px }".to_vec()),
            ),
            (
                "/broken.css",
                Reply::ok("text/css", b"a { content: \"external".to_vec()),
            ),
        ],
        4,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=css-tokens", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let external = format!(
        "Stylesheet ({}, node",
        server.url.join("final.css").unwrap()
    );
    assert_eq!(stdout.matches(&external).count(), 2);
    assert!(stdout.contains(&format!(
        "Stylesheet ({}, node",
        server.url.join("broken.css").unwrap()
    )));
    let embedded = stdout.find("Stylesheet (<style>, node").unwrap();
    let external = stdout.find(&external).unwrap();
    let rejected = stdout.find("eof-in-string").unwrap();
    let last_embedded = stdout.rfind("Stylesheet (<style>, node").unwrap();
    assert!(embedded < external && external < rejected && rejected < last_embedded);
    assert!(stdout.contains("String(\"external\")"));
    assert!(stdout.contains("BadString"));
    assert!(stdout.contains("newline-in-string"));
    assert!(stdout.contains("Inline style (node"));
    assert!(!stdout.contains("not-css"));
    assert!(!stdout.contains("\\u{feff}"));
    assert!(stderr.contains("CSS/resource"));
    assert!(stderr.contains("broken.css"));
    assert_eq!(
        server.finish(),
        ["/", "/sheet.css", "/final.css", "/broken.css"]
    );
}

#[test]
fn styled_layout_and_paint_are_wired_to_the_cli() {
    let layout = dump("layout");
    assert!(layout.contains("<span> Block"));
    assert!(!layout.contains("Hidden content."));
    let paint = dump("display-list");
    assert!(
        paint.contains("color: 13382400"),
        "inline #c30 color should reach paint"
    );
    assert!(
        paint.contains("size: 20.0"),
        "relative inline font size should reach paint"
    );
    assert!(
        paint.contains(&format!("color: {}", 0xEEEEFF)),
        "#eef background should reach paint"
    );
    assert!(
        paint.contains("color: 6697881"),
        "#639 border should reach paint"
    );
    assert!(!paint.contains("Hidden"));
}

#[test]
fn media_width_queries_use_the_default_css_viewport_in_all_cli_stages() {
    for format in ["stylesheets", "style", "layout", "display-list"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/media.html"))
            .arg(format!("--dump={format}"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        match format {
            "stylesheets" => {
                assert!(stdout.contains("@media screen and (max-width: 37.5em)"));
                assert!(stdout.contains("@media (max-width: 400px)"));
            }
            "style" => {
                let start = stdout.find("id=Some(\"panel\")").unwrap();
                let end = stdout[start..].find("id=Some(\"wide\")").unwrap() + start;
                let panel = &stdout[start..end];
                assert!(panel.contains("background-color: Rgb(11393254)"), "{panel}");
                assert!(panel.contains("border-top-width: Px(5.0)"), "{panel}");
            }
            "layout" => {
                assert!(stdout.contains("Wide layout: more than 600 CSS pixels."));
                assert!(!stdout.contains("Narrow layout: at most 600 CSS pixels."));
            }
            _ => {
                assert!(stdout.contains("w: 800.0,"));
                assert!(stdout.contains("h: 600.0,"));
                assert!(stdout.contains("color: 11393254"));
                assert!(!stdout.contains("Narrow layout"));
            }
        }
    }
}

#[test]
fn expanded_selectors_from_redirected_cached_sheets_reach_layout_and_paint() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<!doctype html><style>p {color:red}</style>\
                    <link rel='stylesheet' href='selectors.css'>\
                    <link rel='stylesheet' href='selectors.css'>\
                    <main><!-- comment --><p data-kind='Notice'>First</p>\
                    text <p>Next</p><p hidden>Hidden</p><span>Last</span></main>"
                        .to_vec(),
                ),
            ),
            ("/selectors.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    b"main > p[data-kind=notice i]:first-child {color:#123456}\
                    main > p + p:not([hidden], .skip) {color:#654321}\
                    main > p ~ span:last-child {background-color:#abcdef}\
                    main > [hidden] {display:none}"
                        .to_vec(),
                ),
            ),
        ],
        3,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=display-list", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paint = String::from_utf8(output.stdout).unwrap();
    for color in [0x123456, 0x654321, 0xABCDEF] {
        assert!(paint.contains(&format!("color: {color}")), "{paint}");
    }
    for text in ["First", "Next", "Last"] {
        assert!(paint.contains(text), "{paint}");
    }
    assert!(!paint.contains("Hidden"), "{paint}");
    assert_eq!(server.finish(), ["/", "/selectors.css", "/final.css"]);
}

#[test]
fn important_and_wide_keywords_from_all_author_paths_reach_cli_paint() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<style>main {color:navy; font-size:24px}\
                    #important, #inline {color:red !important}\
                    #keywords {font-size:40px; font-weight:bold}\
                    #reset {display:none}</style>\
                    <link rel='stylesheet' href='cascade.css'>\
                    <style>#order {color:#654321 !important}</style>\
                    <main><p id='important' style='color:purple'>Important</p>\
                    <p id='inline' style='color:#abcdef !important'>InlineImportant</p>\
                    <h1 id='keywords'>Initial</h1><p id='order'>Ordered</p>\
                    <p id='reset' style='display:initial !important'>Visible</p></main>"
                        .to_vec(),
                ),
            ),
            (
                "/cascade.css",
                Reply::ok(
                    "text/css",
                    b"#important {color:#123456 !important; color:invalid !important}\
                    #inline {color:blue !important}\
                    #keywords {font-size:initial !important; font-weight:initial !important;\
                    color:inherit !important; margin:unset !important}\
                    #order {color:green !important} #reset {display:none !important}"
                        .to_vec(),
                ),
            ),
        ],
        2,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=display-list", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let diagnostics = String::from_utf8(output.stderr).unwrap();
    assert!(
        diagnostics.contains("invalid or unsupported CSS value: color: invalid"),
        "{diagnostics}"
    );
    let paint = String::from_utf8(output.stdout).unwrap();
    for color in [0x123456, 0xABCDEF, 0x654321, 0x000080] {
        assert!(paint.contains(&format!("color: {color}")), "{paint}");
    }
    for text in [
        "Important",
        "InlineImportant",
        "Initial",
        "Ordered",
        "Visible",
    ] {
        assert!(paint.contains(text), "{paint}");
    }
    assert!(paint.contains("size: 16.0"), "{paint}");
    assert!(paint.contains("size: 24.0"), "{paint}");
    assert_eq!(server.finish(), ["/", "/cascade.css"]);
}
