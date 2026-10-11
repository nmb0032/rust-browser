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
    if root.dom_node_id() == Some(id) {
        return root;
    }
    root.children()
        .iter()
        .find_map(|child| {
            fn find(root: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
                if root.dom_node_id() == Some(id) {
                    return Some(root);
                }
                root.children().iter().find_map(|child| find(child, id))
            }
            find(child, id)
        })
        .unwrap()
}

fn font() -> rust_browser::text::FontContext {
    rust_browser::text::FontContext::bundled()
}

#[test]
fn redirected_cached_variable_tokens_restyle_through_media_inline_importance_and_user_origins() {
    use rust_browser::css::{
        media::MediaEnvironment,
        values::{BorderStyle, Length},
    };

    let server = Server::new(vec![
        ("/", Reply::ok("text/html", br#"<!doctype html>
            <style>main {--theme:red; --size:20px; --font:italic 20px/2 serif}
            #target {color:var(--theme); width:var(--size); font:var(--font); border:var(--border,solid)}</style>
            <link rel='stylesheet' href='theme.css' media='(min-width:600px)'>
            <link rel='stylesheet' href='theme.css' media='(min-width:600px)'>
            <main><p id='target' style='--border:5px dashed purple !important;
            border-left-color:var(--theme) !important'>theme</p></main>"#.to_vec())),
        ("/theme.css", Reply::redirect("/final-theme.css")),
        ("/final-theme.css", Reply::ok("text/css", br#"main {
            --theme:rgb(1, 2, 3); --size:40px; --font:oblique 650 24px/1.25 "Noto Sans", serif}
            #target {--border:1px solid blue !important}"#.to_vec())),
    ], 3);
    let mut frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(frame.stylesheets.diagnostics.is_empty());
    assert_eq!(frame.stylesheets.stylesheets.len(), 3);
    assert!(Arc::ptr_eq(
        &frame.stylesheets.stylesheets[1].stylesheet,
        &frame.stylesheets.stylesheets[2].stylesheet
    ));
    assert!(
        frame.stylesheets.stylesheets[1]
            .url
            .as_ref()
            .unwrap()
            .path()
            .ends_with("final-theme.css")
    );
    let node = id(&frame, "target");
    for (width, color, size, font_size) in [
        (400.0, 0xff0000, 20.0, 20.0),
        (600.0, 0x010203, 40.0, 24.0),
        (400.0, 0xff0000, 20.0, 20.0),
    ] {
        frame.set_media_environment(MediaEnvironment::new(width, 600.0));
        let style = frame.styles.get(node).unwrap();
        assert_eq!(style.color, Some(color));
        assert_eq!(style.width, Some(Length::Px(size)));
        assert_eq!(style.font_size, font_size);
        assert_eq!(style.border_width.top, 5.0);
        assert_eq!(style.border_style.top, BorderStyle::Dashed);
        assert_eq!(style.border_color.top, Some(0x800080));
        assert_eq!(style.border_color.left, Some(color));
        assert!(frame.stylesheets.diagnostics.is_empty());
    }
    let resolution = style::resolve_with_user_stylesheets_and_environment(
        &frame.document,
        frame.stylesheets.clone(),
        &["p {--theme:green !important; --border:3px double blue !important}"],
        MediaEnvironment::new(600.0, 600.0),
    );
    let target = resolution.styles.get(node).unwrap();
    assert_eq!(target.color, Some(0x008000));
    assert_eq!(target.border_width.top, 3.0);
    assert_eq!(target.border_style.top, BorderStyle::Double);
    assert_eq!(server.finish(), ["/", "/theme.css", "/final-theme.css"]);
}

#[test]
fn progressive_loading_publishes_changes_to_an_existing_text_node() {
    let source = format!("<p>{}</p>", "x".repeat(1024));
    let server = Server::new(vec![("/", Reply::ok("text/html", source.into_bytes()))], 1);
    let mut updates = Vec::new();
    load_page(
        &server.url,
        LoadOptions {
            trace: false,
            throttle_ms: Some(0),
        },
        |frame| {
            let text_bytes: usize = frame
                .document
                .preorder()
                .filter_map(|id| match frame.document.node(id).unwrap().kind() {
                    rust_browser::dom::NodeKind::Text(text) => Some(text.len()),
                    _ => None,
                })
                .sum();
            updates.push((frame.document.node_count(), text_bytes));
            Ok(())
        },
        || false,
    )
    .unwrap();
    assert!(
        updates
            .windows(2)
            .any(|pair| { pair[0].0 == pair[1].0 && pair[0].1 < pair[1].1 }),
        "text-only progress must not be lost: {updates:?}"
    );
    assert_eq!(updates.last().unwrap().1, 1024);
    server.finish();
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
fn external_embedded_inline_and_user_origins_cascade_with_wide_keywords() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<style>#normal {color:red} #important {color:red !important}</style>\
                    <link rel='stylesheet' href='cascade.css'>\
                    <style>#order {color:purple !important}</style>\
                    <main style='color:navy; font-size:20px; padding:2em; width:50%'>\
                    <p id='normal'>normal</p>\
                    <p id='important' style='color:purple'>important</p>\
                    <p id='inline' style='color:purple !important'>inline</p>\
                    <p id='order'>order</p><h1 id='keywords'>keywords</h1>\
                    <span id='user'>user</span>\
                    <p id='user-important' style='color:purple !important; padding:8px !important'>\
                    user important</p></main>"
                        .to_vec(),
                ),
            ),
            ("/cascade.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    b"#normal {color:blue} #important {color:blue !important}\
                    #inline {color:blue !important} #order {color:blue !important}\
                    #keywords {font-size:initial; font-weight:initial; display:unset;\
                    margin:unset; padding:inherit; width:inherit; color:inherit}\
                    #user-important {color:blue !important; padding:9px !important}"
                        .to_vec(),
                ),
            ),
        ],
        3,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    let result = style::resolve_with_user_stylesheets(
        &frame.document,
        frame.stylesheets.clone(),
        &[
            "#normal#normal {color:green} #user {color:green}\
             p {color:red !important}",
            "#user-important {color:inherit !important; padding:initial !important}",
        ],
    );
    assert!(result.stylesheets.diagnostics.is_empty());
    let get = |name| result.styles.get(id(&frame, name)).unwrap();
    // Normal user specificity cannot beat author origin; important user rules
    // beat both important external rules and important inline declarations.
    for name in ["normal", "important", "inline", "order"] {
        assert_eq!(get(name).color, Some(0xFF0000), "{name}");
    }

    assert_eq!(get("user").color, Some(0x008000));
    assert_eq!(get("user-important").color, Some(0x000080));
    assert_eq!(
        get("user-important").padding,
        rust_browser::css::values::Edges::default()
    );
    let keywords = get("keywords");
    assert_eq!(keywords.font_size, 16.0);
    assert!(!keywords.bold);
    assert_eq!(keywords.display, style::Display::Inline);
    assert_eq!(keywords.color, Some(0x000080));
    assert_eq!(keywords.margin, rust_browser::css::values::Edges::default());
    assert_eq!(
        keywords.padding,
        rust_browser::css::values::Edges::all(rust_browser::css::values::Length::Px(40.0))
    );
    assert_eq!(
        keywords.width,
        Some(rust_browser::css::values::Length::Percent(50.0))
    );
    let author = style::resolve_with_stylesheets(&frame.document, frame.stylesheets.clone());
    let get = |name| author.styles.get(id(&frame, name)).unwrap();
    assert_eq!(get("normal").color, Some(0x0000FF));
    assert_eq!(get("important").color, Some(0x0000FF));
    assert_eq!(get("inline").color, Some(0x800080));
    assert_eq!(get("order").color, Some(0x800080));
    assert_eq!(server.finish(), ["/", "/cascade.css", "/final.css"]);
}

