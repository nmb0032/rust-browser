use super::*;
use crate::{
    css::media::MediaEnvironment,
    dom::{Document, NodeId},
    engine::Frame,
    html::parser,
    paint,
    style::{self, Overflow},
    text::FontContext,
};

fn id(doc: &Document, name: &str) -> NodeId {
    doc.preorder()
        .find(|&id| {
            doc.node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

fn page(css: &str, html: &str) -> (Document, LayoutTree) {
    let doc = parser::parse(&format!(
        "<!doctype html><style>html,body{{margin:0}}body{{font:16px/20px sans-serif}}{css}</style>{html}"
    )).unwrap();
    let result = style::resolve(&doc);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&doc, &result.styles);
    tree.layout_viewport(300.0, 200.0, &FontContext::bundled());
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (doc, tree)
}

fn pixels(tree: &LayoutTree, scale: f32, outlines: bool) -> Vec<u32> {
    let width = (300.0 * scale) as usize;
    let height = (200.0 * scale) as usize;
    let mut pixels = vec![0; width * height];
    paint::rasterize_display_list(
        &paint::build_display_list(tree, 300.0, 200.0, outlines),
        &FontContext::bundled(),
        &mut pixels,
        width,
        height,
        scale as f64,
        tree.viewport_scroll().y,
    );
    pixels
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.005, "{a} != {b}");
}

#[test]
fn overflow_shorthand_axes_couple_after_cascade_and_reject_non_keywords_atomically() {
    for (source, expected) in [
        ("overflow:visible", (Overflow::Visible, Overflow::Visible)),
        ("overflow:clip visible", (Overflow::Clip, Overflow::Visible)),
        (
            "overflow:visible hidden",
            (Overflow::Auto, Overflow::Hidden),
        ),
        ("overflow:clip scroll", (Overflow::Hidden, Overflow::Scroll)),
        (
            "overflow-x:auto;overflow-y:visible",
            (Overflow::Auto, Overflow::Auto),
        ),
        (
            "--o:clip auto;overflow:var(--o)",
            (Overflow::Hidden, Overflow::Auto),
        ),
        (
            "overflow:hidden;overflow:initial",
            (Overflow::Visible, Overflow::Visible),
        ),
        (
            "overflow:hidden;overflow-y:visible",
            (Overflow::Hidden, Overflow::Auto),
        ),
    ] {
        let (doc, tree) = page(&format!("#panel{{{source}}}"), "<div id=panel></div>");
        let panel = id(&doc, "panel");
        let mut found = None;
        fn walk<'a>(b: &'a LayoutBox, node: NodeId, found: &mut Option<&'a LayoutBox>) {
            if b.dom_node_id() == Some(node) {
                *found = Some(b);
            }
            for child in b.children() {
                walk(child, node, found);
            }
        }
        walk(tree.root(), panel, &mut found);
        let style = found.unwrap().style();
        assert_eq!((style.overflow_x, style.overflow_y), expected, "{source}");
    }
    let doc = parser::parse(
        "<div id=p style='overflow:hidden;overflow:scroll 20%;overflow:clip auto hidden'></div>",
    )
    .unwrap();
    let result = style::resolve(&doc);
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
    let style = result.styles.get(id(&doc, "p")).unwrap();
    assert_eq!(
        (style.overflow_x, style.overflow_y),
        (Overflow::Hidden, Overflow::Hidden)
    );
}

