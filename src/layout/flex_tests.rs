use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint::load_bundled_font,
};

fn layout(source: &str) -> (Document, LayoutTree) {
    let document = parser::parse(source).unwrap();
    let mut tree = build_layout_tree(&document);
    tree.layout(600.0, &load_bundled_font().unwrap());
    (document, tree)
}

fn id(document: &Document, name: &str) -> NodeId {
    document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

fn find(b: &LayoutBox, id: NodeId) -> &LayoutBox {
    if b.node_id == Some(id) {
        return b;
    }
    b.children.iter().find_map(|c| find_opt(c, id)).unwrap()
}

fn find_opt(b: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
    if b.node_id == Some(id) {
        Some(b)
    } else {
        b.children.iter().find_map(|c| find_opt(c, id))
    }
}

#[test]
fn flex_grow_and_scaled_shrink_are_not_equal_columns() {
    for (width, first, second) in [(400, 150.0, 250.0), (150, 50.0, 100.0)] {
        let (doc, tree) = layout(&format!(
            "<main style='display:flex;width:{width}px'><div id='a' style='flex:1 1 100px'></div><div id='b' style='flex:1 1 200px'></div></main>"
        ));
        assert_eq!(find(tree.root(), id(&doc, "a")).content_rect.width, first);
        assert_eq!(find(tree.root(), id(&doc, "b")).content_rect.width, second);
    }
}

fn scene(css: &str, contents: &str) -> (Document, LayoutTree) {
    layout(&format!(
        "<!doctype html><style>body{{margin:0}}#host{{display:flex;width:300px}}{css}</style><main id='host'>{contents}</main>"
    ))
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
}

fn box_for<'a>(doc: &Document, tree: &'a LayoutTree, name: &str) -> &'a LayoutBox {
    find(tree.root(), id(doc, name))
}

#[test]
fn min_max_violations_freeze_and_redistribute_until_resolved() {
    let (doc, tree) = scene(
        "#host{width:600px}div{flex:1 1 100px}#a{max-width:120px}#c{flex-grow:2}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    for (name, width) in [("a", 120.0), ("b", 193.33333), ("c", 286.66667)] {
        near(box_for(&doc, &tree, name).content_rect.width, width);
    }
    let (doc, tree) = scene(
        "#host{width:200px}div{flex:1 1 100px;min-width:0}#a{flex-basis:200px;min-width:150px}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    for (name, width) in [("a", 150.0), ("b", 25.0), ("c", 25.0)] {
        near(box_for(&doc, &tree, name).content_rect.width, width);
    }
}

#[test]
fn fractional_factor_sums_leave_free_space_or_overflow_instead_of_normalizing() {
    for (width, property, expected) in [(400, "flex-grow", 150.0), (100, "flex-shrink", 75.0)] {
        let (doc, tree) = scene(
            &format!("#host{{width:{width}px}}div{{flex-basis:100px;{property}:0.25;min-width:0}}"),
            "<div id='a'></div><div id='b'></div>",
        );
        near(box_for(&doc, &tree, "a").content_rect.width, expected);
        near(box_for(&doc, &tree, "b").content_rect.width, expected);
    }
    let (doc, tree) = scene(
        "#host{width:400px}div{flex:0.25 1 100px}#a{max-width:120px}",
        "<div id='a'></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 120.0);
    near(box_for(&doc, &tree, "b").content_rect.width, 150.0);
}

#[test]
fn shrink_uses_scaled_bases_and_freezes_zero_factors() {
    let (doc, tree) = scene(
        "#host{width:160px}#a{flex:0 2 100px}#b{flex:0 1 200px}#c{flex:0 0 60px}",
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 0.0);
    near(box_for(&doc, &tree, "b").content_rect.width, 100.0);
    near(box_for(&doc, &tree, "c").content_rect.width, 60.0);
}

#[test]
fn wrap_hypothetical_sizes_gaps_and_reversals_map_both_axes() {
    for (direction, rtl, wrap, expected) in [
        (
            "row",
            false,
            "wrap",
            [(0.0, 0.0), (120.0, 0.0), (0.0, 30.0)],
        ),
        (
            "row-reverse",
            false,
            "wrap",
            [(120.0, 0.0), (0.0, 0.0), (120.0, 30.0)],
        ),
        (
            "row",
            true,
            "wrap",
            [(120.0, 0.0), (0.0, 0.0), (120.0, 30.0)],
        ),
        (
            "row-reverse",
            true,
            "wrap-reverse",
            [(0.0, 80.0), (120.0, 80.0), (0.0, 50.0)],
        ),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:220px;height:100px;flex-direction:{direction};direction:{};flex-wrap:{wrap};gap:10px 20px;align-content:flex-start;align-items:flex-start}}div{{flex:none;width:100px;height:20px}}",
                if rtl { "rtl" } else { "ltr" }
            ),
            "<div id='a'></div><div id='b'></div><div id='c'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        for (name, (x, y)) in ["a", "b", "c"].into_iter().zip(expected) {
            let r = box_for(&doc, &tree, name).rect;
            near(r.x - host.x, x);
            near(r.y - host.y, y);
        }
    }
    let (doc, tree) = scene(
        "#host{width:220px;flex-wrap:wrap;gap:10px 20px}div{flex:none;width:100px;height:20px}",
        "<div></div><div></div><div></div>",
    );
    near(box_for(&doc, &tree, "host").content_rect.height, 50.0);
}

