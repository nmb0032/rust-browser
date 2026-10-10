mod support;

use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use image::{ImageBuffer, ImageFormat, Rgba};
use rust_browser::{
    dom::NodeId,
    engine::{Frame, LoadEvent, LoadOptions, Navigation, document_base_url, link_url, load_page},
    layout::LayoutBox,
    net, style,
};

use support::{Reply, Server};

fn image_bytes() -> Vec<u8> {
    let source = ImageBuffer::from_pixel(4, 2, Rgba([255u8, 0, 0, 128]));
    let mut bytes = Cursor::new(Vec::new());
    source.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn id(frame: &Frame, value: &str) -> NodeId {
    frame
        .document
        .preorder()
        .find(|id| {
            frame
                .document
                .node(*id)
                .and_then(|node| node.as_element())
                .is_some_and(|element| element.attribute("id") == Some(value))
        })
        .unwrap()
}

fn layout_box(root: &LayoutBox, id: NodeId) -> &LayoutBox {
    if root.dom_node_id() == id {
        return root;
    }
    root.children()
        .iter()
        .find_map(|child| {
            fn find(root: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
                if root.dom_node_id() == id {
                    return Some(root);
                }
                root.children().iter().find_map(|child| find(child, id))
            }
            find(child, id)
        })
        .unwrap()
}

fn font() -> fontdue::Font {
    fontdue::Font::from_bytes(
        include_bytes!("../assets/fonts/noto-sans/NotoSans.ttf").as_slice(),
        fontdue::FontSettings::default(),
    )
    .unwrap()
}

#[test]
fn redirected_document_resolves_base_and_cascades_external_sheets_in_document_order() {
    let server = Server::new(
        vec![
            ("/start", Reply::redirect("/docs/page")),
            (
                "/docs/page",
                Reply::ok(
                    "text/html",
                    b"<base href='../assets/'>\
            <style>p { color: red }</style><link rel='StyleSheet' href='first.css'>\
            <style>p { color: green }</style><link rel='stylesheet' href='last.css'>\
            <link rel='stylesheet' href='first.css'><p id='target'>styled</p>"
                        .to_vec(),
                ),
            ),
            (
                "/assets/first.css",
                Reply::ok("text/css; charset=utf-8", b"p { color: blue }".to_vec()),
            ),
            (
                "/assets/last.css",
                Reply::ok("text/css", b"p { color: gold }".to_vec()),
            ),
        ],
        4,
    );
    let mut colors = Vec::new();
    let frame = load_page(
        &server.url.join("start#part").unwrap(),
        LoadOptions::default(),
        |frame| {
            if frame.stylesheets.stylesheets.len() >= 2 {
                let result =
                    style::resolve_with_stylesheets(&frame.document, frame.stylesheets.clone());
                colors.push(result.styles.get(id(frame, "target")).unwrap().color);
            }
            Ok(())
        },
        || false,
    )
    .unwrap();
    assert_eq!(frame.url, server.url.join("docs/page#part").unwrap());
    assert_eq!(
        document_base_url(&frame.document, &frame.url),
        server.url.join("assets/").unwrap()
    );
    assert!(frame.stylesheets.diagnostics.is_empty());
    assert_eq!(frame.stylesheets.stylesheets.len(), 5);
    assert!(
        colors.contains(&Some(0x008000)),
        "embedded CSS applies before external loads"
    );
    assert!(
        colors.contains(&Some(0xFFD700)),
        "last external sheet applies at its document slot"
    );
    assert_eq!(
        colors.last(),
        Some(&Some(0x0000FF)),
        "duplicate sheet applies again at its later slot"
    );
    assert_eq!(
        server.finish(),
        [
            "/start",
            "/docs/page",
            "/assets/first.css",
            "/assets/last.css"
        ]
    );
}

#[test]
fn redirected_url_without_base_is_used_for_relative_resources() {
    let server = Server::new(
        vec![
            ("/start", Reply::redirect("/nested/page")),
            (
                "/nested/page",
                Reply::ok(
                    "text/html",
                    b"<link rel='stylesheet' href='style.css'><p>x</p>".to_vec(),
                ),
            ),
            (
                "/nested/style.css",
                Reply::ok("text/css", b"p { color: red }".to_vec()),
            ),
        ],
        3,
    );
    let frame = load_page(
        &server.url.join("start").unwrap(),
        LoadOptions::default(),
        |_| Ok(()),
        || false,
    )
    .unwrap();
    assert_eq!(
        frame.stylesheets.stylesheets[0].url,
        Some(server.url.join("nested/style.css").unwrap())
    );
    assert_eq!(
        server.finish(),
        ["/start", "/nested/page", "/nested/style.css"]
    );
}

#[test]
fn redirect_fragment_overrides_the_requested_fragment() {
    let server = Server::new(
        vec![
            ("/start", Reply::redirect("/page#new")),
            ("/page", Reply::ok("text/html", b"<p>target</p>".to_vec())),
        ],
        2,
    );
    let frame = load_page(
        &server.url.join("start#old").unwrap(),
        LoadOptions::default(),
        |_| Ok(()),
        || false,
    )
    .unwrap();
    assert_eq!(frame.url, server.url.join("page#new").unwrap());
    assert_eq!(server.finish(), ["/start", "/page"]);
}

#[test]
fn all_get_redirect_statuses_honor_an_explicitly_empty_fragment() {
    for status in [301, 302, 303, 307, 308] {
        let mut reply = Reply::redirect("/page#");
        reply.status = status;
        let server = Server::new(
            vec![
                ("/start", reply),
                ("/page", Reply::ok("text/html", b"<p>target</p>".to_vec())),
            ],
            2,
        );
        let frame = load_page(
            &server.url.join("start#old").unwrap(),
            LoadOptions::default(),
            |_| Ok(()),
            || false,
        )
        .unwrap();
        assert_eq!(frame.url.fragment(), Some(""), "status {status}");
        assert_eq!(server.finish(), ["/start", "/page"]);
    }
}

#[test]
fn image_dimensions_intrinsic_ratio_block_inline_and_link_hit_testing_are_wired() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", b"<a href='/next'><img id='natural' src='pixel'></a>\
            <img id='width' src='pixel' width='20'><img id='height' src='pixel' height='12'>\
            <img id='both' src='pixel' width='10' height='15'>\
            <img id='css' src='pixel' width='10' style='width:30px; display:block; padding:2px; border-width:1px'>\
            <img id='zero' src='pixel' width='0'>\
            <img id='auto' src='pixel' width='20' style='width:auto'>".to_vec())),
        ("/pixel", Reply::ok("image/png", image_bytes())),
    ], 2);
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(frame.stylesheets.diagnostics.is_empty());
    assert_eq!(frame.images.len(), 7);
    let data = frame.images.get(&id(&frame, "natural")).unwrap();
    assert!(Arc::ptr_eq(
        data,
        frame.images.get(&id(&frame, "width")).unwrap()
    ));
    let mut tree = frame.layout_tree();
    tree.layout(400.0, &font());
    for (name, size) in [
        ("natural", (4.0, 2.0)),
        ("width", (20.0, 10.0)),
        ("height", (24.0, 12.0)),
        ("both", (10.0, 15.0)),
        ("css", (30.0, 15.0)),
        ("zero", (0.0, 0.0)),
        ("auto", (4.0, 2.0)),
    ] {
        let rect = layout_box(tree.root(), id(&frame, name)).content_rect();
        assert_eq!((rect.width, rect.height), size, "{name}");
    }
    let css = layout_box(tree.root(), id(&frame, "css")).rect();
    assert_eq!((css.width, css.height), (36.0, 21.0));
    let rect = tree.node_rect(id(&frame, "natural")).unwrap();
    let hit = tree.hit_test(rect.x + 1.0, rect.y + 1.0).unwrap();
    assert_eq!(
        link_url(&frame.document, hit, &frame.url).unwrap(),
        Some(server.url.join("next").unwrap())
    );
    assert_eq!(server.finish(), ["/", "/pixel"]);
}