#[test]
fn padding_edge_clip_keeps_border_and_hidden_content_out_of_root_range() {
    let (doc, mut tree) = page(
        "#panel{width:100px;height:50px;padding:5px;border:4px solid blue;overflow:hidden}\
         #child{height:500px;width:200px;background:red}",
        "<div id=panel><div id=child></div></div>",
    );
    let panel = id(&doc, "panel");
    let child = id(&doc, "child");
    let container = tree.scroll_container(panel).unwrap();
    assert_eq!(
        container.port,
        Rect {
            x: 28.0,
            y: 28.0,
            width: 110.0,
            height: 60.0
        }
    );
    close(container.range.max.x, 100.0);
    close(container.range.max.y, 450.0);
    assert_eq!(tree.viewport_scroll_container().range.max.y, 0.0);
    assert_eq!(tree.node_rect(panel).unwrap().height, 68.0);
    assert_eq!(tree.node_rect(child).unwrap().height, 500.0);
    assert_eq!(tree.visual_node_rect(child, 0.0).unwrap().height, 55.0);
    let p = pixels(&tree, 1.0, false);
    assert_eq!(p[40 * 300 + 25], 0x0000ff);
    assert_eq!(p[40 * 300 + 135], 0xff0000);
    assert_eq!(p[40 * 300 + 139], 0x0000ff);
    assert_eq!(p[100 * 300 + 35], 0xffffff);
    assert_ne!(tree.hit_test(35.0, 100.0), Some(child));
    assert_eq!(tree.hit_test(35.0, 40.0), Some(child));
    assert!(tree.set_scroll_offset(panel, ScrollOffset { x: 50.25, y: 120.5 }));
    assert_eq!(tree.scroll_offset(panel).y, 120.5);
    assert!(!tree.scroll_wheel(50.0, 50.0, ScrollOffset { x: 10.0, y: 20.0 }));
    assert_eq!(tree.scroll_offset(panel).y, 120.5);
}

#[test]
fn clip_is_axis_specific_non_scrollable_and_does_not_create_a_bfc() {
    let (doc, mut tree) = page(
        "#panel{width:80px;height:40px;overflow:clip visible} #child{width:140px;height:100px;background:red}",
        "<div id=panel><div id=child></div></div>",
    );
    let panel = id(&doc, "panel");
    let child = id(&doc, "child");
    assert_eq!(
        tree.scroll_container(panel).unwrap().range,
        ScrollRange::default()
    );
    assert!(!tree.set_scroll_offset(panel, ScrollOffset { x: 30.0, y: 30.0 }));
    assert_eq!(tree.hit_test(30.0, 100.0), Some(child));
    assert_ne!(tree.hit_test(120.0, 50.0), Some(child));
    let p = pixels(&tree, 1.0, false);
    assert_eq!(p[100 * 300 + 30], 0xff0000);
    assert_eq!(p[50 * 300 + 120], 0xffffff);
    for (mode, child_offset, parent_height) in [
        ("visible", 0.0, 10.0),
        ("clip", 0.0, 10.0),
        ("hidden", 30.0, 60.0),
        ("scroll", 30.0, 60.0),
        ("auto", 30.0, 60.0),
    ] {
        let (doc, tree) = page(
            &format!("#p{{overflow:{mode}}}#c{{height:10px;margin-top:30px;margin-bottom:20px}}"),
            "<div id=p><div id=c></div></div>",
        );
        let p = tree.node_rect(id(&doc, "p")).unwrap();
        let c = tree.node_rect(id(&doc, "c")).unwrap();
        close(c.y - p.y, child_offset);
        close(p.height, parent_height);
    }
}

#[test]
fn nested_wheels_clamp_and_chain_unconsumed_axes_and_direction() {
    let (doc, mut tree) = page(
        "#outer{width:80px;height:100px;overflow:auto}\
         #inner{width:100px;height:50px;overflow:scroll}\
         #content{width:220px;height:180px;background:red}\
         #tail{height:200px}#root-tail{height:500px}",
        "<div id=outer><div id=inner><div id=content></div></div><div id=tail></div></div><div id=root-tail></div>",
    );
    let outer = id(&doc, "outer");
    let inner = id(&doc, "inner");
    close(tree.scroll_container(inner).unwrap().range.max.y, 130.0);
    close(tree.scroll_container(inner).unwrap().range.max.x, 120.0);
    close(tree.scroll_container(outer).unwrap().range.max.y, 150.0);
    close(tree.scroll_container(outer).unwrap().range.max.x, 20.0);
    assert!(tree.scroll_wheel(40.0, 40.0, ScrollOffset { x: 50.5, y: 40.25 }));
    assert_eq!(
        tree.scroll_offset(inner),
        ScrollOffset { x: 50.5, y: 40.25 }
    );
    assert_eq!(tree.scroll_offset(outer), ScrollOffset::default());
    assert!(tree.scroll_wheel(40.0, 40.0, ScrollOffset { x: 100.0, y: 220.0 }));
    assert_eq!(
        tree.scroll_offset(inner),
        ScrollOffset { x: 120.0, y: 130.0 }
    );
    close(tree.scroll_offset(outer).y, 130.25);
    close(tree.scroll_offset(outer).x, 20.0);
    assert_eq!(tree.viewport_scroll().x, 0.0);
    tree.set_scroll_offset(outer, ScrollOffset::default());
    tree.set_viewport_scroll(ScrollOffset::default());
    assert!(tree.scroll_wheel(
        40.0,
        40.0,
        ScrollOffset {
            x: -500.0,
            y: -500.0
        }
    ));
    assert_eq!(tree.scroll_offset(inner), ScrollOffset::default());
    assert_eq!(tree.scroll_offset(outer), ScrollOffset::default());
    // A single event can overshoot both panels and reach root.
    assert!(tree.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 400.0 }));
    close(tree.scroll_offset(inner).y, 130.0);
    close(tree.scroll_offset(outer).y, 150.0);
    close(tree.viewport_scroll().y, 120.0);
}