#[test]
fn column_wrap_column_reverse_and_rtl_cross_axis() {
    for (direction, rtl, expected) in [
        ("column", false, [(0.0, 0.0), (0.0, 120.0), (40.0, 0.0)]),
        (
            "column-reverse",
            false,
            [(0.0, 120.0), (0.0, 0.0), (40.0, 120.0)],
        ),
        ("column", true, [(80.0, 0.0), (80.0, 120.0), (40.0, 0.0)]),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:100px;height:220px;flex-flow:{direction} wrap;direction:{};gap:20px;align-items:flex-start;align-content:flex-start}}div{{flex:none;width:20px;height:100px}}",
                if rtl { "rtl" } else { "ltr" }
            ),
            "<div id='a'></div><div id='b'></div><div id='c'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        for (name, (x, y)) in ["a", "b", "c"].into_iter().zip(expected) {
            let r = box_for(&doc, &tree, name).rect;
            near(r.x - host.x, x);
            near(r.y - host.y, y);
        }
    }
}

#[test]
fn automatic_main_minimum_differs_from_zero_and_scrollable_overflow() {
    let word = "unbreakableunbreakableunbreakable";
    let mut intrinsic = 0.0;
    for (declaration, shrink) in [
        ("", false),
        ("min-width:0", true),
        ("overflow:auto", true),
        ("overflow:hidden", true),
        ("overflow:clip", false),
    ] {
        let (doc, tree) = scene(
            &format!("#host{{width:40px}}#a{{flex:1;{declaration}}}"),
            &format!("<div id='a'>{word}</div>"),
        );
        let a = box_for(&doc, &tree, "a");
        if shrink {
            near(a.content_rect.width, 40.0);
        } else {
            assert!(a.content_rect.width > 100.0);
            if intrinsic == 0.0 {
                intrinsic = a.content_rect.width;
            } else {
                near(a.content_rect.width, intrinsic);
            }
        }
    }
}