#[test]
fn failed_css_images_and_wrong_mime_are_diagnostic_without_failing_the_page() {
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", b"<link rel='stylesheet' href='missing.css'>\
            <link rel='stylesheet' href='wrong.css'><link rel='stylesheet' href='bad.css'>\
            <link rel='alternate stylesheet' href='skip'><link rel='stylesheet' disabled href='skip'>\
            <link rel='stylesheet' media='print' href='skip'>\
            <img src='broken' alt='Image unavailable'><p>still visible</p>".to_vec())),
        ("/missing.css", Reply { status: 404, headers: Vec::new(), body: Vec::new() }),
        ("/wrong.css", Reply::ok("text/html", b"p { color: red }".to_vec())),
        ("/bad.css", Reply::ok("text/css", b"p {".to_vec())),
        ("/broken", Reply::ok("image/png", b"broken".to_vec())),
    ], 5);
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(frame.error.is_none());
    assert!(frame.images.is_empty());
    assert!(frame.stylesheets.stylesheets.is_empty());
    let messages = frame
        .stylesheets
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 5);
    assert!(messages.iter().any(|message| message.contains("404")));
    assert!(
        messages
            .iter()
            .any(|message| message.contains("not text/css"))
    );
    let mut tree = frame.layout_tree();
    tree.layout(400.0, &font());
    assert!(tree.root().rect().height > 0.0);
    assert_eq!(
        server.finish(),
        ["/", "/missing.css", "/wrong.css", "/bad.css", "/broken"]
    );
}