#[test]
fn fixed_escapes_ordinary_overflow_absolute_follows_containing_block_not_dom_parent() {
    let (doc, mut tree) = page(
        "#cb{position:relative}#clip{overflow:hidden;width:60px;height:40px}\
         #escape{position:absolute;left:120px;top:50px;width:20px;height:20px;background:blue;z-index:2}\
         #fixed{position:fixed;left:10px;top:10px;width:20px;height:20px;background:green;z-index:3}\
         #root-tail{height:800px}",
        "<div id=cb><div id=clip><div id=escape></div><div id=fixed></div></div></div><div id=root-tail></div>",
    );
    let escape = id(&doc, "escape");
    let fixed = id(&doc, "fixed");
    assert_eq!(tree.hit_test(150.0, 80.0), Some(escape));
    let zero = pixels(&tree, 1.0, false);
    assert_eq!(zero[80 * 300 + 150], 0x0000ff);
    tree.set_viewport_scroll(ScrollOffset { x: 0.0, y: 100.0 });
    assert_eq!(tree.hit_test_viewport(15.0, 15.0, 100.0), Some(fixed));
    assert_eq!(pixels(&tree, 1.0, false)[15 * 300 + 15], 0x008000);
    // A positioned intermediate context may escape a paint group, not its clip.
    let (doc, mut tree) = page(
        "#outer{position:relative;overflow:auto;width:100px;height:60px}\
         #auto{position:relative}\
         #inside{position:absolute;left:80px;top:40px;width:50px;height:80px;background:red;z-index:9}",
        "<div id=outer><div id=auto><div id=inside></div></div></div>",
    );
    let outer = id(&doc, "outer");
    let inside = id(&doc, "inside");
    close(tree.scroll_container(outer).unwrap().range.max.y, 60.0);
    close(tree.scroll_container(outer).unwrap().range.max.x, 30.0);
    assert_ne!(tree.hit_test(130.0, 100.0), Some(inside));
    assert_eq!(pixels(&tree, 1.0, false)[100 * 300 + 130], 0xffffff);
    tree.set_scroll_offset(outer, ScrollOffset { x: 30.0, y: 40.0 });
    assert_eq!(tree.hit_test(80.0, 35.0), Some(inside));
    assert_eq!(pixels(&tree, 1.0, false)[35 * 300 + 80], 0xff0000);
}

#[test]
fn root_body_propagation_controls_viewport_without_clipping_fixed_headers() {
    for (css, expected) in [
        ("body{overflow:hidden}", Overflow::Hidden),
        ("html{overflow:clip}body{overflow:auto}", Overflow::Clip),
        (
            "html{overflow:auto}body{overflow:hidden;height:60px}",
            Overflow::Auto,
        ),
    ] {
        let (doc, mut tree) = page(
            &format!(
                "{css}#tall{{height:700px}}#f{{position:fixed;inset:0 auto auto 0;width:20px;height:20px;background:red}}"
            ),
            "<div id=tall></div><div id=f></div>",
        );
        assert_eq!(tree.viewport_scroll_container().overflow_y, expected);
        if expected == Overflow::Hidden {
            assert!(tree.scroll_container(id(&doc, "tall")).is_none());
            assert!(tree.set_viewport_scroll(ScrollOffset { x: 0.0, y: 100.0 }));
            assert!(!tree.scroll_wheel(100.0, 100.0, ScrollOffset { x: 0.0, y: 50.0 }));
        }
        if expected == Overflow::Clip {
            assert_eq!(tree.viewport_scroll_container().range.max.y, 0.0);
            assert!(!tree.set_viewport_scroll(ScrollOffset { x: 0.0, y: 50.0 }));
        }
        assert_eq!(pixels(&tree, 1.0, false)[5 * 300 + 5], 0xff0000);
        assert_eq!(
            tree.hit_test_viewport(5.0, 5.0, tree.viewport_scroll().y),
            Some(id(&doc, "f"))
        );
    }
}