#[test]
fn percentages_use_definite_main_sizes_and_indefinite_column_basis_is_content() {
    let (doc, tree) = scene(
        "#host{gap:10%;width:300px}div{flex:0 0 40%;height:10px}",
        "<div id='a'></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 120.0);
    near(
        box_for(&doc, &tree, "b").rect.x - box_for(&doc, &tree, "a").rect.x,
        150.0,
    );
    let (doc, tree) = scene(
        "#host{flex-direction:column;row-gap:10%}div{flex:1 1 50%;height:20px}",
        "<div id='a'></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "host").content_rect.height, 40.0);
    near(box_for(&doc, &tree, "a").content_rect.height, 20.0);
    assert!(
        tree.diagnostics
            .iter()
            .any(|d| d.message.contains("indefinite main size"))
    );
    assert!(
        tree.diagnostics
            .iter()
            .any(|d| d.message.contains("cyclic percentage"))
    );
}

#[test]
fn decorations_margins_border_box_floor_and_min_max_are_inside_the_equations() {
    let (doc, tree) = scene(
        "#host{width:200px;gap:10px}div{flex:1 1 40px;padding:5px;border:5px solid;margin:0 5px;min-width:0}#b{box-sizing:border-box;max-width:60px}",
        "<div id='a'></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 90.0);
    near(box_for(&doc, &tree, "b").content_rect.width, 40.0);
    let (doc, tree) = scene(
        "#host{width:40px}div{flex:1 1 0px;box-sizing:border-box;padding:10px;min-width:0}",
        "<div id='a'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 20.0);
    near(box_for(&doc, &tree, "a").rect.width, 40.0);
}

#[test]
fn main_auto_margins_take_positive_free_space_before_justification() {
    for direction in ["row", "row-reverse"] {
        let (doc, tree) = scene(
            &format!(
                "#host{{flex-direction:{direction};justify-content:center}}div{{flex:none;width:50px;height:10px}}#b{{margin-left:auto}}"
            ),
            "<div id='a'></div><div id='b'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        let a = box_for(&doc, &tree, "a");
        let b = box_for(&doc, &tree, "b");
        near(b.margin.left, 200.0);
        near(
            a.rect.x - host.x,
            if direction == "row" { 0.0 } else { 250.0 },
        );
        near(
            b.rect.x - host.x,
            if direction == "row" { 250.0 } else { 200.0 },
        );
    }
}

#[test]
fn cross_auto_margins_stretch_min_max_and_descendant_percentages_reflow() {
    let (doc, tree) = scene(
        "#host{height:100px}div{flex:1}#a{margin-top:auto;margin-bottom:auto;height:20px}#b{max-height:60px}#inner{display:block;height:50%;width:100%}",
        "<div id='a'></div><div id='b'><span id='inner'></span></div>",
    );
    let host = box_for(&doc, &tree, "host");
    near(box_for(&doc, &tree, "a").rect.y - host.content_rect.y, 40.0);
    near(box_for(&doc, &tree, "b").content_rect.height, 60.0);
    near(box_for(&doc, &tree, "inner").content_rect.height, 30.0);
    near(box_for(&doc, &tree, "inner").content_rect.width, 150.0);
}

#[test]
fn line_distribution_and_negative_safe_justification() {
    for (align, expected) in [
        ("space-between", [0.0, 80.0]),
        ("space-around", [15.0, 65.0]),
        ("space-evenly", [20.0, 60.0]),
        ("stretch", [0.0, 50.0]),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:100px;height:100px;flex-wrap:wrap;align-content:{align};align-items:flex-start}}div{{flex:none;width:100px;height:20px}}"
            ),
            "<div id='a'></div><div id='b'></div>",
        );
        let host = box_for(&doc, &tree, "host").content_rect;
        for (name, y) in ["a", "b"].into_iter().zip(expected) {
            near(box_for(&doc, &tree, name).rect.y - host.y, y);
        }
    }
    for (justify, expected) in [
        ("center", -25.0),
        ("safe center", 0.0),
        ("space-between", 0.0),
        ("space-around", 0.0),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:100px;justify-content:{justify}}}div{{flex:none;width:150px;height:20px}}"
            ),
            "<div id='a'></div>",
        );
        near(
            box_for(&doc, &tree, "a").rect.x - box_for(&doc, &tree, "host").content_rect.x,
            expected,
        );
    }
}

#[test]
fn baseline_alignment_uses_shaped_line_metrics_and_image_bottom_fallback() {
    let (doc, tree) = scene(
        "#host{align-items:baseline}#a{font:10px/14px serif}#b{font:30px/40px serif}img{width:20px;height:20px}",
        "<div id='a'>small</div><div id='b'>A&#x6f22;</div><img id='c' alt=''>",
    );
    let a = box_for(&doc, &tree, "a");
    let b = box_for(&doc, &tree, "b");
    let c = box_for(&doc, &tree, "c");
    near(a.line_boxes[0].baseline, b.line_boxes[0].baseline);
    near(c.rect.y + c.rect.height, b.line_boxes[0].baseline);
    assert!(box_for(&doc, &tree, "host").rect.height >= 40.0);
}

#[test]
fn flex_box_generation_blockifies_each_element_and_only_wraps_text_sequences() {
    let (doc, tree) = scene(
        "#host:before{content:'before';order:-1}#host:after{content:'after'}span{order:2}",
        " \n <span id='a'>one<div id='block'>inside</div>tail</span> text <em id='b'>two</em> \n ",
    );
    let host = box_for(&doc, &tree, "host");
    assert_eq!(host.children.len(), 5);
    assert_eq!(
        host.children[0].pseudo,
        Some(crate::style::PseudoElement::Before)
    );
    assert_eq!(host.children[1].node_id, Some(id(&doc, "a")));
    assert_eq!(host.children[2].tag_name, "#anonymous-flex-item");
    assert_eq!(host.children[3].node_id, Some(id(&doc, "b")));
    assert_eq!(
        host.children[4].pseudo,
        Some(crate::style::PseudoElement::After)
    );
    assert_eq!(box_for(&doc, &tree, "a").kind, BoxKind::Block);
    assert!(
        box_for(&doc, &tree, "a")
            .children
            .iter()
            .any(|c| c.node_id == Some(id(&doc, "block")))
    );
    assert!(host.children.iter().all(|c| c.is_flex_item));
}

