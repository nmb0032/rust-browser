mod support;

use base64::Engine;
use rust_browser::{
    css::{
        media::MediaEnvironment,
        values::{FamilyName, FontFamily},
    },
    engine::{Frame, LoadEvent, LoadOptions, Navigation, load_page},
    style::ComputedStyle,
    text::{FontContext, ShapedText},
};
use std::{
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use support::{Reply, Server};

const REGULAR: &[u8] = include_bytes!("../assets/fonts/noto-sans/NotoSans.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/noto-sans/NotoSansBold-fixture.ttf");
const ITALIC: &[u8] = include_bytes!("../assets/fonts/noto-sans/NotoSansItalic-fixture.ttf");
const WOFF: &[u8] = include_bytes!("../assets/fonts/noto-sans/NotoSansItalic-fixture.woff");
const WOFF2: &[u8] = include_bytes!("../assets/fonts/noto-sans/NotoSansItalic-fixture.woff2");

fn style(frame: &Frame, id: &str) -> ComputedStyle {
    let node = frame
        .document
        .preorder()
        .find(|&n| {
            frame
                .document
                .node(n)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(id))
        })
        .unwrap();
    frame.styles.get(node).unwrap().clone()
}

fn run(frame: &Frame, id: &str, text: &str) -> Arc<ShapedText> {
    frame.font_context().shape(&style(frame, id), text)
}

fn load(server: &Server) -> Frame {
    load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap()
}

#[test]
fn downloaded_faces_match_weight_style_ranges_and_share_sources_across_relayouts() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<style>
          @font-face {font-family:Fixture; src:url(regular.ttf)}
          @font-face {font-family:Fixture; src:url(bold.ttf); font-weight:700}
          @font-face {font-family:Fixture; src:url(italic.ttf); font-style:italic}
          @font-face {font-family:Split; src:url(bold.ttf); unicode-range:U+0041}
          @font-face {font-family:Split; src:url(italic.ttf); unicode-range:U+0056}
          p {font-family:Fixture; font-size:32px}
          #bold {font-weight:700} #italic {font-style:italic}
          #split {font-family:Split} #units {font-style:italic; width:10ch}
        </style><p id=regular>AVffi</p><p id=bold>AVffi</p><p id=italic>AVffi</p>
        <p id=split>AVx</p><p id=units>water</p>"#
                        .to_vec(),
                ),
            ),
            ("/regular.ttf", Reply::ok("font/ttf", REGULAR)),
            ("/bold.ttf", Reply::ok("font/ttf", BOLD)),
            ("/italic.ttf", Reply::ok("font/ttf", ITALIC)),
        ],
        4,
    );
    let mut frame = load(&server);
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("default instance")),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    let regular = run(&frame, "regular", "AVffi");
    let bold = run(&frame, "bold", "AVffi");
    let italic = run(&frame, "italic", "AVffi");
    assert_ne!(regular.width, bold.width);
    assert_ne!(regular.width, italic.width);
    assert_eq!(bold.glyphs[0].face.weight, 700);
    assert!(bold.glyphs.iter().all(|g| !g.bold && g.id != 0));
    assert!(
        italic
            .glyphs
            .iter()
            .all(|g| g.face.italic && !g.italic && g.id != 0)
    );
    let split = run(&frame, "split", "AVx");
    assert_eq!(split.glyphs[0].face.weight, 700);
    assert!(split.glyphs[1].face.italic);
    assert!(!split.glyphs[2].face.italic);
    assert!(Arc::ptr_eq(&split.glyphs[0].face, &bold.glyphs[0].face));
    assert!(Arc::ptr_eq(&split.glyphs[1].face, &italic.glyphs[0].face));
    assert!(Arc::ptr_eq(
        &frame.font_context().select(&style(&frame, "split")),
        &rust_browser::text::bundled_face()
    ));
    let expected_ch = italic.glyphs[0].face.metrics('0', 32.0).advance_width * 10.0;
    assert_eq!(
        style(&frame, "units").width,
        Some(rust_browser::css::values::Length::Px(expected_ch))
    );
    let pixels = italic.glyphs[0]
        .face
        .rasterized(italic.glyphs[0].id, 32.0)
        .unwrap();
    assert!(pixels.pixels.iter().any(|&p| p != 0));
    assert_ne!(
        pixels.pixels,
        regular.glyphs[0]
            .face
            .rasterized(regular.glyphs[0].id, 32.0)
            .unwrap()
            .pixels
    );
    let fonts = FontContext::bundled();
    let tree = frame.layout_with_container_queries(&fonts);
    for width in [500.0, 800.0, 500.0, 800.0] {
        let mut clone = frame.clone();
        clone.set_media_environment(MediaEnvironment::new(width, 600.0));
        clone.layout_with_container_queries(&fonts);
        assert!(Arc::ptr_eq(
            &run(&clone, "italic", "AVffi").glyphs[0].face,
            &italic.glyphs[0].face
        ));
        assert_eq!(style(&clone, "units").width, style(&frame, "units").width);
    }
    assert_eq!(tree, frame.layout_with_container_queries(&fonts));
    assert_eq!(
        server.finish(),
        ["/", "/regular.ttf", "/bold.ttf", "/italic.ttf"]
    );
}

