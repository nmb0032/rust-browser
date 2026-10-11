mod support;

use std::sync::Arc;

use rust_browser::{
    css::{Rule, media::MediaEnvironment, values::Length},
    engine::{Frame, LoadOptions, load_page},
    style,
};
use support::{Reply, Server};

fn id(frame: &Frame, name: &str) -> rust_browser::dom::NodeId {
    frame
        .document
        .preorder()
        .find(|&id| {
            frame
                .document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

#[test]
fn imported_functional_selectors_and_generated_pseudos_restyle_without_refetching() {
    use rust_browser::style::{InteractionState, PseudoElement};
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html><style>@layer base, app; @import 'selectors.css' layer(base);
          @layer app {#target::before {color:revert-layer}}
          </style><main><p id='target' class='item' data-label='Label' style="--prefix:'Imported-'; color:red">Origin</p><aside class='next'></aside></main>"#.to_vec())),
        ("/selectors.css", Reply::ok("text/css", br#"@namespace h 'http://www.w3.org/1999/xhtml';
          h|p:is(.item,:unsupported):has(+ .next):nth-child(1 of h|p)::before {content:var(--prefix) attr(data-label); color:blue}
          @media (min-width:600px) {h|p::after {content:'Wide'; color:green}}
          h|p:hover::after {content:'Hover'; color:purple}"#.to_vec())),
    ], 2);
    let mut frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    let target = id(&frame, "target");
    let element = frame
        .document
        .node(target)
        .unwrap()
        .as_element()
        .unwrap()
        .clone();
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    assert_eq!(frame.styles.get(target).unwrap().color, Some(0xff0000));
    assert_eq!(
        frame
            .styles
            .pseudo(target, PseudoElement::Before)
            .unwrap()
            .content
            .text(&element)
            .as_deref(),
        Some("Imported-Label")
    );
    assert_eq!(
        frame
            .styles
            .pseudo(target, PseudoElement::Before)
            .unwrap()
            .color,
        Some(0x0000ff)
    );
    assert!(frame.styles.pseudo(target, PseudoElement::After).is_some());
    let retained = Arc::clone(&frame.stylesheets.stylesheets[0].stylesheet);
    frame.set_media_environment(MediaEnvironment::new(400.0, 600.0));
    assert!(frame.styles.pseudo(target, PseudoElement::After).is_none());
    frame.set_interaction_state(InteractionState {
        hovered: Some(target),
        ..InteractionState::default()
    });
    assert_eq!(
        frame
            .styles
            .pseudo(target, PseudoElement::After)
            .unwrap()
            .content
            .text(&element)
            .as_deref(),
        Some("Hover")
    );
    assert!(Arc::ptr_eq(
        &retained,
        &frame.stylesheets.stylesheets[0].stylesheet
    ));
    assert!(
        rust_browser::devtools::format_style(&frame.document, &frame.styles)
            .contains("::before computed (separate target)")
    );
    assert_eq!(server.finish(), ["/", "/selectors.css"]);
}

#[test]
fn imported_generated_content_reaches_cli_paint() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html><style>@import 'generated.css'; body {margin:0}</style><p data-label='Label'>Origin</p>"#.to_vec())),
        ("/generated.css", Reply::ok("text/css", b"p:has(+ *)::before {content:'Wrong'} p:is(:unsupported,p)::before {content:'Before-' attr(data-label); color:blue} p::after {content:'After'; color:green}".to_vec())),
    ], 2);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=display-list", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for text in ["Before-Label", "Origin", "After"] {
        assert!(stdout.contains(&format!("text: \"{text}\"")), "{stdout}");
    }
    assert!(!stdout.contains("Wrong"));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.finish(), ["/", "/generated.css"]);
}