#[test]
fn flex_items_and_descendants_do_not_collapse_margins_across_the_flex_boundary() {
    let (doc, tree) = scene(
        "#host{flex-direction:column}div{height:10px;margin:10px 0}#inside{margin-top:15px;height:5px}",
        "<div id='a'><p id='inside'></p></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    let a = box_for(&doc, &tree, "a");
    let b = box_for(&doc, &tree, "b");
    near(a.rect.y - host.y, 10.0);
    near(b.rect.y - a.rect.y - a.rect.height, 20.0);
    near(
        box_for(&doc, &tree, "inside").rect.y - a.content_rect.y,
        15.0,
    );
}

#[test]
fn absolute_and_fixed_children_are_not_items_and_static_alignment_uses_the_final_size() {
    let (doc, tree) = scene(
        "#host{position:relative;height:100px;justify-content:center;align-items:center}#a{flex:1;height:20px}#abs,#fixed{position:absolute;width:40px;height:10px}#fixed{position:fixed}",
        "<div id='a'></div><div id='abs'></div><div id='fixed'></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "a").content_rect.width, 300.0);
    for name in ["abs", "fixed"] {
        let child = box_for(&doc, &tree, name);
        assert!(!child.is_flex_item);
        near(child.rect.x - host.x, 130.0);
        near(child.rect.y - host.y, 45.0);
    }
    assert_eq!(
        box_for(&doc, &tree, "fixed").coordinate_space,
        CoordinateSpace::Viewport
    );
}

#[test]
fn relative_translation_and_nested_flex_are_installed_only_once() {
    let (doc, mut tree) = scene(
        "#host{height:100px;align-items:center}#a{display:flex;flex:1;position:relative;left:10px;top:5px}#b{flex:1;height:20px}",
        "<div id='a'><div id='b'></div></div>",
    );
    let host = box_for(&doc, &tree, "host").content_rect;
    near(box_for(&doc, &tree, "a").rect.x - host.x, 10.0);
    near(box_for(&doc, &tree, "a").rect.y - host.y, 45.0);
    near(box_for(&doc, &tree, "b").rect.y - host.y, 45.0);
    near(box_for(&doc, &tree, "b").rect.x - host.x, 10.0);
    let stable = tree.clone();
    tree.layout(600.0, &load_bundled_font().unwrap());
    assert_eq!(tree, stable);
}

#[test]
fn inline_flex_is_atomic_shrink_to_fit_and_exports_a_baseline() {
    let (doc, tree) = layout(
        "<style>body{margin:0}p{width:220px;font:16px/20px serif;margin:0}#a{display:inline-flex;gap:10px;padding:2px}#a>span{flex:none;width:40px}</style><p>before <span id='a'><span id='b'>x</span><span id='c'>y</span></span> after</p>",
    );
    let a = box_for(&doc, &tree, "a");
    let b = box_for(&doc, &tree, "b");
    let c = box_for(&doc, &tree, "c");
    near(a.content_rect.width, 90.0);
    near(c.rect.x - b.rect.x, 50.0);
    assert_eq!(a.kind, BoxKind::Inline);
    assert!(a.inline_fragments.is_empty());
    let p = tree
        .root
        .children
        .iter()
        .flat_map(|b| &b.children)
        .flat_map(|b| &b.children)
        .find(|b| b.tag_name == "<p>")
        .unwrap();
    near(b.line_boxes[0].baseline, p.line_boxes[0].baseline);
    assert_eq!(
        tree.hit_test(b.rect.x + 1.0, b.rect.y + 1.0),
        Some(id(&doc, "b"))
    );
}

fn pixels(tree: &LayoutTree) -> Vec<u32> {
    let mut buffer = vec![0; 600 * 240];
    crate::paint::rasterize_display_list(
        &crate::paint::build_display_list(tree, 600.0, 240.0, false),
        &crate::text::FontContext::bundled(),
        &mut buffer,
        600,
        240,
        1.0,
        0.0,
    );
    buffer
}

#[test]
fn order_changes_layout_and_atomic_paint_but_not_source_identity_or_hit_order() {
    let (doc, tree) = scene(
        "#host{width:100px}div{flex:none;width:100px;height:100px;margin-right:-100px}#a{order:2;background:red}#b{order:1;background:blue}",
        "<div id='a'></div><div id='b'></div>",
    );
    let host = box_for(&doc, &tree, "host");
    assert_eq!(host.children[0].node_id, Some(id(&doc, "a")));
    assert_eq!(host.children[1].node_id, Some(id(&doc, "b")));
    let a = box_for(&doc, &tree, "a").rect;
    near(a.x, box_for(&doc, &tree, "b").rect.x);
    let (x, y) = ((a.x + 10.0) as usize, (a.y + 10.0) as usize);
    assert_eq!(pixels(&tree)[y * 600 + x], 0xff0000);
    assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, "a")));
}

