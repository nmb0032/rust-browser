use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint, style,
    text::FontContext,
};

fn node(d: &Document, id: &str) -> NodeId {
    d.preorder()
        .find(|&n| {
            d.node(n)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(id))
        })
        .unwrap()
}
fn box_for(b: &LayoutBox, id: NodeId) -> &LayoutBox {
    if b.dom_node_id() == Some(id) {
        return b;
    }
    b.children()
        .iter()
        .find_map(|c| {
            fn find(b: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
                if b.dom_node_id() == Some(id) {
                    return Some(b);
                }
                b.children().iter().find_map(|c| find(c, id))
            }
            find(c, id)
        })
        .unwrap()
}
fn scene(source: &str) -> (Document, LayoutTree) {
    let d = parser::parse(&format!(
        "<!doctype html><style>html,body{{margin:0}}{source}"
    ))
    .unwrap();
    let s = style::resolve(&d);
    assert!(
        s.stylesheets.diagnostics.is_empty(),
        "{:?}",
        s.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&d, &s.styles);
    tree.layout_viewport(200.0, 150.0, &FontContext::bundled());
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    (d, tree)
}
fn render(tree: &LayoutTree, scale: f64) -> Vec<u32> {
    let mut p = vec![0; (200.0 * scale) as usize * (150.0 * scale) as usize];
    let list = paint::build_display_list(tree, 200.0, 150.0, false);
    paint::rasterize_display_list(
        &list,
        &FontContext::bundled(),
        &mut p,
        (200.0 * scale) as usize,
        (150.0 * scale) as usize,
        scale,
        tree.viewport_scroll().y,
    );
    p
}

#[test]
fn post_layout_transform_keeps_flow_geometry_and_shares_inverse_hit_pixels() {
    let (d, t) = scene(
        "div{width:20px;height:20px}#a{background:red;transform:translate(50%,10px) scale(2);transform-origin:0 0}#b{background:blue}</style><div id=a></div><div id=b></div>",
    );
    let a = box_for(t.root(), node(&d, "a"));
    let b = box_for(t.root(), node(&d, "b"));
    assert_eq!(a.rect.width, 20.0);
    assert_eq!(b.rect.y, a.rect.y + 20.0);
    assert_eq!(
        t.hit_test(a.rect.x + 15.0, a.rect.y + 15.0),
        Some(node(&d, "a"))
    );
    assert_ne!(
        t.hit_test(a.rect.x + 1.0, a.rect.y + 1.0),
        Some(node(&d, "a"))
    );
    for scale in [1.0, 2.0] {
        let p = render(&t, scale);
        let index = ((a.rect.y + 15.0) * scale as f32) as usize * (200.0 * scale) as usize
            + ((a.rect.x + 15.0) * scale as f32) as usize;
        assert_eq!(p[index], 0xff0000);
    }
}

#[test]
fn grouped_opacity_is_once_after_overlap_and_creates_static_stacking_context() {
    let (d, t) = scene(
        "#g{position:absolute;left:10px;top:10px;width:50px;height:30px;opacity:.5}#a,#b{position:absolute;background:red;width:30px;height:30px}#b{left:20px;z-index:999}#cover{position:absolute;left:60px;top:10px;width:20px;height:20px;background:blue}</style><div id=g><div id=a></div><div id=b></div></div><div id=cover></div>",
    );
    let p = render(&t, 1.0);
    assert_eq!(p[20 * 200 + 20], 0xff7f7f);
    assert_eq!(
        p[20 * 200 + 35],
        0xff7f7f,
        "overlap must not receive opacity twice"
    );
    assert_eq!(p[20 * 200 + 65], 0x0000ff);
    assert_eq!(t.hit_test(65.0, 20.0), Some(node(&d, "cover")));
    let (_, nested) = scene(
        "#outer{position:absolute;left:10px;top:10px;width:30px;height:30px;opacity:.5}#inner{width:30px;height:30px;opacity:.5;background:red}</style><div id=outer><div id=inner></div></div>",
    );
    assert_eq!(render(&nested, 1.0)[20 * 200 + 20], 0xffbfbf);
}