#[test]
fn ink_and_replaced_content_clips_at_fractional_scale_without_moving_glyph_indices() {
    let (doc, mut tree) = page(
        "#p{width:80px;height:22px;overflow:hidden;position:relative}\
         #t{font:italic bold 32px/0 sans-serif;white-space:pre;position:relative;left:5px;top:12px}",
        "<div id=p><span id=t>MMMMMMMM<br>MMMMMMMM</span></div>",
    );
    let p = id(&doc, "p");
    assert!(tree.scroll_container(p).unwrap().range.max.x > 80.0);
    assert!(tree.scroll_container(p).unwrap().extent.y < tree.node_rect(p).unwrap().y + 12.0);
    assert_eq!(tree.viewport_scroll_container().range.max.x, 0.0);
    let before = tree.clone();
    tree.set_scroll_offset(p, ScrollOffset { x: 13.25, y: 2.75 });
    fn glyphs(b: &LayoutBox) -> Vec<std::sync::Arc<crate::text::ShapedText>> {
        b.text_fragments()
            .iter()
            .map(|f| f.glyphs.clone())
            .chain(b.children().iter().flat_map(glyphs))
            .collect()
    }
    let old = glyphs(before.root());
    let new = glyphs(tree.root());
    for (a, b) in old.iter().zip(&new) {
        assert!(std::sync::Arc::ptr_eq(a, b));
    }
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for outlines in [false, true] {
            let image = pixels(&tree, scale, outlines);
            let width = (300.0 * scale) as usize;
            // No indexed ink or synthetic bold/slant can cross the panel edge.
            for y in (27.0 * scale) as usize..(40.0 * scale) as usize {
                assert_eq!(
                    image[y * width + (105.0 * scale).ceil() as usize],
                    0xffffff,
                    "scale={scale} outlines={outlines} y={y}"
                );
            }
            if !outlines {
                assert!(image.contains(&0));
            }
        }
    }
}

#[test]
fn percentage_sizes_offsets_clone_and_frame_relayout_are_independent_and_clamped() {
    let source = "<style>html,body{margin:0}#parent{height:120px;width:200px}\
        #p{width:50%;height:50%;overflow:auto}#c{height:300px;width:250px}\
        @media(max-width:250px){#c{height:30px;width:30px}}</style>\
        <div id=parent><div id=p><div id=c></div></div></div>";
    let document = parser::parse(source).unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://overflow.test/").unwrap(),
        document,
        MediaEnvironment {
            viewport_width: 300.0,
            viewport_height: 200.0,
            ..MediaEnvironment::default()
        },
    );
    let node = id(&frame.document, "p");
    let font = FontContext::bundled();
    let mut tree = frame.layout_with_container_queries(&font);
    assert_eq!(tree.node_rect(node).unwrap().width, 100.0);
    assert_eq!(tree.node_rect(node).unwrap().height, 60.0);
    tree.set_scroll_offset(node, ScrollOffset { x: 30.5, y: 90.25 });
    frame.retain_scroll_state(&tree);
    let cloned_frame = frame.clone();
    let mut cloned = tree.clone();
    cloned.set_scroll_offset(node, ScrollOffset { x: 100.0, y: 200.0 });
    assert_eq!(tree.scroll_offset(node).y, 90.25);
    tree.layout_viewport(300.0, 200.0, &font);
    assert_eq!(tree.scroll_offset(node).y, 90.25);
    let again = tree.clone();
    tree.layout_viewport(300.0, 200.0, &font);
    assert_eq!(tree, again);
    let rebuilt = frame.layout_with_container_queries(&font);
    assert_eq!(
        rebuilt.scroll_offset(node),
        ScrollOffset { x: 30.5, y: 90.25 }
    );
    frame.set_media_environment(MediaEnvironment {
        viewport_width: 240.0,
        ..frame.media_environment()
    });
    let shrunk = frame.layout_with_container_queries(&font);
    assert_eq!(shrunk.scroll_offset(node), ScrollOffset::default());
    assert_eq!(cloned_frame.scroll_state(), rebuilt.scroll_state());
    let mut fresh = Frame::new_with_environment(
        url::Url::parse("https://overflow.test/next").unwrap(),
        frame.document.clone(),
        cloned_frame.media_environment(),
    );
    assert_eq!(
        fresh
            .layout_with_container_queries(&font)
            .scroll_offset(node),
        ScrollOffset::default()
    );
}