#[test]
fn static_flex_integer_z_index_contains_descendants_while_auto_does_not() {
    for (z, color, hit) in [("0", 0x0000ff, "b"), ("auto", 0x008000, "child")] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:100px}}#a,#b{{flex:none;width:100px;height:100px}}#a{{margin-right:-100px;z-index:{z};background:red;position:relative}}#a{{position:static}}#b{{background:blue;z-index:1}}#child{{position:relative;width:100px;height:100px;background:green;z-index:100}}"
            ),
            "<div id='a'><div id='child'></div></div><div id='b'></div>",
        );
        let r = box_for(&doc, &tree, "a").rect;
        assert_eq!(
            box_for(&doc, &tree, "a").style.position,
            crate::style::Position::Static
        );
        let (x, y) = ((r.x + 10.0) as usize, (r.y + 10.0) as usize);
        assert_eq!(pixels(&tree)[y * 600 + x], color);
        assert_eq!(tree.hit_test(x as f32, y as f32), Some(id(&doc, hit)));
    }
}

#[test]
fn intrinsic_replaced_image_ratio_basis_and_shrink_zero_minimum() {
    use std::sync::Arc;
    let doc=parser::parse("<style>body{margin:0}main{display:flex;width:120px;align-items:flex-start}img{flex:1 1 auto;min-width:0}#b{height:20px}</style><main><img id='a'><img id='b'></main>").unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(80, 40, image::Rgba([255; 4]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let data = Arc::new(crate::media::DecodedImage::decode(png.get_ref()).unwrap());
    let styles = crate::style::resolve(&doc);
    let images = [(id(&doc, "a"), data.clone()), (id(&doc, "b"), data)]
        .into_iter()
        .collect();
    let mut tree = build_layout_tree_with_resources(&doc, &styles.styles, &images);
    tree.layout(600.0, &crate::text::FontContext::bundled());
    near(box_for(&doc, &tree, "a").content_rect.width, 80.0);
    near(box_for(&doc, &tree, "a").content_rect.height, 40.0);
    near(box_for(&doc, &tree, "b").content_rect.width, 40.0);
    near(box_for(&doc, &tree, "b").content_rect.height, 20.0);
}

#[test]
fn flex_pseudo_containers_empty_items_and_markers_keep_semantic_identity() {
    let (doc, tree) = scene(
        "#host:before{display:flex;content:'generated';flex:1}#empty{border:2px solid;flex:none}li{flex:1}",
        "<span id='empty'></span><li id='a'>item</li>",
    );
    let host = box_for(&doc, &tree, "host");
    assert_eq!(host.children.len(), 3);
    assert!(!host.children[0].children.is_empty());
    assert!(host.children[0].children[0].is_flex_item);
    near(box_for(&doc, &tree, "empty").rect.width, 4.0);
    assert!(box_for(&doc, &tree, "a").list_marker.is_some());
    near(
        box_for(&doc, &tree, "a")
            .list_marker
            .as_ref()
            .unwrap()
            .baseline,
        box_for(&doc, &tree, "a").line_boxes[0].baseline,
    );
}

#[test]
fn positioned_flex_shrink_to_fit_and_opposing_insets_format_real_items() {
    let (doc, tree) = layout(
        "<style>body{margin:0}main{position:relative;width:300px;height:200px}#host{display:flex;position:absolute;top:10px;gap:10px}#host>div{flex:none;width:40px;height:20px}#wide{display:flex;position:absolute;left:0;right:0;top:80px}#wide>div{flex:1;height:20px}</style><main><section id='host'><div id='a'></div><div></div></section><section id='wide'><div id='b'></div><div></div></section></main>",
    );
    near(box_for(&doc, &tree, "host").content_rect.width, 90.0);
    near(box_for(&doc, &tree, "a").content_rect.width, 40.0);
    near(box_for(&doc, &tree, "wide").content_rect.width, 300.0);
    near(box_for(&doc, &tree, "b").content_rect.width, 150.0);
}

#[test]
fn frame_flex_container_feedback_scroll_interaction_and_resize_keep_offsets_and_units() {
    use crate::{css::media::MediaEnvironment, engine::Frame, text::FontContext};
    let doc=parser::parse("<style>body{margin:0}main{display:flex;gap:20px}#panel{flex:1;min-width:0;height:80px;overflow:auto;container:panel / inline-size}#side{flex:0 0 100px}#child{height:240px;width:100%}#unit{width:50cqw;height:20px}#panel:hover{background:red}@container panel (width > 200px){#child{background:blue}}</style><main><section id='panel'><div id='child'><span id='unit' style='display:block'></span></div></section><div id='side'></div></main>").unwrap();
    let panel = id(&doc, "panel");
    let child = id(&doc, "child");
    let unit = id(&doc, "unit");
    let mut frame = Frame::new_with_environment(
        url::Url::parse("file:///flex.html").unwrap(),
        doc,
        MediaEnvironment::new(500.0, 240.0),
    );
    let mut tree = frame.layout_with_container_queries(&FontContext::bundled());
    near(find(tree.root(), panel).content_rect.width, 332.0);
    near(find(tree.root(), unit).content_rect.width, 166.0);
    assert_eq!(
        find(tree.root(), child).style.background_color,
        Some(0x0000ff)
    );
    assert!(tree.set_scroll_offset(panel, ScrollOffset { x: 0.0, y: 60.0 }));
    frame.retain_scroll_state(&tree);
    frame.set_media_environment(MediaEnvironment::new(400.0, 240.0));
    let small = frame.layout_with_container_queries(&FontContext::bundled());
    near(find(small.root(), panel).content_rect.width, 232.0);
    near(find(small.root(), unit).content_rect.width, 116.0);
    assert_eq!(small.scroll_offset(panel).y, 60.0);
    let interaction = crate::style::InteractionState {
        hovered: Some(panel),
        ..Default::default()
    };
    frame.set_interaction_state(interaction);
    let hovered = frame.layout_with_container_queries(&FontContext::bundled());
    assert_eq!(hovered.scroll_offset(panel).y, 60.0);
    assert_eq!(
        find(hovered.root(), panel).style.background_color,
        Some(0xff0000)
    );
    assert_eq!(
        hovered,
        frame.layout_with_container_queries(&FontContext::bundled())
    );
}

#[test]
fn logical_start_end_and_safe_alignment_are_independent_of_flex_reversal() {
    for (rtl, reverse, keyword, expected) in [
        (false, false, "start", 0.0),
        (false, true, "start", 0.0),
        (true, false, "start", 200.0),
        (true, true, "start", 200.0),
        (false, true, "end", 200.0),
        (true, true, "end", 0.0),
    ] {
        let (doc, tree) = scene(
            &format!(
                "#host{{direction:{};flex-direction:{};justify-content:{keyword}}}div{{flex:none;width:100px;height:10px}}",
                if rtl { "rtl" } else { "ltr" },
                if reverse { "row-reverse" } else { "row" }
            ),
            "<div id='a'></div>",
        );
        near(
            box_for(&doc, &tree, "a").rect.x - box_for(&doc, &tree, "host").content_rect.x,
            expected,
        );
    }
    let (doc, tree) = scene(
        "#host{width:100px;flex-direction:row-reverse;justify-content:safe center}div{flex:none;width:150px;height:10px}",
        "<div id='a'></div>",
    );
    near(
        box_for(&doc, &tree, "a").rect.x - box_for(&doc, &tree, "host").content_rect.x,
        0.0,
    );
}

#[test]
fn used_cross_size_does_not_make_indefinite_percentage_descendants_definite() {
    let (doc, tree) = scene(
        "#host{align-items:stretch}#a{flex:1}#b{height:60px;flex:1}#c{height:50%}",
        "<div id='a'><span id='c' style='display:block'></span></div><div id='b'></div>",
    );
    near(box_for(&doc, &tree, "a").content_rect.height, 60.0);
    near(box_for(&doc, &tree, "c").content_rect.height, 0.0);
    assert!(
        tree.diagnostics
            .iter()
            .any(|d| d.node_id == Some(id(&doc, "c")) && d.message.contains("percentage height"))
    );
}

#[test]
fn line_collection_is_exact_at_hypothetical_outer_size_thresholds() {
    for (width, y) in [(209, 10.0), (210, 0.0), (211, 0.0)] {
        let (doc, tree) = scene(
            &format!(
                "#host{{width:{width}px;flex-wrap:wrap;gap:10px}}div{{flex:1 1 100px;height:10px}}"
            ),
            "<div id='a'></div><div id='b'></div>",
        );
        near(
            box_for(&doc, &tree, "b").rect.y - box_for(&doc, &tree, "host").content_rect.y,
            if y == 0.0 { 0.0 } else { 20.0 },
        );
    }
}

#[test]
fn async_font_overlay_flex_geometry_relayout_retains_latest_scroll_state() {
    use crate::{css::media::MediaEnvironment, engine::Frame, text::FontContext};
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../assets/fonts/noto-sans/NotoSansItalic-fixture.ttf"
    ));
    let doc=parser::parse(&format!("<style>body{{margin:0}}@font-face{{font-family:Downloaded;src:url(data:font/ttf;base64,{data});font-style:italic}}main{{display:flex;width:300px}}#panel{{flex:1;min-width:0;overflow:auto;height:60px;font:italic 20px/24px Downloaded;container:panel / inline-size}}#child{{width:500px;height:300px}}#side{{flex:0 0 100px}}</style><main><div id='panel'><div id='child'>AVAVAVAVAV</div></div><div id='side'></div></main>")).unwrap();
    let panel = id(&doc, "panel");
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://flex.test/fonts").unwrap(),
        doc,
        MediaEnvironment::new(600.0, 240.0),
    );
    let mut tree = frame.layout_with_container_queries(&FontContext::bundled());
    tree.set_scroll_offset(panel, ScrollOffset { x: 35.0, y: 90.0 });
    frame.retain_scroll_state(&tree);
    let mut loading = frame.clone();
    tree.set_scroll_offset(panel, ScrollOffset { x: 45.0, y: 110.0 });
    frame.retain_scroll_state(&tree);
    loading
        .load_pending_web_fonts(|_| Ok(()), || false)
        .unwrap();
    loading.copy_scroll_state_from(&frame);
    let loaded = loading.layout_with_container_queries(&FontContext::bundled());
    assert_eq!(
        loaded.scroll_offset(panel),
        ScrollOffset { x: 45.0, y: 110.0 }
    );
    assert!(loading.styles.get(panel).unwrap().font_face.italic);
    near(find(loaded.root(), panel).content_rect.width, 200.0);
    assert_eq!(
        loaded,
        loading.layout_with_container_queries(&FontContext::bundled())
    );
}