#[test]
fn imported_container_queries_keep_media_layers_namespace_and_source_order_without_feedback_fetches()
 {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<!doctype html><style>
              @layer base, app; @import 'containers.css' layer(base) (width >= 500px);
              #container {container:Panel / inline-size; width:40vw}
              @layer app {@container Panel (width >= 300px) {#card {color:revert-layer}}}
              </style><div id='container'><p id='card'>Conditional import</p></div>"#
                        .to_vec(),
                ),
            ),
            (
                "/containers.css",
                Reply::ok(
                    "text/css",
                    br#"@namespace h 'http://www.w3.org/1999/xhtml';
              h|p {color:red; width:20px}
              @container Panel (width >= 300px) {
                h|p {color:blue; width:10cqi}
                h|p {color:green}
              }"#
                    .to_vec(),
                ),
            ),
        ],
        2,
    );
    let mut frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    let target = id(&frame, "card");
    let retained = Arc::clone(&frame.stylesheets.stylesheets[0].stylesheet);
    let font = rust_browser::text::FontContext::bundled();
    for (viewport, color, width) in [
        (800.0, 0x008000, Some(32.0)),
        (600.0, 0xff0000, Some(20.0)),
        (1000.0, 0x008000, Some(40.0)),
        (400.0, 0x000000, None),
        (800.0, 0x008000, Some(32.0)),
    ] {
        frame.set_media_environment(MediaEnvironment::new(viewport, 600.0));
        let _tree = frame.layout_with_container_queries(&font);
        assert_eq!(frame.styles.get(target).unwrap().color, Some(color));
        assert_eq!(
            frame.styles.get(target).unwrap().width,
            width.map(Length::Px)
        );
        assert!(
            frame.stylesheets.diagnostics.is_empty(),
            "{:?}",
            frame.stylesheets.diagnostics
        );
        #[cfg(feature = "style-bench-reference")]
        for mode in [
            style::BenchmarkMatching::Unindexed,
            style::BenchmarkMatching::Indexed,
        ] {
            let snapshot = style::ContainerSnapshot::from_layout(&_tree, frame.media_environment());
            let reference = style::resolve_for_benchmark_with_containers(
                &frame.document,
                frame.stylesheets.clone(),
                &[],
                mode,
                frame.media_environment(),
                frame.interaction_state(),
                &snapshot,
            );
            for node in frame.document.preorder() {
                assert_eq!(reference.styles.get(node), frame.styles.get(node));
            }
            assert_eq!(
                reference.stylesheets.diagnostics,
                frame.stylesheets.diagnostics
            );
        }
        assert!(Arc::ptr_eq(
            &retained,
            &frame.stylesheets.stylesheets[0].stylesheet
        ));
    }
    assert_eq!(server.finish(), ["/", "/containers.css"]);
}
#[test]
fn import_graph_retains_redirected_base_cache_conditions_layers_and_source_order_on_resize() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html><base href='/assets/'>
            <style>@layer base, theme; @import 'inline.css' layer(base) supports(display:block) screen; p {margin:3px}</style>
            <link rel='stylesheet' href='entry.css'><link rel='stylesheet' href='entry.css'>
            <link rel='stylesheet' href='/final/entry.css'>
            <p id='target'>nested imported text</p>"#.to_vec())),
        ("/assets/inline.css", Reply::ok("text/css", b"p {color:red; width:10px}".to_vec())),
        ("/assets/entry.css", Reply::redirect("/final/entry.css")),
        ("/final/entry.css", Reply::ok("text/css", br#"@import "child.css" layer(theme) supports((display:block) and selector(p)) (min-width:600px);
            @import "child.css" layer(base) supports(grid-template-columns:subgrid);
            @import "print.css" layer(theme) print;
            p {padding:4px}"#.to_vec())),
        ("/final/child.css", Reply::ok("text/css", b"p {color:blue; width:30px} @media (min-width:900px) {p {width:40px}}".to_vec())),
        ("/final/print.css", Reply::ok("text/css", b"p {color:purple; width:99px}".to_vec())),
    ], 6);
    let mut frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(frame.stylesheets.stylesheets.len(), 4);
    let sheets = &frame.stylesheets.stylesheets;
    assert!(Arc::ptr_eq(&sheets[1].stylesheet, &sheets[2].stylesheet));
    assert!(Arc::ptr_eq(&sheets[1].stylesheet, &sheets[3].stylesheet));
    let Rule::At(first) = &sheets[1].stylesheet.rules()[0] else {
        panic!("import")
    };
    let loaded = first.imported().unwrap();
    assert_eq!(loaded.url.path(), "/final/child.css");
    assert!(loaded.source.contains("width:30px"));
    let Rule::At(second) = &sheets[1].stylesheet.rules()[1] else {
        panic!("import")
    };
    assert!(
        second.imported().is_none(),
        "false supports must not expose a loaded import even when its URL is cached"
    );
    let target = id(&frame, "target");
    for (width, color, size) in [
        (400.0, 0xff0000, 10.0),
        (600.0, 0x0000ff, 30.0),
        (900.0, 0x0000ff, 40.0),
        (400.0, 0xff0000, 10.0),
    ] {
        frame.set_media_environment(MediaEnvironment::new(width, 600.0));
        let computed = frame.styles.get(target).unwrap();
        assert_eq!(computed.color, Some(color));
        assert_eq!(computed.width, Some(Length::Px(size)));
        assert_eq!(computed.padding.left, Length::Px(4.0));
        assert_eq!(computed.margin.top, Length::Px(3.0).into());
        #[cfg(feature = "style-bench-reference")]
        {
            let reference = style::resolve_for_benchmark_with_stylesheets(
                &frame.document,
                frame.stylesheets.clone(),
                style::BenchmarkMatching::Unindexed,
                frame.media_environment(),
            );
            for mode in [
                style::BenchmarkMatching::Indexed,
                style::BenchmarkMatching::IndexedWithBloom,
            ] {
                let result = style::resolve_for_benchmark_with_stylesheets(
                    &frame.document,
                    frame.stylesheets.clone(),
                    mode,
                    frame.media_environment(),
                );
                for node in frame.document.preorder() {
                    assert_eq!(result.styles.get(node), reference.styles.get(node));
                }
                assert_eq!(
                    result.stylesheets.diagnostics,
                    reference.stylesheets.diagnostics
                );
            }
        }
        let mut layout = frame.layout_tree();
        let font = rust_browser::text::FontContext::bundled();
        layout.layout(width, &font);
        assert!(rust_browser::devtools::format_values(&layout, 1.0).contains("Width"));
    }
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("unsupported @supports declaration"))
    );
    assert_eq!(
        server.finish(),
        [
            "/",
            "/assets/inline.css",
            "/assets/entry.css",
            "/final/entry.css",
            "/final/child.css",
            "/final/print.css"
        ]
    );
}