#[test]
fn static_effects_capture_absolute_and_fixed_through_semantic_ancestry() {
    let (d, mut t) = scene(
        "#host{margin-left:40px;margin-top:30px;width:70px;height:60px;transform:translateX(10px);overflow:hidden}#contents{display:contents}#fixed,#abs{position:fixed;left:5px;top:5px;width:15px;height:15px;background:red}#abs{position:absolute;left:30px;background:blue}#tail{height:500px}</style><div id=host><div id=contents><div id=fixed></div><div id=abs></div></div></div><div id=tail></div>",
    );
    let host = box_for(t.root(), node(&d, "host"));
    let fixed = box_for(t.root(), node(&d, "fixed"));
    assert_eq!(fixed.coordinate_space(), CoordinateSpace::Document);
    assert_eq!(fixed.position_parent, Some(host.box_id));
    assert_eq!(fixed.rect.x, host.rect.x + 5.0);
    let (x, y) = (fixed.rect.x + 12.0, fixed.rect.y + 2.0);
    assert_eq!(t.hit_test(x, y), Some(node(&d, "fixed")));
    t.set_viewport_scroll(ScrollOffset { x: 0.0, y: 20.0 });
    assert_eq!(
        t.hit_test_viewport(x, y - 20.0, 20.0),
        Some(node(&d, "fixed"))
    );
}

#[test]
fn rotated_ancestor_overflow_uses_its_own_plane_not_child_axis_bounds() {
    let (d, mut t) = scene(
        "#host{position:absolute;left:30px;top:30px;width:40px;height:40px;overflow:hidden;transform:rotate(90deg);transform-origin:0 0}#child{width:80px;height:20px;background:red}</style><div id=host><div id=child></div></div>",
    );
    let host = box_for(t.root(), node(&d, "host"));
    let r = host.rect;
    assert_eq!(t.hit_test(r.x - 5.0, r.y + 10.0), Some(node(&d, "child")));
    assert_ne!(t.hit_test(r.x - 5.0, r.y + 60.0), Some(node(&d, "child")));
    let p = render(&t, 1.0);
    assert_eq!(p[(r.y as usize + 10) * 200 + r.x as usize - 5], 0xff0000);
    assert_eq!(p[(r.y as usize + 60) * 200 + r.x as usize - 5], 0xffffff);
    t.set_scroll_offset(node(&d, "host"), ScrollOffset { x: 30.0, y: 0.0 });
    assert_eq!(t.hit_test(r.x - 5.0, r.y + 10.0), Some(node(&d, "child")));
}

#[test]
fn perspective_separate_properties_singular_and_backface_use_plane_projection() {
    let (d, t) = scene(
        "#a{position:absolute;left:10px;top:10px;width:10px;height:10px;background:red;transform:perspective(100px) translateZ(50px);transform-origin:0 0}#b{position:absolute;left:80px;top:10px;width:20px;height:20px;background:blue;transform:scaleX(0)}#c{position:absolute;left:110px;top:10px;width:20px;height:20px;background:green;transform:rotateY(180deg);backface-visibility:hidden}</style><div id=a></div><div id=b></div><div id=c></div>",
    );
    assert_eq!(t.hit_test(25.0, 25.0), Some(node(&d, "a")));
    assert_ne!(t.hit_test(80.0, 20.0), Some(node(&d, "b")));
    assert_ne!(t.hit_test(120.0, 20.0), Some(node(&d, "c")));
    let p = render(&t, 1.0);
    assert_eq!(p[25 * 200 + 25], 0xff0000);
    assert_eq!(p[20 * 200 + 120], 0xffffff);
    let (_, t) = scene(
        "#a{position:absolute;left:10px;top:10px;width:10px;height:10px;background:red;translate:20px;rotate:90deg;scale:2;transform:translateX(5px);transform-origin:0 0}</style><div id=a></div>",
    );
    assert_eq!(render(&t, 1.0)[25 * 200 + 15], 0xff0000);
}