#[test]
fn intrinsic_inline_flex_uses_flex_bases_and_declared_fractional_intrinsic_model() {
    for (grow, width, item_width) in [
        (0.0, 100.0, 100.0),
        (0.5, 150.0, 125.0),
        (1.0, 200.0, 200.0),
    ] {
        let (doc, tree) = layout(&format!(
            "<style>body{{margin:0}}#host{{display:inline-flex}}#a{{flex:{grow} 0 100px;min-width:0}}#inner{{width:200px;height:10px}}</style><p><span id='host'><span id='a'><span id='inner' style='display:block'></span></span></span></p>"
        ));
        near(box_for(&doc, &tree, "host").content_rect.width, width);
        near(box_for(&doc, &tree, "a").content_rect.width, item_width);
    }
}

#[test]
fn nested_flex_reuses_unchanged_item_measurements_and_local_coordinates() {
    let contents = (0..10).fold(
        "<div id='leaf' style='height:20px'></div>".to_owned(),
        |child, i| format!("<div id='n{i}' style='display:flex;flex:1;min-width:0'>{child}</div>"),
    );
    let (doc, tree) = scene("", &contents);
    near(box_for(&doc, &tree, "leaf").rect.height, 20.0);
    near(
        box_for(&doc, &tree, "leaf").rect.x,
        box_for(&doc, &tree, "host").content_rect.x,
    );
    near(
        box_for(&doc, &tree, "leaf").rect.y,
        box_for(&doc, &tree, "host").content_rect.y,
    );
}