#[test]
fn cycles_failures_and_misplaced_imports_recover_without_refetching_and_do_not_drop_valid_rules() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html>
            <link rel='stylesheet' href='a.css'><link rel='stylesheet' href='a.css'>
            <style>@namespace svg 'http://www.w3.org/2000/svg'; @import 'not-loaded.css'; p {padding:2px}</style>
            <p id='target'>text</p>"#.to_vec())),
        ("/a.css", Reply::ok("text/css", b"@import 'b.css'; @import 'bad.css'; @import 'bad.css'; p {color:red; width:20px}".to_vec())),
        ("/b.css", Reply::ok("text/css", b"@import 'a.css'; p {color:blue; margin:4px}".to_vec())),
        ("/bad.css", Reply::ok("text/html", b"<html>not css</html>".to_vec())),
    ], 4);
    let mut frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    let target = id(&frame, "target");
    let computed = frame.styles.get(target).unwrap();
    assert_eq!(computed.color, Some(0xff0000));
    assert_eq!(computed.margin.top, Length::Px(4.0).into());
    assert_eq!(computed.padding.top, Length::Px(2.0));
    let messages: Vec<_> = frame
        .stylesheets
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    for expected in ["cycle", "not text/css", "@import must precede"] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{messages:?}"
        );
    }
    let diagnostics = frame.stylesheets.diagnostics.clone();
    frame.set_media_environment(MediaEnvironment::new(500.0, 500.0));
    assert_eq!(frame.stylesheets.diagnostics, diagnostics);
    assert_eq!(server.finish(), ["/", "/a.css", "/b.css", "/bad.css"]);
}