#[test]
fn shape_clipping_is_shared_with_input_masks_do_not_change_hit_testing() {
    let (d, t) = scene(
        "#a{position:absolute;left:10px;top:10px;width:40px;height:40px;background:red;clip-path:polygon(0 0,100% 0,100% 25%,25% 25%,25% 100%,0 100%)}#b{position:absolute;left:70px;top:10px;width:30px;height:30px;background:blue;mask-image:linear-gradient(transparent,transparent)}</style><div id=a></div><div id=b></div>",
    );
    assert_eq!(t.hit_test(15.0, 35.0), Some(node(&d, "a")));
    assert_ne!(t.hit_test(35.0, 35.0), Some(node(&d, "a")));
    assert_eq!(t.hit_test(80.0, 20.0), Some(node(&d, "b")));
    let p = render(&t, 1.0);
    assert_eq!(p[35 * 200 + 15], 0xff0000);
    assert_eq!(p[35 * 200 + 35], 0xffffff);
    assert_eq!(p[20 * 200 + 80], 0xffffff);
}

#[test]
fn mask_repeat_size_luminance_and_composite_have_actual_alpha() {
    let (_, t) = scene(
        "#a{position:absolute;left:10px;top:10px;width:40px;height:20px;background:red;mask:linear-gradient(to right,black 0%,black 50%,transparent 50%,transparent 100%) 0 0 / 20px 20px repeat alpha}#b{position:absolute;left:60px;top:10px;width:20px;height:20px;background:blue;mask-image:linear-gradient(red,red);mask-mode:luminance}</style><div id=a></div><div id=b></div>",
    );
    let p = render(&t, 1.0);
    for x in [15, 35] {
        assert_eq!(p[15 * 200 + x], 0xff0000);
    }
    for x in [25, 45] {
        assert_eq!(p[15 * 200 + x], 0xffffff);
    }
    assert_eq!(p[15 * 200 + 65], 0xc9c9ff);
    let (_, t) = scene(
        "#a{position:absolute;left:10px;top:10px;width:40px;height:20px;background:red;mask-image:linear-gradient(to right,black 0%,black 50%,transparent 50%,transparent 100%),linear-gradient(black,black);mask-composite:subtract}</style><div id=a></div>",
    );
    assert_eq!(render(&t, 1.0)[15 * 200 + 15], 0xffffff);
}

#[test]
fn backdrop_captures_prior_pixels_then_group_opacity_and_blends_nonisolated() {
    let (_, t) = scene(
        "#back{position:absolute;left:10px;top:10px;width:60px;height:40px;background:red}#front{position:absolute;left:20px;top:20px;width:20px;height:20px;backdrop-filter:invert(1);opacity:.5}#blend{position:absolute;left:45px;top:20px;width:15px;height:15px;background:blue;mix-blend-mode:multiply}</style><div id=back></div><div id=front></div><div id=blend></div>",
    );
    let p = render(&t, 1.0);
    assert_eq!(p[25 * 200 + 25], 0x7f8080);
    assert_eq!(p[25 * 200 + 50], 0x000000);
    assert_eq!(p[15 * 200 + 15], 0xff0000);
    let (_, t) = scene(
        "#back{position:absolute;left:10px;top:10px;width:60px;height:40px;background:red}#group{position:absolute;left:20px;top:20px;width:20px;height:20px;isolation:isolate}#blend{width:20px;height:20px;background:blue;mix-blend-mode:multiply}</style><div id=back></div><div id=group><div id=blend></div></div>",
    );
    assert_eq!(render(&t, 1.0)[25 * 200 + 25], 0x0000ff);
}

#[test]
fn colored_filter_shadow_and_transparency_expand_real_ink() {
    let (_, t) = scene(
        "#a{position:absolute;left:20px;top:20px;width:10px;height:10px;background:red;filter:drop-shadow(12px 0 0 blue) invert(1)}</style><div id=a></div>",
    );
    let p = render(&t, 1.0);
    assert_eq!(p[25 * 200 + 25], 0x00ffff);
    assert_eq!(p[25 * 200 + 37], 0xffff00);
    assert_eq!(p[25 * 200 + 31], 0xffffff);
    let (_, t) = scene(
        "#a{position:absolute;left:20px;top:20px;width:1px;height:1px;background:red;filter:blur(1px)}</style><div id=a></div>",
    );
    let p = render(&t, 1.0);
    assert_eq!(p[20 * 200 + 20], 0xffd6d6);
    assert_eq!(p[20 * 200 + 21], 0xffe6e6);
}