#[test]
fn extreme_flex_values_have_finite_geometry_and_repeatable_fallback_diagnostics() {
    let (_doc, mut tree) = scene(
        "#host{width:3e38px;height:3e38px;gap:3e38px;align-items:center}div{flex:3e38 3e38 3e38px;padding:3e38px;margin:3e38px}",
        "<div></div><div></div>",
    );
    fn finite(b: &LayoutBox) {
        for r in [b.rect, b.content_rect] {
            assert!(
                [r.x, r.y, r.width, r.height]
                    .into_iter()
                    .all(f32::is_finite),
                "{r:?}"
            );
        }
        for child in &b.children {
            finite(child);
        }
    }
    finite(tree.root());
    assert!(!tree.diagnostics.is_empty());
    let stable = tree.clone();
    tree.layout(600.0, &crate::text::FontContext::bundled());
    assert_eq!(tree, stable);
}

#[test]
fn flex_fixture_pixels_match_card_rows_gaps_and_navigation_background() {
    use crate::{paint, text::FontContext};
    let document = parser::parse(include_str!("../../tests/pages/flexbox.html")).unwrap();
    let mut tree = build_layout_tree(&document);
    let font = FontContext::bundled();
    tree.layout_viewport(800.0, 1000.0, &font);
    let mut buffer = vec![0; 800 * 1000];
    paint::rasterize_display_list(
        &paint::build_display_list(&tree, 800.0, 1000.0, false),
        &font,
        &mut buffer,
        800,
        1000,
        1.0,
        0.0,
    );
    let cards: Vec<_> = document
        .preorder()
        .filter(|&id| {
            document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.tag_name() == "article")
        })
        .map(|id| tree.node_rect(id).unwrap())
        .collect();
    assert_eq!(cards.len(), 3);
    for (card, width) in cards.iter().zip([222.66667, 262.66667, 182.66667]) {
        assert!((card.width - width).abs() < 0.01);
        assert!((card.y - cards[0].y).abs() < 0.01);
        let (x, y) = ((card.x + 4.0) as usize, (card.y + 4.0) as usize);
        assert_eq!(buffer[y * 800 + x], 0xffffff);
    }
    let gap = (cards[0].x + cards[0].width + 8.0) as usize;
    assert_eq!(buffer[(cards[0].y + 4.0) as usize * 800 + gap], 0xfaf8f4);
    assert_eq!(buffer[30 * 800 + 30], 0x18212c);
}