fn finish_navigation(navigation: &mut Navigation) -> Arc<Frame> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut latest = None;
    loop {
        assert!(Instant::now() < deadline, "page load did not finish");
        match navigation.try_recv() {
            Ok(LoadEvent::Progress(frame)) => latest = Some(frame),
            Ok(LoadEvent::Finished) => return latest.unwrap(),
            Err(std::sync::mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(1)),
            Err(error) => panic!("page channel disconnected: {error}"),
        }
    }
}

#[test]
fn failed_navigation_produces_safe_renderable_error_page_with_retry_link() {
    let server = Server::new(
        vec![(
            "/",
            Reply {
                status: 404,
                headers: Vec::new(),
                body: Vec::new(),
            },
        )],
        1,
    );
    let mut navigation = Navigation::new(server.url.clone(), LoadOptions::default());
    navigation.start_load().unwrap();
    let frame = finish_navigation(&mut navigation);
    assert!(frame.error.as_deref().unwrap().contains("404"));
    let mut tree = frame.layout_tree();
    tree.layout(400.0, &font());
    assert!(tree.root().rect().height > 0.0);
    let retry = frame.document.preorder().last().unwrap();
    assert_eq!(
        link_url(&frame.document, retry, &frame.url).unwrap(),
        Some(server.url.clone())
    );
    assert_eq!(server.finish(), ["/"]);
}

#[test]
fn redirected_error_pages_keep_the_final_url_for_title_history_and_retry() {
    let server = Server::new(
        vec![
            ("/start", Reply::redirect("/missing")),
            (
                "/missing",
                Reply {
                    status: 404,
                    headers: Vec::new(),
                    body: Vec::new(),
                },
            ),
        ],
        2,
    );
    let mut navigation = Navigation::new(server.url.join("start").unwrap(), LoadOptions::default());
    navigation.start_load().unwrap();
    let frame = finish_navigation(&mut navigation);
    assert_eq!(frame.url, server.url.join("missing").unwrap());
    assert_eq!(navigation.history.current().url, frame.url);
    assert!(frame.error.as_deref().unwrap().contains("404"));
    assert_eq!(server.finish(), ["/start", "/missing"]);
}