#[test]
fn imported_namespace_environment_does_not_escape_into_the_importing_sheet() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<!doctype html><style>@import 'svg.css';
            @namespace h 'http://www.w3.org/1999/xhtml'; h|a {color:green}
            s|a {color:purple}</style><a id='html'>html</a><svg><a id='svg'>svg</a></svg>"#
                        .to_vec(),
                ),
            ),
            (
                "/svg.css",
                Reply::ok(
                    "text/css",
                    b"@namespace s 'http://www.w3.org/2000/svg'; s|a {color:red}".to_vec(),
                ),
            ),
        ],
        2,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(
        frame.styles.get(id(&frame, "svg")).unwrap().color,
        Some(0xff0000)
    );
    assert_eq!(
        frame.styles.get(id(&frame, "html")).unwrap().color,
        Some(0x008000)
    );
    assert_eq!(frame.stylesheets.diagnostics.len(), 1);
    assert_eq!(server.finish(), ["/", "/svg.css"]);
}

#[test]
fn imported_registration_layer_and_nested_rules_reach_style_and_layout() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html><style>@layer framework, app;
            @import 'framework.css' layer(framework); @layer app {p {color:blue}}
            body {margin:0} </style><main><p id='target'>text</p></main>"#.to_vec())),
        ("/framework.css", Reply::ok("text/css", br#"@property --width {syntax:'<length>'; inherits:false; initial-value:80px}
            main { & > p {width:var(--width); color:red; background:yellow; margin:0; padding:2px} }"#.to_vec())),
    ], 2);
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    let target = frame.styles.get(id(&frame, "target")).unwrap();
    assert_eq!(target.color, Some(0x0000ff));
    assert_eq!(target.width, Some(Length::Px(80.0)));
    let dump = rust_browser::devtools::format_style(&frame.document, &frame.styles);
    assert!(dump.contains("width: Px(80.0)"), "{dump}");
    let font = rust_browser::text::FontContext::bundled();
    let mut layout = frame.layout_tree();
    layout.layout(200.0, &font);
    assert!(rust_browser::devtools::format_values(&layout, 1.0).contains("80"));
    let result = style::resolve_with_stylesheets(&frame.document, frame.stylesheets.clone());
    assert_eq!(result.styles.get(id(&frame, "target")), Some(target));
    assert_eq!(server.finish(), ["/", "/framework.css"]);
}

#[test]
fn imported_cascade_reaches_cli_display_list() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", b"<!doctype html><style>@import 'paint.css' layer(framework); body {margin:0}</style><p id='target'>imported paint</p>".to_vec())),
        ("/paint.css", Reply::ok("text/css", b"@property --width {syntax:'<length>'; inherits:false; initial-value:80px} @supports (display:block) {p {width:var(--width); background:yellow; color:blue; margin:0; padding:2px}}".to_vec())),
    ], 2);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args(["--dump=display-list", server.url.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("color: 16776960"), "{stdout}");
    assert!(stdout.contains("color: 255"), "{stdout}");
    assert!(
        stdout.contains("text: \"imported\"") && stdout.contains("text: \"paint\""),
        "{stdout}"
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.finish(), ["/", "/paint.css"]);
}