#[test]
fn invalid_offsets_and_empty_ports_are_bounded_with_used_value_diagnostics() {
    let (doc, mut tree) = page(
        "#p{width:0;height:0;overflow:scroll}#c{width:300px;height:300px}",
        "<div id=p><div id=c></div></div>",
    );
    let node = id(&doc, "p");
    assert_eq!(
        tree.scroll_container(node).unwrap().range,
        ScrollRange::default()
    );
    assert!(!tree.set_scroll_offset(
        node,
        ScrollOffset {
            x: f32::NAN,
            y: f32::INFINITY
        }
    ));
    assert_eq!(tree.diagnostics().len(), 2);
    assert!(
        tree.diagnostics()
            .iter()
            .all(|d| d.message.contains("scroll"))
    );
    assert!(!tree.set_scroll_offset(node, ScrollOffset { x: -10.0, y: -10.0 }));
}

#[test]
fn scrollbar_tracks_drag_and_hit_the_same_clipped_context_order() {
    let (doc, mut tree) = page(
        "#p{overflow:auto;width:100px;height:60px;position:relative}\
         #content{height:300px;width:200px;background:red}\
         #overlay{position:fixed;left:118px;top:24px;width:6px;height:20px;background:blue;z-index:10}",
        "<div id=p><div id=content></div></div><div id=overlay></div>",
    );
    let p = id(&doc, "p");
    let bars = tree.scrollbars();
    assert_eq!(bars.len(), 2);
    let vertical = bars.iter().find(|b| b.vertical).unwrap();
    let x = vertical.track.x + 2.0;
    assert!(tree.scrollbar_at(x, 30.0).is_none()); // High z context covers it.
    assert_eq!(pixels(&tree, 1.0, false)[30 * 300 + x as usize], 0x0000ff);
    let y = vertical.track.y + vertical.track.height - 2.0;
    let bar = tree.scrollbar_at(x, y).unwrap();
    assert_eq!(tree.hit_test(x, y), Some(p));
    let grab = tree.scrollbar_grab(&bar, x, y);
    assert!(tree.drag_scrollbar(&bar, x, y, grab));
    assert_eq!(
        tree.scroll_offset(p).y,
        tree.scroll_container(p).unwrap().range.max.y
    );
    assert!(!tree.drag_scrollbar(&bar, x, 5000.0, grab)); // Already bounded at end.
    assert!(tree.drag_scrollbar(&bar, x, -5000.0, grab));
    assert_eq!(tree.scroll_offset(p).y, 0.0);
    let (doc, mut tree) = page(
        "#outer{height:30px;overflow:hidden}#p{height:60px;width:100px;overflow:scroll}#content{height:200px}",
        "<div id=outer><div id=p><div id=content></div></div></div>",
    );
    assert!(tree.scrollbar_at(120.0, 75.0).is_none());
    let p = id(&doc, "p");
    let outer = id(&doc, "outer");
    tree.set_scroll_offset(outer, ScrollOffset { x: 0.0, y: 30.0 });
    assert_eq!(tree.scrollbar_at(120.0, 45.0).unwrap().node_id(), p);
}