#[test]
fn transformed_text_preserves_immutable_shaping_offsets_and_source_hits() {
    let (d, t) = scene(
        "a{position:absolute;left:20px;top:20px;font-size:20px;transform:translate(30px,10px) scale(1.25);transform-origin:0 0}</style><a id=a href=x>ffi</a>",
    );
    let a = box_for(t.root(), node(&d, "a"));
    fn fragment(b: &LayoutBox) -> Option<&super::TextFragment> {
        b.text_fragments
            .first()
            .or_else(|| b.children.iter().find_map(fragment))
    }
    let f = fragment(a).unwrap();
    let before = f.glyphs.clone();
    let point = (
        f.ink_rect.x + f.ink_rect.width / 2.0,
        f.ink_rect.y + f.ink_rect.height / 2.0,
    );
    let matrix = a
        .style
        .effects
        .matrix(a.rect, a.content_rect, a.style.font_size);
    let (x, y) = matrix.project(point.0 as f64, point.1 as f64).unwrap();
    let hit = t.hit_test(x, y).unwrap();
    assert_eq!(d.node(hit).unwrap().parent(), Some(node(&d, "a")));
    for scale in [1.0, 1.5, 2.0] {
        assert!(render(&t, scale).iter().any(|&p| p != 0xffffff));
    }
    assert!(std::sync::Arc::ptr_eq(&before, &f.glyphs));
    assert_eq!(f.glyphs.glyphs.len(), 1);
}

#[test]
fn transformed_scrollbar_drag_wheel_and_rebuild_share_the_physical_plane() {
    let (d, mut t) = scene(
        "#port{position:absolute;left:90px;top:20px;width:40px;height:60px;overflow:auto;transform:rotate(90deg);transform-origin:0 0}#wide{width:100px;height:200px;background:red}</style><div id=port><div id=wide></div></div>",
    );
    let id = node(&d, "port");
    let bar = t
        .scrollbars()
        .into_iter()
        .find(|b| b.node_id() == id && b.vertical)
        .unwrap();
    let point = bar
        .spatial
        .matrix
        .project(
            (bar.thumb.x + bar.thumb.width / 2.0) as f64,
            (bar.thumb.y + bar.thumb.height / 2.0) as f64,
        )
        .unwrap();
    assert_eq!(t.scrollbar_at(point.0, point.1).unwrap().node_id(), id);
    let grab = t.scrollbar_grab(bar, point.0, point.1);
    let next = bar
        .spatial
        .matrix
        .project(
            (bar.thumb.x + bar.thumb.width / 2.0) as f64,
            (bar.track.y + bar.track.height + 20.0) as f64,
        )
        .unwrap();
    assert!(
        t.drag_scrollbar(bar, next.0, next.1, grab),
        "drag outside the track still inversely projects"
    );
    assert_eq!(
        t.scroll_offset(id).y,
        t.scroll_container(id).unwrap().range.max.y
    );
    t.set_scroll_offset(id, ScrollOffset::default());
    assert!(t.scroll_wheel(70.0, 30.0, ScrollOffset { x: 0.0, y: 25.0 }));
    assert_eq!(t.scroll_offset(id).y, 25.0);
    let state = t.scroll_state().clone();
    let range = t.scroll_container(id).unwrap();
    t.layout_viewport(200.0, 150.0, &FontContext::bundled());
    assert_eq!(t.scroll_state(), &state);
    assert_eq!(t.scroll_container(id).unwrap(), range);
}

#[test]
fn clip_path_applies_to_absolute_descendants_that_escape_static_overflow_lineage() {
    let (d, t) = scene(
        "#host{width:80px;height:80px;clip-path:inset(20px)}#a{position:absolute;left:0;top:0;width:150px;height:150px;background:red}</style><div id=host><div id=a></div></div>",
    );
    let r = box_for(t.root(), node(&d, "host")).rect;
    assert_eq!(t.hit_test(r.x + 30.0, r.y + 30.0), Some(node(&d, "a")));
    assert_ne!(t.hit_test(r.x + 5.0, r.y + 5.0), Some(node(&d, "a")));
    let p = render(&t, 1.0);
    assert_eq!(p[(r.y as usize + 30) * 200 + r.x as usize + 30], 0xff0000);
    assert_eq!(p[(r.y as usize + 5) * 200 + r.x as usize + 5], 0xffffff);
}