#[test]
fn cancelled_load_stops_progress_and_replaced_navigation_has_no_stale_events() {
    let first = net::location_url(&format!(
        "{}/tests/pages/basic.html",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let cancelled = AtomicBool::new(false);
    let mut progress = 0;
    let result = load_page(
        &first,
        LoadOptions {
            trace: false,
            throttle_ms: Some(0),
        },
        |_| {
            progress += 1;
            cancelled.store(true, Ordering::Relaxed);
            Ok(())
        },
        || cancelled.load(Ordering::Relaxed),
    );
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert_eq!(progress, 1);
    let second = net::location_url(&format!(
        "{}/tests/pages/styles.html",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut navigation = Navigation::new(first, LoadOptions::default());
    navigation.start_load().unwrap();
    navigation.history.push(second.clone());
    navigation.start_load().unwrap();
    let frame = finish_navigation(&mut navigation);
    assert_eq!(frame.url, second);
    assert!(frame.error.is_none());
}

#[test]
fn redirect_loop_is_bounded_and_http_errors_fail_explicitly() {
    let server = Server::new(vec![("/", Reply::redirect("/"))], 11);
    let error = net::fetch_html(server.url.as_str()).unwrap_err();
    assert!(error.to_string().to_lowercase().contains("redirect"));
    assert_eq!(server.finish().len(), 11);
    let server = Server::new(
        vec![(
            "/",
            Reply {
                status: 503,
                headers: Vec::new(),
                body: Vec::new(),
            },
        )],
        1,
    );
    assert!(
        net::fetch_html(server.url.as_str())
            .unwrap_err()
            .to_string()
            .contains("503")
    );
    assert_eq!(server.finish(), ["/"]);
}

#[test]
fn resource_byte_limits_and_truncated_http_bodies_fail_instead_of_succeeding_partially() {
    let server = Server::new(vec![("/", Reply::ok("text/plain", b"abcd".to_vec()))], 1);
    assert!(
        net::fetch_resource(&server.url, 3)
            .err()
            .unwrap()
            .to_string()
            .contains("byte limit")
    );
    assert_eq!(server.finish(), ["/"]);
    let server = Server::new(
        vec![(
            "/",
            Reply {
                status: 200,
                headers: vec![("Content-Length", "100".into())],
                body: b"<p>truncated".to_vec(),
            },
        )],
        1,
    );
    assert!(net::fetch_html(server.url.as_str()).is_err());
    assert_eq!(server.finish(), ["/"]);
}

#[test]
fn non_http_redirects_and_missing_locations_are_rejected() {
    for reply in [
        Reply::redirect("file:///private/document"),
        Reply::redirect("javascript:alert(1)"),
        Reply {
            status: 302,
            headers: Vec::new(),
            body: Vec::new(),
        },
    ] {
        let server = Server::new(vec![("/", reply)], 1);
        assert!(
            net::fetch_html(server.url.as_str())
                .unwrap_err()
                .to_string()
                .contains("redirect")
        );
        assert_eq!(server.finish(), ["/"]);
    }
}

#[test]
fn local_files_load_external_css_and_cli_dumps_use_it() {
    let location = format!(
        "{}/tests/pages/navigation/index.html",
        env!("CARGO_MANIFEST_DIR")
    );
    let url = net::location_url(&location).unwrap();
    let frame = load_page(&url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(frame.stylesheets.diagnostics.is_empty());
    assert_eq!(frame.stylesheets.stylesheets.len(), 1);
    assert_eq!(
        frame.stylesheets.stylesheets[0]
            .url
            .as_ref()
            .unwrap()
            .scheme(),
        "file"
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rust-browser"))
        .args([&location, "--dump=display-list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("color: 1458779"),
        "external heading #16425b reaches paint"
    );
}

#[test]
fn page_resource_count_is_bounded_even_for_repeated_cached_urls() {
    let source = format!(
        "{}<p>content survives</p>",
        "<link rel='stylesheet' href='/empty.css'>".repeat(129)
    );
    let server = Server::new(
        vec![
            ("/", Reply::ok("text/html", source.into_bytes())),
            ("/empty.css", Reply::ok("text/css", Vec::new())),
        ],
        2,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(frame.stylesheets.stylesheets.len(), 128);
    assert_eq!(frame.stylesheets.diagnostics.len(), 1);
    assert!(
        frame.stylesheets.diagnostics[0]
            .message
            .contains("count exceeds 128")
    );
    assert_eq!(server.finish(), ["/", "/empty.css"]);
}