#[test]
fn replaced_images_keep_sampling_origins_and_clip_device_pixels() {
    use std::{io::Cursor, sync::Arc};
    let (doc, _) = page(
        "#p{overflow:hidden;width:40px;height:30px}img{width:100px;height:80px}",
        "<div id=p><img id=image src=image.png></div>",
    );
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_fn(2, 2, |x, y| {
        if x == 0 && y == 0 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let decoded = Arc::new(crate::media::DecodedImage::decode(png.get_ref()).unwrap());
    let images = [(id(&doc, "image"), decoded)].into_iter().collect();
    let styles = style::resolve(&doc).styles;
    let mut tree = build_layout_tree_with_resources(&doc, &styles, &images);
    tree.layout_viewport(300.0, 200.0, &FontContext::bundled());
    let p = id(&doc, "p");
    assert!(tree.set_scroll_offset(p, ScrollOffset { x: 30.25, y: 20.5 }));
    for scale in [1.0, 1.25, 2.0] {
        let image = pixels(&tree, scale, false);
        let width = (300.0 * scale) as usize;
        let pixel = |x: f32, y: f32| image[(y * scale) as usize * width + (x * scale) as usize];
        assert_eq!(pixel(30.0, 30.0), 0xff0000);
        assert_eq!(pixel(55.0, 48.0), 0x0000ff);
        assert_eq!(pixel(65.0, 40.0), 0xffffff);
        assert_eq!(pixel(40.0, 55.0), 0xffffff);
    }
    assert_eq!(tree.hit_test(30.0, 30.0), Some(id(&doc, "image")));
    assert_ne!(tree.hit_test(65.0, 40.0), Some(id(&doc, "image")));
}

#[test]
fn negative_ink_and_rtl_ranges_are_distinct_from_layout_and_zero_scroll_origin() {
    let (doc, mut tree) = page(
        "#p{overflow:auto;width:80px;height:40px;direction:rtl;position:relative}\
         #c{position:absolute;left:-50px;top:-10px;width:100px;height:20px;background:red}",
        "<div id=p><div id=c></div></div>",
    );
    let p = id(&doc, "p");
    let container = tree.scroll_container(p).unwrap();
    close(container.extent.x, -26.0);
    close(container.extent.y, 14.0);
    close(container.range.min.x, -50.0);
    assert_eq!(container.range.max.x, 0.0);
    assert_eq!(container.range.min.y, 0.0);
    assert!(tree.set_scroll_offset(
        p,
        ScrollOffset {
            x: -500.0,
            y: -50.0
        }
    ));
    assert_eq!(tree.scroll_offset(p), ScrollOffset { x: -50.0, y: 0.0 });
    assert_eq!(tree.hit_test(100.0, 30.0), Some(id(&doc, "c")));
    assert!(tree.scroll_wheel(100.0, 30.0, ScrollOffset { x: 500.0, y: 0.0 }));
    assert_eq!(tree.scroll_offset(p).x, 0.0);
    assert_eq!(tree.viewport_scroll().x, 0.0);
    assert_eq!(tree.hit_test_viewport(310.0, 30.0, 0.0), None);
}

#[test]
fn font_resource_snapshots_and_container_feedback_retain_document_offsets() {
    use base64::Engine;
    let font_data = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../assets/fonts/noto-sans/NotoSansItalic-fixture.ttf"
    ));
    let source = format!(
        "<style>html,body{{margin:0}}\
        @font-face{{font-family:Downloaded;src:url(data:font/ttf;base64,{font_data});font-style:italic}}\
        #host{{container:host / inline-size;width:200px}}\
        #p{{overflow:auto;width:100px;height:60px;font:italic 20px/24px Downloaded}}\
        #c{{width:250px;height:300px}}\
        @container host (width>100px){{#c{{height:320px}}}}</style>\
        <div id=host><div id=p><div id=c>AVAVAVAVAVAV</div></div></div>"
    );
    let doc = parser::parse(&source).unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://overflow.test/fonts").unwrap(),
        doc,
        MediaEnvironment::new(300.0, 200.0),
    );
    let node = id(&frame.document, "p");
    let font = FontContext::bundled();
    let mut tree = frame.layout_with_container_queries(&font);
    close(
        tree.node_rect(id(&frame.document, "c")).unwrap().height,
        320.0,
    );
    tree.set_scroll_offset(node, ScrollOffset { x: 35.25, y: 110.5 });
    frame.retain_scroll_state(&tree);
    let mut loading = frame.clone();
    assert_eq!(
        style::font_rules(
            &loading.document,
            &loading.stylesheets,
            loading.media_environment()
        )
        .len(),
        1,
        "{:?}",
        loading.stylesheets.diagnostics
    );
    tree.set_scroll_offset(node, ScrollOffset { x: 45.5, y: 120.25 });
    frame.retain_scroll_state(&tree);
    loading
        .load_pending_web_fonts(|_| Ok(()), || false)
        .unwrap();
    // This is the exact retained-state handoff used for asynchronous UI fonts.
    loading.copy_scroll_state_from(&frame);
    let loaded = loading.layout_with_container_queries(&font);
    assert_eq!(
        loaded.scroll_offset(node),
        ScrollOffset { x: 45.5, y: 120.25 }
    );
    fn faces(b: &LayoutBox) -> Vec<bool> {
        b.text_fragments()
            .iter()
            .flat_map(|f| f.glyphs.glyphs.iter().map(|g| g.face.italic))
            .chain(b.children().iter().flat_map(faces))
            .collect()
    }

    assert!(loading.styles.get(node).unwrap().font_face.italic);
    assert!(faces(loaded.root()).contains(&true));
    assert!(!faces(tree.root()).iter().any(|&italic| italic));
    assert_eq!(loading.layout_with_container_queries(&font), loaded);
}