#[test]
fn sheet_import_redirect_base_and_css_escapes_select_real_woff2_glyphs() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<base href='/assets/'><link rel=stylesheet href='start.css'>
          <p id=target>AVffi</p>"#
                        .to_vec(),
                ),
            ),
            ("/assets/start.css", Reply::redirect("/css/main.css")),
            (
                "/css/main.css",
                Reply::ok(
                    "text/css",
                    br#"@import "../imports/fonts.css" layer(web);
          p {font-family:"Web Face";font-style:italic}"#
                        .to_vec(),
                ),
            ),
            ("/imports/fonts.css", Reply::redirect("/final/fonts.css")),
            (
                "/final/fonts.css",
                Reply::ok(
                    "text/css",
                    br#"@font-face {
          font-family:Web\20 Face; src:url(font\2e woff2) format("woff2");font-style:italic}"#
                        .to_vec(),
                ),
            ),
            ("/final/font.woff2", Reply::ok("font/woff2", WOFF2)),
        ],
        6,
    );
    let mut progress = Vec::new();
    let frame = load_page(
        &server.url,
        LoadOptions::default(),
        |frame| {
            progress.push(frame.clone());
            Ok(())
        },
        || false,
    )
    .unwrap();
    let glyphs = run(&frame, "target", "AVffi");
    assert!(
        glyphs
            .glyphs
            .iter()
            .all(|g| g.face.italic && !g.italic && g.id != 0)
    );
    let mut database = fontdb::Database::new();
    database.load_font_data(ITALIC.to_vec());
    let expected = FontContext::from_database(database).shape(
        &ComputedStyle {
            font_family: FontFamily(vec![FamilyName::Named("Noto Sans".into())]),
            font_style: rust_browser::css::values::FontStyle::Italic,
            italic: true,
            ..Default::default()
        },
        "AVffi",
    );
    assert_eq!(
        glyphs.glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
        expected.glyphs.iter().map(|g| g.id).collect::<Vec<_>>()
    );
    assert!((glyphs.width - expected.width).abs() < 0.001);
    assert!(
        progress
            .iter()
            .any(|f| !run(f, "target", "AVffi").glyphs[0].face.italic)
    );
    assert!(
        progress
            .iter()
            .any(|f| run(f, "target", "AVffi").glyphs[0].face.italic)
    );
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    assert_eq!(
        server.finish(),
        [
            "/",
            "/assets/start.css",
            "/css/main.css",
            "/imports/fonts.css",
            "/final/fonts.css",
            "/final/font.woff2"
        ]
    );
}

#[test]
fn inactive_media_supports_and_layers_do_not_fetch_and_activation_has_a_separate_resource_phase() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<style>
          @layer low, high;
          @layer high {@font-face {font-family:Choice;src:url(bold.ttf)}}
          @layer low {@font-face {font-family:Choice;src:url(italic.ttf)}}
          @media (max-width:500px) {@font-face {font-family:Narrow;src:url(narrow.woff)}}
          @supports (display:grid) {@font-face {font-family:Unsupported;src:url(must-not-fetch.ttf)}}
          @media print {@font-face {font-family:Print;src:url(must-not-fetch.ttf)}}
          p {font-family:Choice} @media (max-width:500px) {p {font-family:Narrow}}
        </style><p id=target>AVffi</p>"#.to_vec())),
        ("/bold.ttf", Reply::ok("font/ttf", BOLD)),
        ("/italic.ttf", Reply::ok("font/ttf", ITALIC)),
        ("/narrow.woff", Reply::ok("font/woff", WOFF)),
    ], 4);
    let mut frame = load(&server);
    assert_eq!(run(&frame, "target", "AVffi").glyphs[0].face.weight, 700);
    frame.set_media_environment(MediaEnvironment::new(400.0, 600.0));
    assert!(frame.has_pending_web_fonts());
    frame.layout_with_container_queries(&FontContext::bundled());
    assert!(!run(&frame, "target", "AVffi").glyphs[0].face.italic);
    frame.load_pending_web_fonts(|_| Ok(()), || false).unwrap();
    let narrow = run(&frame, "target", "AVffi");
    assert!(narrow.glyphs[0].face.italic);
    assert!(!frame.has_pending_web_fonts());
    for width in [800.0, 400.0, 800.0, 400.0] {
        frame.set_media_environment(MediaEnvironment::new(width, 600.0));
        assert!(!frame.has_pending_web_fonts());
        frame.layout_with_container_queries(&FontContext::bundled());
    }
    assert!(Arc::ptr_eq(
        &narrow.glyphs[0].face,
        &run(&frame, "target", "AVffi").glyphs[0].face
    ));
    assert_eq!(
        server.finish(),
        ["/", "/italic.ttf", "/bold.ttf", "/narrow.woff"]
    );
}