#[test]
fn false_supports_and_import_placement_do_not_trigger_network_requests() {
    let server = Server::new(
        vec![(
            "/",
            Reply::ok(
                "text/html",
                br#"<!doctype html><style>
            @layer default, theme; @import 'unsupported.css' supports(grid-template-columns:subgrid);
            @layer theme; @import 'misplaced.css'; p {color:blue}
            </style><p id='target'>text</p>"#
                    .to_vec(),
            ),
        )],
        1,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(
        frame.styles.get(id(&frame, "target")).unwrap().color,
        Some(0x0000ff)
    );
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("@import must precede"))
    );
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("no loaded stylesheet"))
    );
    assert_eq!(server.finish(), ["/"]);
}

#[test]
fn cached_ast_occurrences_still_create_distinct_anonymous_layer_paths() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<!doctype html><style>
            @import url("anonymous.css") layer(); @import 'named.css' layer(theme);
            @import url("anonymous.css") layer; </style><p id='target'>text</p>"#
                        .to_vec(),
                ),
            ),
            (
                "/anonymous.css",
                Reply::ok("text/css", b"@layer private {p {color:blue}}".to_vec()),
            ),
            (
                "/named.css",
                Reply::ok("text/css", b"p {color:red}".to_vec()),
            ),
        ],
        3,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    assert_eq!(
        frame.styles.get(id(&frame, "target")).unwrap().color,
        Some(0x0000ff)
    );
    let dump = rust_browser::devtools::format_stylesheets(&frame.stylesheets.stylesheets);
    assert!(
        dump.contains("Imported stylesheet") && dump.contains("anonymous.css"),
        "{dump}"
    );
    assert_eq!(server.finish(), ["/", "/anonymous.css", "/named.css"]);
}

#[test]
fn depth_limit_recovers_and_rest_of_sheet_still_applies() {
    let mut routes = vec![(
        "/",
        Reply::ok(
            "text/html",
            b"<!doctype html><link rel='stylesheet' href='depth0.css'><p id='target'>text</p>"
                .to_vec(),
        ),
    )];
    let paths: Vec<_> = (0..=16).map(|index| format!("/depth{index}.css")).collect();
    for (index, path) in paths.iter().enumerate() {
        routes.push((
            path.as_str(),
            Reply::ok(
                "text/css",
                format!(
                    "@import 'depth{}.css'; p {{width: {}px}}",
                    index + 1,
                    index + 20
                )
                .into_bytes(),
            ),
        ));
    }

    let server = Server::new(routes, 18);
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(
        frame.styles.get(id(&frame, "target")).unwrap().width,
        Some(Length::Px(20.0))
    );
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("depth exceeds 16"))
    );
    assert_eq!(server.finish().len(), 18);
}

#[test]
fn cached_cycle_graphs_are_path_specific_but_independent_repeated_links_share_asts() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html><link rel='stylesheet' href='a.css'>
            <link rel='stylesheet' href='b.css'><link rel='stylesheet' href='b.css'><p id='target'>text</p>"#.to_vec())),
        ("/a.css", Reply::ok("text/css", b"@import 'b.css'; p {color:red; width:10px}".to_vec())),
        ("/b.css", Reply::ok("text/css", b"@import 'a.css'; p {color:blue}".to_vec())),
    ], 3);
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(
        frame.styles.get(id(&frame, "target")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        frame.styles.get(id(&frame, "target")).unwrap().width,
        Some(Length::Px(10.0))
    );
    let sheets = &frame.stylesheets.stylesheets;
    assert!(Arc::ptr_eq(&sheets[1].stylesheet, &sheets[2].stylesheet));
    let Rule::At(b_in_a) = &sheets[0].stylesheet.rules()[0] else {
        panic!("import")
    };
    let Rule::At(blocked_a) = &b_in_a.imported().unwrap().stylesheet.rules()[0] else {
        panic!("import")
    };
    assert!(blocked_a.imported().is_none());
    let Rule::At(a_in_b) = &sheets[1].stylesheet.rules()[0] else {
        panic!("import")
    };
    assert!(
        a_in_b.imported().is_some(),
        "top-level B must include independent A, not cached B-within-A"
    );
    assert_eq!(server.finish(), ["/", "/a.css", "/b.css"]);
}