#[test]
fn fixed_scroll_containers_ignore_root_axes_and_nested_fixed_escapes_their_clip() {
    let (doc, mut tree) = page(
        "#p{overflow:hidden;width:60px;height:40px}\
             #f{position:fixed;left:5px;top:5px;width:100px;height:50px;overflow:auto;z-index:1}\
             #c{height:180px;width:160px;background:red}\
             #nested{position:fixed;left:150px;top:10px;width:20px;height:20px;background:blue;z-index:9}\
             #wide{width:450px;height:700px;background:green}",
        "<div id=p><div id=f><div id=c></div><div id=nested></div></div></div><div id=wide></div>",
    );
    let f = id(&doc, "f");
    tree.set_scroll_offset(f, ScrollOffset { x: 20.0, y: 30.0 });
    let first = pixels(&tree, 1.0, false);
    assert!(tree.set_viewport_scroll(ScrollOffset { x: 45.0, y: 60.0 }));
    let moved = pixels(&tree, 1.0, false);
    assert_eq!(first[15 * 300 + 15], 0xff0000);
    assert_eq!(moved[15 * 300 + 15], 0xff0000);
    assert_eq!(moved[15 * 300 + 155], 0x0000ff);
    assert_eq!(
        tree.hit_test_viewport(15.0, 15.0, 60.0),
        Some(id(&doc, "c"))
    );
    assert_eq!(
        tree.hit_test_viewport(155.0, 15.0, 60.0),
        Some(id(&doc, "nested"))
    );
    assert_eq!(tree.visual_node_rect(f, 60.0).unwrap().x, 5.0);
    assert_eq!(tree.hit_test(450.0, 100.0), Some(id(&doc, "wide")));
    assert_eq!(tree.hit_test(155.0, 15.0), Some(id(&doc, "nested")));
}

#[test]
fn clipped_visual_bounds_include_every_block_in_inline_continuation() {
    let (doc, tree) = page(
        "#p{overflow:hidden;width:80px;height:100px}\
             #wrap{padding:3px;border:2px solid red}#block{height:20px}",
        "<div id=p><span id=wrap>one<div id=block></div>two two two two</span></div>",
    );
    let node = id(&doc, "wrap");
    fn collect(b: &LayoutBox, node: NodeId, rects: &mut Vec<Rect>) {
        if b.dom_node_id() == Some(node) && b.pseudo_element().is_none() {
            rects.push(b.rect());
        }
        for child in b.children() {
            collect(child, node, rects);
        }
    }

    let mut rects = Vec::new();
    collect(tree.root(), node, &mut rects);
    assert_eq!(rects.len(), 2);
    let port = tree.scroll_container(id(&doc, "p")).unwrap().port;
    let clip = ClipRect {
        x: Some((port.x, port.x + port.width)),
        y: Some((port.y, port.y + port.height)),
    };
    let expected = rects
        .into_iter()
        .filter_map(|r| clip.apply(r))
        .reduce(Rect::union)
        .unwrap();
    assert_eq!(tree.visual_node_rect(node, 0.0).unwrap(), expected);
    assert!(expected.height > 40.0);
}