#[test]
fn src_recovery_local_data_formats_failures_and_descriptor_errors_use_real_fallback() {
    let data = base64::engine::general_purpose::STANDARD.encode(WOFF);
    let html = format!(
        r#"<style>
      @font-face {{font-family:Recovered;src:url(no.svg) format(svg), nonsense(), url(missing.ttf), url(broken.woff2), url(font.woff) format(woff); font-style:italic; font-display:optional; size-adjust:120%}}
      @font-face {{font-family:Local;src:local("Noto Sans Regular"),url(must-not-fetch.ttf)}}
      @font-face {{font-family:Data;src:url("data:font/woff;base64,{data}");font-style:italic}}
      @font-face {{font-family:Broken;src:url(broken.woff2)}}
      @font-face {{font-family:Invalid;src:nonsense()}}
      @font-face {{font-family:Recovered;src:broken();font-weight:0;font-style:oblique 40deg;unicode-range:U+FFFFFF}}
      #recovered {{font-family:Recovered;font-style:italic}}
      #local {{font-family:Local}} #data {{font-family:Data;font-style:italic}}
      #broken {{font-family:Broken, sans-serif}}
      </style><p id=recovered>AVffi</p><p id=local>AVffi</p><p id=data>AVffi</p><p id=broken>AVffi</p>"#
    );
    let server = Server::new(
        vec![
            ("/", Reply::ok("text/html", html.into_bytes())),
            (
                "/missing.ttf",
                Reply {
                    status: 404,
                    headers: vec![],
                    body: vec![],
                },
            ),
            ("/broken.woff2", Reply::ok("font/woff2", b"wOF2invalid")),
            ("/font.woff", Reply::ok("font/woff", WOFF)),
        ],
        4,
    );
    let frame = load(&server);
    let recovered = run(&frame, "recovered", "AVffi");
    assert!(recovered.glyphs[0].face.italic);
    assert_eq!(recovered.width, run(&frame, "data", "AVffi").width);
    assert!(Arc::ptr_eq(
        &run(&frame, "local", "AVffi").glyphs[0].face,
        &rust_browser::text::bundled_face()
    ));
    let fallback = run(&frame, "broken", "AVffi");
    assert!(fallback.glyphs.iter().all(|g| g.id != 0 && !g.face.italic));
    let messages = frame
        .stylesheets
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for fragment in [
        "src candidate",
        "source failed",
        "font-display:optional",
        "size-adjust",
        "font-weight",
        "unicode-range",
    ] {
        assert!(messages.contains(fragment), "{messages}");
    }
    assert_eq!(
        server.finish(),
        ["/", "/missing.ttf", "/broken.woff2", "/font.woff"]
    );
}