#[test]
fn atomic_inline_flex_paints_in_source_line_order_and_ignores_static_z_index() {
    for positioned in [false, true] {
        let (doc, tree) = layout(&format!(
            "<style>body{{margin:0}}p{{margin:0;width:300px}}#a{{display:inline-flex;width:100px;height:40px;background:red;z-index:100}}#b{{background:blue;padding:10px 40px;{}}}</style><p><span id='a'><span></span></span><span id='b'>X</span></p>",
            if positioned {
                "position:relative;left:-100px"
            } else {
                "margin-left:-100px"
            }
        ));
        let a = box_for(&doc, &tree, "a");
        let b = box_for(&doc, &tree, "b");
        let r = b.inline_fragments[0].rect;
        let x = r.x + 2.0;
        let y = r.y + 2.0;
        assert!(x >= a.rect.x && x < a.rect.x + a.rect.width);
        assert!(y >= a.rect.y && y < a.rect.y + a.rect.height);
        assert_eq!(pixels(&tree)[y as usize * 600 + x as usize], 0x0000ff);
        assert_eq!(tree.hit_test(x, y), Some(id(&doc, "b")));
    }
}

#[test]
fn percentage_bases_in_content_sized_inline_and_positioned_flex_use_content() {
    for display in ["display:inline-flex", "display:flex;position:absolute"] {
        let (doc, tree) = layout(&format!(
            "<style>body{{margin:0}}#host{{{display}}}#a,#b{{flex:0 0 50%;min-width:0}}i{{display:block}}#a>i{{width:100px;height:10px}}#b>i{{width:200px;height:10px}}</style><p><span id='host'><span id='a'><i></i></span><span id='b'><i></i></span></span></p>"
        ));
        near(box_for(&doc, &tree, "host").content_rect.width, 300.0);
        near(box_for(&doc, &tree, "a").content_rect.width, 100.0);
        near(box_for(&doc, &tree, "b").content_rect.width, 200.0);
        assert!(
            tree.diagnostics
                .iter()
                .any(|d| d.message.contains("indefinite main size"))
        );
    }
}

#[test]
fn definite_inline_flex_width_resolves_percentage_basis_and_stretched_height() {
    let (doc, tree) = layout(
        "<style>body{margin:0}#host{display:inline-flex;width:300px;height:80px}#a,#b{flex:0 0 50%;min-width:0}#c{height:50%}</style><p><span id='host'><span id='a'><span id='c' style='display:block'></span></span><span id='b'></span></span></p>",
    );
    near(box_for(&doc, &tree, "a").content_rect.width, 150.0);
    near(box_for(&doc, &tree, "c").content_rect.height, 40.0);
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
}