#[test]
fn conditional_absence_retains_offsets_but_new_documents_cannot_adopt_old_snapshots() {
    let doc = parser::parse(
        "<style>#p{overflow:auto;width:100px;height:60px}#c{height:300px}\
            @media(max-width:280px){#p{display:none}}</style><div id=p><div id=c></div></div>",
    )
    .unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://overflow.test/").unwrap(),
        doc,
        MediaEnvironment::new(300.0, 200.0),
    );
    let node = id(&frame.document, "p");
    let font = FontContext::bundled();
    let mut tree = frame.layout_with_container_queries(&font);
    tree.set_scroll_offset(node, ScrollOffset { x: 0.0, y: 75.5 });
    frame.retain_scroll_state(&tree);
    frame.set_media_environment(MediaEnvironment::new(250.0, 200.0));
    let absent = frame.layout_with_container_queries(&font);
    assert!(absent.node_rect(node).is_none());
    assert_eq!(absent.scroll_offset(node).y, 75.5);
    frame.set_media_environment(MediaEnvironment::new(300.0, 200.0));
    let returned = frame.layout_with_container_queries(&font);
    assert_eq!(returned.scroll_offset(node).y, 75.5);
    let mut fresh = Frame::new_with_environment(
        frame.url.clone(),
        frame.document.clone(),
        frame.media_environment(),
    );
    fresh.copy_scroll_state_from(&frame);
    fresh.retain_scroll_state(&returned);
    assert_eq!(
        fresh
            .layout_with_container_queries(&font)
            .scroll_offset(node),
        ScrollOffset::default()
    );
}

#[test]
fn pseudo_scroll_identity_is_separate_and_clamps_even_with_same_origin_text_boxes() {
    let doc = parser::parse("<style>html,body{margin:0}\
            #owner{width:180px;height:100px;overflow:hidden}\
            #owner::before{content:'MMMMMMMMMMMMMMMM';display:block;width:80px;height:20px;white-space:nowrap;overflow:auto}\
            #child{height:300px;width:300px}\
            @media(max-width:250px){#owner::before{content:''}}</style>\
            <div id=owner><div id=child></div></div>").unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://overflow.test/pseudo").unwrap(),
        doc,
        MediaEnvironment::new(300.0, 200.0),
    );
    let owner = id(&frame.document, "owner");
    let font = FontContext::bundled();
    let mut tree = frame.layout_with_container_queries(&font);
    assert!(tree.scroll_wheel(35.0, 30.0, ScrollOffset { x: 20.0, y: 0.0 }));
    assert_eq!(tree.scroll_offset(owner), ScrollOffset::default());
    let bar = tree
        .scrollbars()
        .iter()
        .find(|b| !b.vertical)
        .unwrap()
        .clone();
    assert!(bar.thumb.x > bar.track.x);
    tree.set_scroll_offset(owner, ScrollOffset { x: 30.0, y: 40.0 });
    frame.retain_scroll_state(&tree);
    let rebuilt = frame.layout_with_container_queries(&font);
    assert_eq!(
        rebuilt.scroll_offset(owner),
        ScrollOffset { x: 30.0, y: 40.0 }
    );
    let bar = rebuilt
        .scrollbars()
        .iter()
        .find(|b| !b.vertical)
        .unwrap()
        .clone();
    assert!(bar.thumb.x > bar.track.x);
    frame.set_media_environment(MediaEnvironment::new(240.0, 200.0));
    assert!(
        frame
            .layout_with_container_queries(&font)
            .scrollbars()
            .is_empty()
    );
    frame.set_media_environment(MediaEnvironment::new(300.0, 200.0));
    let restored = frame.layout_with_container_queries(&font);
    let bar = restored
        .scrollbars()
        .iter()
        .find(|b| !b.vertical)
        .unwrap()
        .clone();
    assert_eq!(bar.thumb.x, bar.track.x);
}

#[test]
fn outside_marker_ink_is_measured_but_cannot_paint_or_hit_outside_the_port() {
    let (doc, tree) = page(
        "#p{overflow:hidden;width:80px;height:22px;margin:0;padding:0}li{font:32px/32px sans-serif}",
        "<ul id=p><li id=item>MMMMMMMM</li></ul>",
    );
    let p = id(&doc, "p");
    let item = id(&doc, "item");
    let container = tree.scroll_container(p).unwrap();
    assert!(container.extent.x < container.port.x);
    assert!(container.range.max.x > 0.0);
    assert_eq!(tree.viewport_scroll_container().range.max.x, 0.0);
    assert_eq!(tree.viewport_scroll_container().range.max.y, 0.0);
    let image = pixels(&tree, 1.0, false);
    for y in 25..45 {
        assert_eq!(image[y * 300 + 20], 0xffffff);
        assert_ne!(tree.hit_test(20.0, y as f32), Some(item));
    }
}