#[test]
fn media_attributes_and_nested_redirected_cached_sheets_restyle_without_refetch() {
    use rust_browser::css::{
        Rule,
        media::{MediaEnvironment, MediaType},
    };
    let server = Server::new(vec![
        ("/", Reply::ok("text/html", b"<!doctype html>\
        <style>html {font-size:100px} p {color:red; width:10px}</style>\
        <link id='narrow' rel='stylesheet' href='sheet.css' media='(max-width:37.5em)'>\
        <style media='screen and (min-width:600px)'>p {color:green; width:200px}</style>\
        <link id='wide' rel='stylesheet' href='sheet.css' media='only screen and (min-width:37.5rem)'>\
        <link id='print' rel='stylesheet' href='sheet.css' media='print'>\
        <style media='only (width:600px)'>p {color:yellow}</style>\
        <p id='target'>responsive</p><img id='image' src='pixel'>".to_vec())),
        ("/sheet.css", Reply::redirect("/final.css")),
        ("/final.css", Reply::ok("text/css", b"p {color:blue; width:300px}\
        @media (max-width:600px) {p {color:purple; width:100px}}\
        @media not (future-height:600px) {p {color:yellow}}".to_vec())),
        ("/pixel", Reply::ok("image/png", image_bytes())),
    ], 4);
    let mut frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(server.finish(), ["/", "/sheet.css", "/final.css", "/pixel"]);
    assert_eq!(frame.stylesheets.stylesheets.len(), 6);
    let narrow = id(&frame, "narrow");
    let wide = id(&frame, "wide");
    let print = id(&frame, "print");
    let target = id(&frame, "target");
    let image_id = id(&frame, "image");
    let sheets = &frame.stylesheets.stylesheets;
    let sheet = Arc::clone(&sheets[1].stylesheet);
    assert!(Arc::ptr_eq(&sheet, &sheets[3].stylesheet));
    assert!(Arc::ptr_eq(&sheet, &sheets[4].stylesheet));
    assert!(
        sheets[1]
            .url
            .as_ref()
            .unwrap()
            .path()
            .ends_with("/final.css")
    );
    assert!(matches!(sheet.rules()[1], Rule::At(_)));
    assert!(Arc::ptr_eq(
        &frame.external_stylesheet_sources[&narrow].source,
        &frame.external_stylesheet_sources[&wide].source
    ));
    assert!(Arc::ptr_eq(
        &frame.external_stylesheet_sources[&narrow].source,
        &frame.external_stylesheet_sources[&print].source
    ));
    let image = Arc::clone(&frame.images[&image_id]);
    let diagnostics = frame.stylesheets.diagnostics.len();
    assert_eq!(diagnostics, 4);
    let font = font();
    for (width, color, expected_width) in [
        (400.0, 0x800080, 100.0),
        (600.0, 0x800080, 100.0),
        (800.0, 0x0000FF, 300.0),
        (400.0, 0x800080, 100.0),
    ] {
        frame.set_media_environment(MediaEnvironment::new(width, 600.0));
        assert_eq!(frame.styles.get(target).unwrap().color, Some(color));
        let mut tree = frame.layout_tree();
        tree.layout(width, &font);
        assert_eq!(tree.node_rect(target).unwrap().width, expected_width);
        assert_eq!(frame.stylesheets.diagnostics.len(), diagnostics);
        assert!(Arc::ptr_eq(
            &sheet,
            &frame.stylesheets.stylesheets[1].stylesheet
        ));
        assert!(Arc::ptr_eq(&image, &frame.images[&image_id]));
        assert_eq!(frame.external_stylesheet_sources.len(), 3);
    }
    frame.set_media_environment(MediaEnvironment {
        media_type: MediaType::Print,
        ..MediaEnvironment::new(800.0, 600.0)
    });
    assert_eq!(frame.styles.get(target).unwrap().color, Some(0x0000FF));
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
fn malformed_external_css_is_recovered_cached_and_applied_at_each_document_slot() {
    let server = Server::new(
        vec![
            (
                "/",
                Reply::ok(
                    "text/html",
                    b"<style>p {color:red; broken; margin:1px}</style>\
                    <link rel='stylesheet' href='recover.css'>\
                    <style>p {padding:8px}</style>\
                    <link rel='stylesheet' href='recover.css'>\
                    <p id='target' style='color:green; broken; width:4px'>text</p>\
                    <h1 id='heading'>heading</h1>"
                        .to_vec(),
                ),
            ),
            ("/recover.css", Reply::redirect("/final.css")),
            (
                "/final.css",
                Reply::ok(
                    "text/css",
                    b"p {color:blue; broken; width:2px} h1 {color:gold} p {padding:3px".to_vec(),
                ),
            ),
        ],
        3,
    );
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert_eq!(frame.stylesheets.stylesheets.len(), 4);
    assert_eq!(frame.stylesheets.diagnostics.len(), 6);
    let first = &frame.stylesheets.stylesheets[1];
    let last = &frame.stylesheets.stylesheets[3];
    assert!(Arc::ptr_eq(&first.stylesheet, &last.stylesheet));
    assert_eq!(first.url, Some(server.url.join("final.css").unwrap()));
    assert_eq!(last.url, first.url);
    assert!(Arc::ptr_eq(
        &frame.external_stylesheet_sources[&first.node_id].source,
        &frame.external_stylesheet_sources[&last.node_id].source,
    ));
    for sheet in [first, last] {
        let errors = frame
            .stylesheets
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.node_id == Some(sheet.node_id))
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|diagnostic| {
            diagnostic.message.contains("recover.css") && diagnostic.message.contains("final.css")
        }));
    }
    let target = frame.styles.get(id(&frame, "target")).unwrap();
    assert_eq!(target.color, Some(0x008000));
    assert_eq!(
        target.width,
        Some(rust_browser::css::values::Length::Px(4.0))
    );
    assert_eq!(
        target.margin,
        rust_browser::css::values::Edges::all(rust_browser::css::values::Length::Px(1.0).into())
    );
    assert_eq!(
        target.padding,
        rust_browser::css::values::Edges::all(rust_browser::css::values::Length::Px(3.0))
    );
    assert_eq!(
        frame.styles.get(id(&frame, "heading")).unwrap().color,
        Some(0xFFD700)
    );
    assert_eq!(server.finish(), ["/", "/recover.css", "/final.css"]);
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
            <img id='css' src='pixel' width='10' style='width:30px; display:block; padding:2px; border-width:1px; border-style:solid'>\
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
            <link rel='stylesheet' media='print' href='print.css'>\
            <img src='broken' alt='Image unavailable'><p>still visible</p>".to_vec())),
        ("/missing.css", Reply { status: 404, headers: Vec::new(), body: Vec::new() }),
        ("/wrong.css", Reply::ok("text/html", b"p { color: red }".to_vec())),
        ("/bad.css", Reply::ok("text/css", b"p {".to_vec())),
        ("/print.css", Reply::ok("text/css", b"p {color:red}".to_vec())),
        ("/broken", Reply::ok("image/png", b"broken".to_vec())),
    ], 6);
    let frame = load_page(&server.url, LoadOptions::default(), |_| Ok(()), || false).unwrap();
    assert!(frame.error.is_none());
    assert!(frame.images.is_empty());
    assert_eq!(frame.stylesheets.stylesheets.len(), 2);
    let rules = frame.stylesheets.stylesheets[0].stylesheet.rules();
    assert!(
        matches!(rules, [rust_browser::css::Rule::Qualified(rule)] if rule.prelude() == "p" && rule.items().is_empty())
    );
    let messages = frame
        .stylesheets
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 4);
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
        [
            "/",
            "/missing.css",
            "/wrong.css",
            "/bad.css",
            "/print.css",
            "/broken"
        ]
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
