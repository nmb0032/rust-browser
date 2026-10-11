use super::*;
use crate::{
    css::{media::MediaEnvironment, values::Side},
    dom::{Document, NodeId},
    engine::{FragmentTarget, Frame},
    html::parser,
    paint,
    style::{
        self, Position,
        scrolling::{Align, Axis, Behavior, Overscroll, Property, Value, Width},
    },
    text::FontContext,
};

fn id(d: &Document, name: &str) -> NodeId {
    d.preorder()
        .find(|&id| {
            d.node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}
fn page(css: &str, html: &str) -> (Frame, LayoutTree) {
    let document = parser::parse(&format!(
        "<!doctype html><style>html,body{{margin:0}}{css}</style>{html}"
    ))
    .unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://example.test/").unwrap(),
        document,
        MediaEnvironment {
            viewport_width: 300.0,
            viewport_height: 200.0,
            ..MediaEnvironment::default()
        },
    );
    let tree = frame.layout_with_container_queries(&FontContext::bundled());
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (frame, tree)
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}
fn pixels(tree: &LayoutTree) -> Vec<u32> {
    let mut pixels = vec![0xffffff; 300 * 200];
    paint::rasterize_display_list(
        &paint::build_display_list(tree, 300.0, 200.0, false),
        &FontContext::bundled(),
        &mut pixels,
        300,
        200,
        1.0,
        tree.viewport_scroll().y,
    );
    pixels
}
#[test]
fn scrolling_values_are_typed_atomic_logical_inherited_and_contextual() {
    let (frame, _) = page(
        "#p{color:red;scrollbar-color:currentcolor blue;scrollbar-width:thin;scroll-behavior:smooth;scroll-snap-type:both mandatory;scroll-snap-align:start end;scroll-snap-stop:always;overscroll-behavior:contain none;scroll-padding:10% 2em auto 4px;scroll-margin:-2px 3px;writing-mode:vertical-rl;direction:rtl;scroll-padding-block-start:9px;scroll-margin-inline-start:7px}#c{color:green}",
        "<div id=p><div id=c></div></div>",
    );
    let s = &frame
        .styles
        .get(id(&frame.document, "p"))
        .unwrap()
        .scrolling;
    assert_eq!(s.behavior(), Behavior::Smooth);
    assert_eq!(
        s.get(Property::SnapType),
        Value::SnapType(Some((Axis::Both, true)))
    );
    assert_eq!(
        s.get(Property::SnapAlign),
        Value::Align(Align::Start, Align::End)
    );
    assert_eq!(s.get(Property::SnapStop), Value::Stop(true));
    assert_eq!(s.overscroll(false), Overscroll::Contain);
    assert_eq!(s.overscroll(true), Overscroll::None);
    close(s.edge(Side::Right, true, 100.0), 9.0);
    close(s.edge(Side::Top, true, 100.0), 10.0);
    close(s.edge(Side::Bottom, false, 100.0), 7.0);
    let c = frame.styles.get(id(&frame.document, "c")).unwrap();
    assert_eq!(c.scrolling.width(), Width::Auto);
    assert_eq!(c.scrolling.behavior(), Behavior::Auto);
    assert_eq!(c.scrolling.colors(c.text_color).0.to_srgb8().0, 0x008000);
    let d = parser::parse("<div id=p style='scroll-snap-type:x;scroll-snap-align:center;scroll-padding:2px;scrollbar-color:red blue;scroll-snap-type:y invalid;scroll-snap-align:center start end;scroll-padding:1px -2px;scroll-margin:10%;scrollbar-color:red;overscroll-behavior:contain none auto'></div>").unwrap();
    let r = style::resolve(&d);
    assert_eq!(r.stylesheets.diagnostics.len(), 6);
    let s = &r.styles.get(id(&d, "p")).unwrap().scrolling;
    assert_eq!(
        s.get(Property::SnapType),
        Value::SnapType(Some((Axis::X, false)))
    );
    close(s.edge(Side::Left, true, 100.0), 2.0);
}
#[test]
fn sticky_root_paint_hit_and_normal_geometry_never_drift() {
    let (mut f, mut t) = page(
        "#s{position:sticky;top:8px;height:20px;background:red}#tail{height:500px}",
        "<div id=s><a id=link href='#tail'>link</a></div><div id=tail></div>",
    );
    let s = id(&f.document, "s");
    let original = t.node_rect(s).unwrap();
    for y in [40.0, 80.0, 40.0, 0.0, 100.0] {
        t.set_viewport_scroll(ScrollOffset { x: 0.0, y });
        let r = t.visual_node_rect(s, y).unwrap();
        close(r.y, if y == 0.0 { original.y } else { 8.0 });
        assert_eq!(t.node_rect(s).unwrap(), original);
        assert_eq!(pixels(&t)[(r.y as usize + 1) * 300 + 200], 0xff0000);
        let hit = t.hit_test_viewport(30.0, r.y + 5.0, y).unwrap();
        assert!(
            hit == id(&f.document, "link")
                || f.document.node(hit).unwrap().parent() == Some(id(&f.document, "link"))
        );
        f.retain_scroll_state(&t);
        let mut snapshot = f.clone();
        let next = snapshot.layout_with_container_queries(&FontContext::bundled());
        assert_eq!(next.node_rect(s), Some(original));
        assert_eq!(pixels(&next), pixels(&t));
    }
}
#[test]
fn sticky_nested_containing_end_opposing_insets_and_negative_margins() {
    let (f, mut t) = page(
        "#p{width:100px;height:80px;overflow:hidden}#cb{height:130px}#s{position:sticky;top:5px;bottom:5px;height:20px;background:red;margin-bottom:-2px}#tail{height:200px}",
        "<div id=p><div id=cb><div id=s></div></div><div id=tail></div></div>",
    );
    let p = id(&f.document, "p");
    let s = id(&f.document, "s");
    let normal = t.node_rect(s).unwrap();
    t.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 40.0 });
    close(t.visual_node_rect(s, 0.0).unwrap().y, normal.y + 5.0);
    t.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 120.0 });
    let state = t.spatial_states(false);
    let b = overflow_box(t.root(), s);
    let rect = state[&b.box_id].matrix.bounds(b.rect).unwrap();
    close(rect.y, normal.y - 8.0);
    let (f, mut t) = page(
        "#p{width:100px;height:50px;overflow:hidden}#cb{height:300px}#s{position:sticky;top:10px;bottom:10px;height:80px;background:blue}#tail{height:200px}",
        "<div id=p><div id=cb><div id=s></div><div id=tail></div></div></div>",
    );
    let p = id(&f.document, "p");
    let s = id(&f.document, "s");
    t.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 30.0 });
    close(
        t.visual_node_rect(s, 0.0).unwrap().y,
        t.scroll_container(p).unwrap().port.y + 10.0,
    );
    let (f, mut t) = page(
        "#p{width:100px;height:80px;overflow:auto}#s{position:sticky;top:5px;height:20px;background:red}#tail{height:300px}",
        "<div id=p><div id=s></div><div id=tail></div></div>",
    );
    let p = id(&f.document, "p");
    let s = id(&f.document, "s");
    t.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 140.0 });
    close(
        t.visual_node_rect(s, 0.0).unwrap().y,
        t.scroll_container(p).unwrap().port.y + 5.0,
    );
}
fn overflow_box(b: &LayoutBox, node: NodeId) -> &LayoutBox {
    if b.dom_node_id() == Some(node) {
        return b;
    }
    b.children().iter().find_map(|b| find_box(b, node)).unwrap()
}
fn find_box(b: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
    if b.dom_node_id() == Some(node) {
        Some(b)
    } else {
        b.children().iter().find_map(|b| find_box(b, node))
    }
}
#[test]
fn nested_sticky_translates_descendants_once_fixed_escapes_and_transform_hits_match() {
    let (f, mut t) = page(
        "#p{width:120px;height:70px;overflow:auto;transform:translateX(20px)}#outer{position:sticky;top:0;height:40px;background:red}#inner{position:sticky;top:5px;height:10px;background:blue}#fixed{position:fixed;top:0;left:0;width:10px;height:10px;background:green}#tail{height:300px}",
        "<div id=p><div id=outer><div id=inner><a id=link href='#tail'>x</a></div><div id=fixed></div></div><div id=tail></div></div>",
    );
    let p = id(&f.document, "p");
    let inner = id(&f.document, "inner");
    let y = t.scroll_container(p).unwrap().port.y;
    t.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 30.0 });
    let r = t.visual_node_rect(inner, 0.0).unwrap();
    close(r.y, y + 5.0);
    close(r.x, 44.0);
    let hit = t.hit_test_viewport(r.x + 2.0, r.y + 3.0, 0.0).unwrap();
    assert!(
        hit == id(&f.document, "link")
            || f.document.node(hit).unwrap().parent() == Some(id(&f.document, "link"))
    );
    let fixed = overflow_box(t.root(), id(&f.document, "fixed"));
    assert_eq!(fixed.coordinate_space(), CoordinateSpace::Document);
    assert!(pixels(&t).contains(&0xff0000));
}
#[test]
fn sticky_creates_auto_z_context_without_changing_flow_or_margin_collapse() {
    let (f, t) = page(
        "#before{height:10px;margin-bottom:20px}#s{position:sticky;top:0;height:20px;margin-top:30px}#a{position:absolute;z-index:9;top:0;left:0;width:40px;height:40px;background:red}#b{position:relative;z-index:1;top:-20px;width:40px;height:40px;background:blue}",
        "<div id=before></div><div id=s><div id=a></div></div><div id=b></div>",
    );
    let s = overflow_box(t.root(), id(&f.document, "s"));
    assert_eq!(s.style().position, Position::Sticky);
    close(s.rect().y, 64.0);
    let order = paint_order(&t);
    let last = order
        .iter()
        .rposition(|(b, p)| {
            b.dom_node_id() == Some(id(&f.document, "b")) && *p == PaintPhase::Decoration
        })
        .unwrap();
    let child = order
        .iter()
        .rposition(|(b, p)| {
            b.dom_node_id() == Some(id(&f.document, "a")) && *p == PaintPhase::Decoration
        })
        .unwrap();
    assert!(last > child);
}
fn snap_page(extra: &str) -> (Frame, LayoutTree, NodeId) {
    let (f, t) = page(
        &format!(
            "#p{{width:100px;height:100px;overflow:auto;scrollbar-width:none;scroll-snap-type:y mandatory;{extra}}}.item{{height:100px;scroll-snap-align:start}}"
        ),
        "<div id=p><div class=item id=a></div><div class=item id=b></div><div class=item id=c></div><div class=item id=d></div></div>",
    );
    let p = id(&f.document, "p");
    (f, t, p)
}
#[test]
fn snaps_only_at_gesture_end_and_programmatic_boundary_with_proximity_and_stop() {
    let (_f, mut t, p) = snap_page("");
    assert!(t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 64.0 }));
    close(t.scroll_offset(p).y, 64.0);
    t.finish_scroll_gesture();
    close(t.scroll_offset(p).y, 100.0);
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 170.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 200.0);
    let (mut f, mut t, p) = snap_page("scroll-snap-type:y proximity");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 50.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 50.0);
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 78.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 100.0);
    f.retain_scroll_state(&t);
    assert!(!f.scroll_state().needs_tick());
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:auto;scroll-snap-type:y mandatory}.item{height:100px;scroll-snap-align:start}#b{scroll-snap-stop:always}",
        "<div id=p><div class=item></div><div id=b class=item></div><div class=item></div><div class=item></div></div>",
    );
    let p = id(&f.document, "p");
    t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 270.0 });
    close(t.scroll_offset(p).y, 270.0);
    t.finish_scroll_gesture();
    close(t.scroll_offset(p).y, 100.0);
}
#[test]
fn snap_margin_padding_oversized_escape_and_hidden_programmatic_scroll() {
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:hidden;scroll-snap-type:y mandatory;scroll-padding-top:10%;scroll-padding-bottom:10px}.item{height:70px;margin-bottom:30px;scroll-snap-align:start;scroll-margin-top:5px}#large{height:250px}",
        "<div id=p><div class=item></div><div class=item id=b></div><div class=item id=large></div><div class=item></div></div>",
    );
    let p = id(&f.document, "p");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 95.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 85.0);
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 280.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 280.0);
    assert!(!t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 10.0 }));
}
#[test]
fn snap_rtl_negative_origin_logical_axes_and_layout_resnap_stable_target() {
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:hidden;direction:rtl;scroll-snap-type:inline mandatory;scroll-padding-inline-start:10px}#a{width:250px;height:20px;scroll-snap-align:start}#tail{width:400px;height:20px}",
        "<div id=p><div id=a></div><div id=tail></div></div>",
    );
    let p = id(&f.document, "p");
    assert!(t.scroll_container(p).unwrap().range.min.x < 0.0);
    t.request_scroll(
        Some(p),
        ScrollOffset { x: -70.0, y: 0.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).x, -70.0);
    let (mut f, mut t, p) = snap_page("");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 190.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    f.retain_scroll_state(&t);
    let source=parser::parse("<style>#p{width:100px;height:100px;overflow:auto;scroll-snap-type:y mandatory}.item{height:120px;scroll-snap-align:start}</style>").unwrap();
    f.replace_author_stylesheets(style::collect_stylesheets(&source));
    let t = f.layout_with_container_queries(&FontContext::bundled());
    close(t.scroll_offset(p).y, 240.0);
    let source=parser::parse("<style>#p{width:100px;height:100px;overflow:auto;scroll-snap-type:y mandatory}.item{height:120px;scroll-snap-align:start}#c{display:none}#d{margin-top:60px}</style>").unwrap();
    f.replace_author_stylesheets(style::collect_stylesheets(&source));
    let t = f.layout_with_container_queries(&FontContext::bundled());
    close(t.scroll_offset(p).y, 300.0);
}
#[test]
fn smooth_clock_clones_interruption_completion_and_keyboard_share_motion() {
    let (mut f, mut t, p) = snap_page("scroll-behavior:smooth;scroll-snap-type:none");
    t.sample_scrolling(1.0).unwrap();
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 200.0 },
        ScrollBehavior::Auto,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 0.0);
    assert!(t.scroll_state().has_animations());
    let frozen = t.clone();
    t.sample_scrolling(1.14).unwrap();
    close(t.scroll_offset(p).y, 100.0);
    close(frozen.scroll_offset(p).y, 0.0);
    f.retain_scroll_state(&t);
    f.set_animation_time(1.21).unwrap();
    let next = f.layout_with_container_queries(&FontContext::bundled());
    assert!(next.scroll_offset(p).y > 150.0);
    close(t.scroll_offset(p).y, 100.0);
    t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 10.0 });
    close(t.scroll_offset(p).y, 110.0);
    t.sample_scrolling(5.0).unwrap();
    close(t.scroll_offset(p).y, 110.0);
    assert!(!t.scroll_state().needs_tick());
    assert!(
        t.request_scroll(
            Some(p),
            ScrollOffset {
                x: f32::NAN,
                y: 0.0
            },
            ScrollBehavior::Auto
        )
        .is_err()
    );
    t.scroll_key(40.0, 40.0, ScrollOffset { x: 0.0, y: 40.0 })
        .unwrap();
    close(t.scroll_offset(p).y, 110.0);
    t.sample_scrolling(6.0).unwrap();
    close(t.scroll_offset(p).y, 150.0);
    assert!(!t.scroll_state().needs_tick());
}
#[test]
fn wheel_deadline_is_explicit_no_sleep_and_cancels_same_axis_only() {
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:auto;scroll-behavior:smooth}#c{width:500px;height:500px}",
        "<div id=p><div id=c></div></div>",
    );
    let p = id(&f.document, "p");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 100.0, y: 100.0 },
        ScrollBehavior::Smooth,
    )
    .unwrap();
    t.sample_scrolling(0.1).unwrap();
    let y = t.scroll_offset(p).y;
    t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 10.0, y: 0.0 });
    let x = t.scroll_offset(p).x;
    t.sample_scrolling(0.119).unwrap();
    assert!(t.scroll_state().needs_tick());
    t.sample_scrolling(1.0).unwrap();
    close(t.scroll_offset(p).x, x);
    close(t.scroll_offset(p).y, 100.0);
    assert!(y < 100.0);
    assert!(!t.scroll_state().needs_tick());
}
#[test]
fn overscroll_containment_is_per_axis_even_without_remaining_range() {
    for value in ["contain", "none"] {
        let (f, mut t) = page(
            &format!(
                "#p{{width:100px;height:100px;overflow:auto;overscroll-behavior-y:{value}}}#c{{width:300px;height:150px}}#tail{{width:600px;height:500px}}"
            ),
            "<div id=p><div id=c></div></div><div id=tail></div>",
        );
        let p = id(&f.document, "p");
        t.set_scroll_offset(p, ScrollOffset { x: 200.0, y: 50.0 });
        t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 30.0, y: 30.0 });
        close(t.viewport_scroll().y, 0.0);
        close(t.viewport_scroll().x, 30.0);
    }
    let (_f, mut t) = page(
        "#p{width:100px;height:100px;overflow:auto;overscroll-behavior:contain}#tail{height:500px}",
        "<div id=p></div><div id=tail></div>",
    );
    assert!(!t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 20.0 }));
    close(t.viewport_scroll().y, 0.0);
}
#[test]
fn styled_scrollbar_geometry_inverse_drag_none_and_root_overlay() {
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:auto;transform:scale(2);transform-origin:top left;scrollbar-width:thin;scrollbar-color:red blue}#c{height:400px}",
        "<div id=p><div id=c></div></div>",
    );
    let p = id(&f.document, "p");
    let bar = t
        .scrollbars()
        .into_iter()
        .find(|b| b.node_id() == p)
        .unwrap();
    close(bar.track.width, 3.0);
    assert_eq!(bar.thumb_color.to_srgb8().0, 0xff0000);
    assert_eq!(bar.track_color.to_srgb8().0, 0x0000ff);
    let r = bar.spatial.matrix.bounds(bar.thumb).unwrap();
    assert_eq!(t.scrollbar_at(r.x + 1.0, r.y + 1.0).unwrap().node_id(), p);
    let grab = t.scrollbar_grab(bar, r.x + 1.0, r.y + 1.0);
    assert!(t.drag_scrollbar(bar, r.x + 1.0, r.y + 120.0, grab));
    assert!(t.scroll_offset(p).y > 100.0);
    let (_f, mut t) = page(
        "html{scrollbar-color:red blue;scrollbar-width:thin}#tail{height:500px}",
        "<div id=tail></div>",
    );
    assert!(t.scrollbars().iter().any(|b| b.viewport));
    let bar = t.scrollbar_at(298.0, 50.0).unwrap();
    assert!(t.drag_scrollbar(bar, 298.0, 160.0, 0.0));
    assert!(t.viewport_scroll().y > 200.0);
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:auto;scrollbar-width:none}#c{height:400px}",
        "<div id=p><div id=c></div></div>",
    );
    let p = id(&f.document, "p");
    assert!(t.scrollbars().is_empty());
    t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 50.0 });
    close(t.scroll_offset(p).y, 50.0);
    let _ = f;
}
#[test]
fn fragments_decode_exact_id_before_legacy_name_top_target_and_nested_reveal() {
    let (mut f, _) = page(
        "#p{width:120px;height:100px;overflow:hidden;scroll-padding-top:10px}#spacer{height:180px}#é{height:30px;scroll-margin-top:5px}#tail{height:200px}#é:target{background:red}",
        "<a name='é' id=legacy></a><div id=p><div id=spacer></div><div id=é></div><div id=tail></div></div><div style='height:400px'></div>",
    );
    let node = id(&f.document, "é");
    let p = id(&f.document, "p");
    assert_eq!(
        f.fragment_target("%C3%A9"),
        Some(FragmentTarget::Node(node))
    );
    assert!(f.fragment_target_checked("%FF").is_err());
    assert!(f.fragment_target_checked("%Q1").is_err());
    assert_eq!(f.fragment_target(""), Some(FragmentTarget::Top));
    f.url.set_fragment(Some("%C3%A9"));
    f.refresh_target();
    let mut t = f.layout_with_container_queries(&FontContext::bundled());
    f.scroll_to_fragment(&mut t, ScrollBehavior::Instant)
        .unwrap();
    close(t.scroll_offset(p).y, 165.0);
    close(
        t.visual_node_rect(node, t.viewport_scroll().y).unwrap().y,
        5.0,
    );
    assert_eq!(f.interaction_state().target, Some(node));
    assert!(pixels(&t).contains(&0xff0000));
    let cloned = f.clone();
    assert_eq!(cloned.scroll_state(), f.scroll_state());
    let mut other = Frame::new(f.url.clone(), f.document.clone());
    assert!(!other.scroll_state().same_document(f.scroll_state()));
    let mut stale = other.layout_with_container_queries(&FontContext::bundled());
    let saved = stale.scroll_state().clone();
    assert!(
        f.scroll_to_fragment(&mut stale, ScrollBehavior::Instant)
            .is_err()
    );
    assert_eq!(stale.scroll_state(), &saved);
}
#[test]
fn fragment_display_contents_union_and_smooth_request_have_no_hit_visibility_dependency() {
    let (mut f, mut t) = page(
        "#p{width:100px;height:100px;overflow:hidden;scroll-behavior:smooth}#s{height:150px}#target{display:contents;scroll-margin-top:20px}#a{height:20px;background:red}#b{height:20px;background:blue}#end{height:200px}",
        "<div id=p><div id=s></div><div id=target><div id=a></div><div id=b></div></div><div id=end></div></div>",
    );
    let p = id(&f.document, "p");
    let node = id(&f.document, "target");
    assert!(t.visual_node_rect(node, 0.0).is_none());
    f.url.set_fragment(Some("target"));
    f.refresh_target();
    f.scroll_to_fragment(&mut t, ScrollBehavior::Auto).unwrap();
    close(t.scroll_offset(p).y, 0.0);
    assert!(t.scroll_state().has_animations());
    t.sample_scrolling(1.0).unwrap();
    close(t.scroll_offset(p).y, 130.0);
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:hidden}#wide{width:250px;height:20px}#end{height:300px;width:500px}",
        "<div id=p><div id=wide></div><div id=end></div></div>",
    );
    let p = id(&f.document, "p");
    let wide = id(&f.document, "wide");
    t.set_scroll_offset(p, ScrollOffset { x: 50.0, y: 0.0 });
    t.scroll_into_view(&f.document, wide, ScrollBehavior::Instant)
        .unwrap();
    close(t.scroll_offset(p).x, 50.0);
}
#[test]
fn sticky_logical_vertical_insets_use_the_negative_scroll_origin() {
    let (f, mut t) = page(
        "#p{writing-mode:vertical-rl;width:100px;height:100px;overflow:hidden}#cb{width:350px}#s{position:sticky;inset-block-start:6px;width:20px;height:80px;background:red}#tail{width:300px}",
        "<div id=p><div id=cb><div id=s></div><div id=tail></div></div></div>",
    );
    let p = id(&f.document, "p");
    let s = id(&f.document, "s");
    let normal = t.node_rect(s).unwrap();
    t.set_scroll_offset(p, ScrollOffset { x: -50.0, y: 0.0 });
    let r = t.visual_node_rect(s, 0.0).unwrap();
    let port = t.scroll_container(p).unwrap().port;
    close(r.x + r.width, port.x + port.width - 6.0);
    assert_eq!(normal, t.node_rect(s).unwrap());
}
#[test]
fn unreachable_clipped_fixed_and_nested_snap_targets_do_not_capture_parent() {
    let (f, mut t) = page(
        "#p{width:100px;height:100px;overflow:auto;scroll-snap-type:y mandatory}#clip{height:10px;overflow:clip}#hidden{position:relative;top:300px;height:20px;scroll-snap-align:start}#nested{height:50px;overflow:hidden}#inner{height:500px;scroll-snap-align:start}#fixed{position:fixed;top:250px;height:20px;scroll-snap-align:start}#tail{height:500px}",
        "<div id=p><div id=clip><div id=hidden></div></div><div id=nested><div id=inner></div></div><div id=fixed></div><div id=tail></div></div>",
    );
    let p = id(&f.document, "p");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 170.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    close(t.scroll_offset(p).y, 170.0);
}
#[test]
fn scroll_timeline_samples_the_smooth_offset_before_styles_and_sticky_paint() {
    let (mut f, mut t) = page(
        "@keyframes k{from{width:0px}to{width:100px}}#p{width:200px;height:100px;overflow:hidden;scroll-behavior:smooth}#cb{height:300px}#s{position:sticky;top:0;height:10px;background:red;animation:k linear both;animation-timeline:scroll(nearest block)}#tail{height:290px}",
        "<div id=p><div id=cb><div id=s></div><div id=tail></div></div></div>",
    );
    let p = id(&f.document, "p");
    let s = id(&f.document, "s");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 0.0, y: 200.0 },
        ScrollBehavior::Auto,
    )
    .unwrap();
    f.retain_scroll_state(&t);
    f.set_animation_time(0.14).unwrap();
    let t = f.layout_with_container_queries(&FontContext::bundled());
    close(t.scroll_offset(p).y, 100.0);
    close(t.node_rect(s).unwrap().width, 50.0);
    close(
        t.visual_node_rect(s, 0.0).unwrap().y,
        t.scroll_container(p).unwrap().port.y,
    );
    assert!(!f.animations().needs_tick());
    assert!(f.scroll_state().has_animations());
    f.set_animation_time(1.0).unwrap();
    f.layout_with_container_queries(&FontContext::bundled());
    assert!(!f.scroll_state().needs_tick());
}
#[test]
fn auto_view_timeline_insets_use_the_source_scroll_padding() {
    let (mut f, mut t) = page(
        "@keyframes k{from{width:0px}to{width:100px}}#p{width:200px;height:100px;overflow:hidden;scroll-padding-top:20px;scroll-padding-bottom:30px}#before{height:100px}#a{height:50px;animation:k linear both;animation-timeline:view(block)}#tail{height:200px}",
        "<div id=p><div id=before></div><div id=a></div><div id=tail></div></div>",
    );
    let p = id(&f.document, "p");
    let a = id(&f.document, "a");
    t.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 80.0 });
    f.retain_scroll_state(&t);
    let t = f.layout_with_container_queries(&FontContext::bundled());
    close(t.node_rect(a).unwrap().width, 50.0);
}
#[test]
fn two_axis_snapping_chooses_an_actual_joint_target_not_an_impossible_cross_pair() {
    let (f, mut t) = page(
        "#p{position:relative;width:100px;height:100px;overflow:hidden;scroll-snap-type:both mandatory}#tail{width:500px;height:500px}.item{position:absolute;width:20px;height:20px;scroll-snap-align:start}#a{left:100px;top:300px}#b{left:300px;top:100px}#c{left:200px;top:200px}",
        "<div id=p><div id=tail></div><div id=a class=item></div><div id=b class=item></div><div id=c class=item></div></div>",
    );
    let p = id(&f.document, "p");
    t.request_scroll(
        Some(p),
        ScrollOffset { x: 110.0, y: 110.0 },
        ScrollBehavior::Instant,
    )
    .unwrap();
    assert_eq!(t.scroll_offset(p), ScrollOffset { x: 200.0, y: 200.0 });
}
#[test]
fn wheel_reversal_uses_the_latest_direction_and_origin_without_trapping_at_start() {
    let (_f, mut t, p) = snap_page("");
    t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: 150.0 });
    t.scroll_wheel(40.0, 40.0, ScrollOffset { x: 0.0, y: -90.0 });
    close(t.scroll_offset(p).y, 60.0);
    t.finish_scroll_gesture();
    close(t.scroll_offset(p).y, 100.0);
}