#[test]
fn cross_origin_fonts_need_cors_and_redirect_aliases_share_one_decoded_face() {
    let mut allowed = Reply::ok("font/woff2", WOFF2);
    allowed
        .headers
        .push(("Access-Control-Allow-Origin", "*".into()));
    let mut redirect = Reply::redirect("/allowed");
    redirect
        .headers
        .push(("Access-Control-Allow-Origin", "*".into()));
    let fonts = Server::new(
        vec![
            ("/denied", Reply::ok("font/woff2", WOFF2)),
            ("/redirect", redirect),
            ("/allowed", allowed),
        ],
        3,
    );
    let html = format!(
        r#"<style>
        @font-face {{font-family:Cors;src:url("{}"),url("{}");font-style:italic}}
        @font-face {{font-family:Alias;src:url("{}");font-style:italic}}
        @font-face {{font-family:Blocked;src:url(file:///private/font.ttf),url(javascript:font)}}
        #target {{font-family:Cors;font-style:italic}} #alias {{font-family:Alias;font-style:italic}}
        </style><p id=target>AVffi</p><p id=alias>AVffi</p>"#,
        fonts.url.join("denied").unwrap(),
        fonts.url.join("redirect").unwrap(),
        fonts.url.join("allowed").unwrap()
    );
    let server = Server::new(vec![("/", Reply::ok("text/html", html.into_bytes()))], 1);
    let frame = load(&server);
    let target = run(&frame, "target", "AVffi");
    assert!(target.glyphs[0].face.italic);
    assert!(Arc::ptr_eq(
        &target.glyphs[0].face,
        &run(&frame, "alias", "AVffi").glyphs[0].face
    ));
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("CORS"))
    );
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("blocked font URL"))
    );
    assert_eq!(fonts.finish(), ["/denied", "/redirect", "/allowed"]);
    server.finish();
}

#[test]
fn snapshots_navigation_isolation_and_cancelled_font_work_do_not_poison_shared_fonts() {
    let html = br#"<style>
      @font-face {font-family:"Noto Sans";src:url(font.woff2);font-style:italic}
      @font-face {font-family:Next;src:url(must-not-fetch.ttf)}
      p {font-family:"Noto Sans";font-style:italic}
      </style><p id=target>AVffi</p>"#;
    let server = Server::new(
        vec![
            ("/", Reply::ok("text/html", html)),
            ("/font.woff2", Reply::ok("font/woff2", WOFF2)),
        ],
        2,
    );
    let cancelled = AtomicBool::new(false);
    let mut snapshot = None;
    let result = load_page(
        &server.url,
        LoadOptions::default(),
        |frame| {
            if run(frame, "target", "AVffi").glyphs[0].face.italic {
                snapshot = Some(frame.clone());
                cancelled.store(true, Ordering::Relaxed);
            }
            Ok(())
        },
        || cancelled.load(Ordering::Relaxed),
    );
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    let snapshot = snapshot.unwrap();
    let font = FontContext::bundled();
    let mut frame = snapshot.clone();
    frame.layout_with_container_queries(&font);
    assert!(run(&frame, "target", "AVffi").glyphs[0].face.italic);
    assert!(!font.select(&style(&frame, "target")).italic);
    let other = Server::new(
        vec![(
            "/",
            Reply::ok(
                "text/html",
                b"<p id=target style='font-family:\"Noto Sans\";font-style:italic'>AVffi</p>",
            ),
        )],
        1,
    );
    let mut other_frame = load(&other);
    other_frame.layout_with_container_queries(&font);
    assert!(!run(&other_frame, "target", "AVffi").glyphs[0].face.italic);
    assert!(run(&other_frame, "target", "AVffi").glyphs[0].italic);
    assert_eq!(server.finish(), ["/", "/font.woff2"]);
    other.finish();
}

#[test]
fn viewport_font_worker_reuses_document_resources_in_navigation_channel() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    br#"<style>@media (max-width:500px) {
          @font-face {font-family:Responsive;src:url(font.woff2);font-style:italic}}
          p {font-family:Responsive;font-style:italic}</style><p id=target>AVffi</p>"#
                        .to_vec(),
                ),
            ),
            ("/font.woff2", Reply::ok("font/woff2", WOFF2)),
        ],
        2,
    );
    let mut frame = load(&server);
    frame.set_media_environment(MediaEnvironment::new(400.0, 600.0));
    let mut navigation = Navigation::new(server.url.clone(), LoadOptions::default());
    navigation.start_font_load(Arc::new(frame)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut loaded = None;
    loop {
        match navigation.try_recv() {
            Ok(LoadEvent::Progress(frame)) => loaded = Some(frame),
            Ok(LoadEvent::Finished) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("{error}"),
        }
    }
    assert!(
        run(&loaded.unwrap(), "target", "AVffi").glyphs[0]
            .face
            .italic
    );
    assert!(!navigation.is_font_loading());
    assert_eq!(server.finish(), ["/", "/font.woff2"]);
}

#[test]
fn licensed_webfont_page_runs_through_cli_in_bundled_and_production_modes() {
    for bundled in [true, false] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rust-browser"));
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/web_fonts.html"))
            .arg("--dump=layout");
        if bundled {
            command.arg("--bundled-fonts");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains("Noto Sans Italic"), "{output}");
        assert!(
            output.contains("glyph=") && output.contains("cluster="),
            "{output}"
        );
    }
}
