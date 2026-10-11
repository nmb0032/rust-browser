use super::*;
use crate::{
    dom::{Document, NodeId},
    html::parser,
    paint::{self, load_bundled_font},
    style,
};

fn id(document: &Document, name: &str) -> NodeId {
    document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.attribute("id") == Some(name))
        })
        .unwrap()
}

fn box_for<'a>(document: &Document, tree: &'a LayoutTree, name: &str) -> &'a LayoutBox {
    fn find(layout: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if layout.dom_node_id() == Some(node) && layout.pseudo_element().is_none() {
            return Some(layout);
        }
        layout.children().iter().find_map(|child| find(child, node))
    }
    find(tree.root(), id(document, name)).unwrap()
}

fn layout(css: &str, contents: &str) -> (Document, LayoutTree) {
    let document = parser::parse(&format!(
        "<!doctype html><style>body{{margin:0}}\
         #host{{width:200px;padding:1px 0}}p{{margin:0;font:16px/20px serif}}\
         {css}</style><main id='host'>{contents}</main>"
    ))
    .unwrap();
    let styles = style::resolve(&document);
    assert!(
        styles.stylesheets.diagnostics.is_empty(),
        "{:?}",
        styles.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&document, &styles.styles);
    tree.layout(448.0, &load_bundled_font().unwrap());
    (document, tree)
}

fn gap(document: &Document, tree: &LayoutTree, first: &str, second: &str) -> f32 {
    let first = box_for(document, tree, first).rect();
    box_for(document, tree, second).rect().y - first.y - first.height
}

#[test]
fn paragraph_gaps_collapse_for_every_sign_combination_without_changing_used_margins() {
    for (bottom, top, expected) in [
        (20, 30, 30),
        (30, 20, 30),
        (0, 30, 30),
        (20, 0, 20),
        (-20, -30, -30),
        (-30, -20, -30),
        (20, -30, -10),
        (-20, 30, 10),
        (0, 0, 0),
    ] {
        let (document, tree) = layout(
            &format!("#a{{margin-bottom:{bottom}px}}#b{{margin-top:{top}px}}"),
            "<p id='a'>First</p><p id='b'>Second</p>",
        );
        assert_eq!(gap(&document, &tree, "a", "b"), expected as f32);
        assert_eq!(
            box_for(&document, &tree, "a").used_margins().bottom.0,
            bottom as f32
        );
        assert_eq!(
            box_for(&document, &tree, "b").used_margins().top.0,
            top as f32
        );
        assert!(tree.diagnostics().is_empty());
    }
}

#[test]
fn sibling_decorations_do_not_separate_their_external_margins() {
    let (document, tree) = layout(
        "#a{margin-bottom:20px;padding:3px;border:2px solid}\
         #b{margin-top:30px;padding:5px;border:1px solid}",
        "<p id='a'>First</p><p id='b'>Last</p>",
    );
    assert_eq!(gap(&document, &tree, "a", "b"), 30.0);
    assert_eq!(box_for(&document, &tree, "a").rect().height, 30.0);
    assert_eq!(box_for(&document, &tree, "b").rect().height, 32.0);
}

#[test]
fn nested_empty_chains_keep_all_extrema_and_have_css_through_box_positions() {
    for contents in [
        "<p id='a'>First</p><div id='e'><div id='f'></div></div><p id='b'>Last</p>",
        "<p id='a'>First</p><div id='e'></div><div id='f'></div><p id='b'>Last</p>",
    ] {
        let (document, tree) = layout(
            "#a{margin-bottom:20px}#e{margin-top:-10px;margin-bottom:50px}\
             #f{margin-top:-30px;margin-bottom:40px}#b{margin-top:15px}",
            contents,
        );
        assert_eq!(gap(&document, &tree, "a", "b"), 20.0);
        let a = box_for(&document, &tree, "a").rect();
        let e = box_for(&document, &tree, "e").rect();
        let f = box_for(&document, &tree, "f").rect();
        assert_eq!(e.height, 0.0);
        assert_eq!(f.height, 0.0);
        if contents.contains("<div id='e'><div") {
            // e's top set includes f's entire through-strut, but not e's
            // own bottom. f's margins adjoin its parent's top.
            assert_eq!(e.y, a.y + a.height + 10.0);
            assert_eq!(f.y, e.y);
        } else {
            assert_eq!(e.y, a.y + a.height + 10.0);
            assert_eq!(f.y, a.y + a.height + 20.0);
        }
    }
    let nested = (0..40).fold(String::new(), |child, index| {
        format!(
            "<div style='margin-top:{}px;margin-bottom:{}px'>{child}</div>",
            -index,
            index * 2
        )
    });
    let (document, tree) = layout(
        "#a{margin-bottom:20px}#b{margin-top:-10px}",
        &format!("<p id='a'>First</p>{nested}<p id='b'>Last</p>"),
    );
    assert_eq!(gap(&document, &tree, "a", "b"), 39.0);
    assert!(tree.diagnostics().is_empty());
}

#[test]
fn parent_edges_propagate_recursively_without_counting_external_margins_in_height() {
    let (document, tree) = layout(
        "#parent{margin:10px 0 15px}#middle{margin:20px 0 25px}\
         #child{height:12px;margin:30px 0 35px}",
        "<section id='parent'><article id='middle'><div id='child'></div></article></section>",
    );
    let host = box_for(&document, &tree, "host");
    for name in ["parent", "middle", "child"] {
        let child = box_for(&document, &tree, name);
        assert_eq!(child.rect().y, host.content_rect().y + 30.0);
        assert_eq!(child.content_rect().height, 12.0);
    }
    assert_eq!(host.content_rect().height, 30.0 + 12.0 + 35.0);
}

#[test]
fn parent_edge_groups_include_empty_siblings_before_and_after_nonempty_descendants() {
    let (document, tree) = layout(
        "#a{margin-bottom:20px}#parent{margin:10px 0 15px}\
         #leading{margin:-10px 0 50px}#child{height:12px;margin:40px 0 30px}\
         #trailing{margin:-25px 0 60px}#b{margin-top:10px}",
        "<p id='a'>First</p><section id='parent'><div id='leading'></div>\
         <div id='child'></div><div id='trailing'></div></section><p id='b'>Last</p>",
    );
    let parent = box_for(&document, &tree, "parent");
    let leading = box_for(&document, &tree, "leading");
    let child = box_for(&document, &tree, "child");
    let trailing = box_for(&document, &tree, "trailing");
    assert_eq!(gap(&document, &tree, "a", "parent"), 40.0);
    assert_eq!(leading.rect().y, parent.rect().y);
    assert_eq!(child.rect().y, parent.rect().y);
    assert_eq!(parent.content_rect().height, 12.0);
    assert_eq!(trailing.rect().y, child.rect().y + 12.0 + 5.0);
    assert_eq!(gap(&document, &tree, "parent", "b"), 35.0);
}

#[test]
fn all_empty_descendants_escape_only_the_unblocked_edge_and_keep_noninfluencing_positions() {
    for (decoration, y_offset, parent_height, host_height) in [
        ("padding-top:1px", 10.0, 0.0, 31.0),
        ("padding-bottom:1px", 20.0, 0.0, 36.0),
        ("padding:1px 0", 10.0, 20.0, 47.0),
    ] {
        let (document, tree) = layout(
            &format!(
                "#parent{{margin:10px 0 15px;{decoration}}}\
                 #empty{{margin:30px 0 -10px}}"
            ),
            "<section id='parent'><div id='empty'></div></section>",
        );
        let host = box_for(&document, &tree, "host");
        let parent = box_for(&document, &tree, "parent");
        let empty = box_for(&document, &tree, "empty");
        assert_eq!(parent.rect().y - host.content_rect().y, y_offset);
        assert_eq!(parent.content_rect().height, parent_height);
        assert_eq!(host.content_rect().height, host_height);
        assert_eq!(
            empty.rect().y - parent.content_rect().y,
            if decoration == "padding-bottom:1px" {
                0.0
            } else {
                30.0
            }
        );
    }
}

#[test]
fn padding_and_borders_block_only_the_decorated_parent_edge() {
    for (declaration, top, bottom, decoration) in [
        ("padding-top:3px", false, true, 3.0),
        ("border-top:3px solid", false, true, 3.0),
        ("padding-bottom:3px", true, false, 3.0),
        ("border-bottom:3px solid", true, false, 3.0),
        ("padding:3px 0", false, false, 6.0),
        (
            "border-top:1px solid;border-bottom:2px solid",
            false,
            false,
            3.0,
        ),
        ("padding:0;border:0 solid", true, true, 0.0),
    ] {
        let (document, tree) = layout(
            &format!(
                "#parent{{margin:10px 0 15px;{declaration}}}\
                 #child{{height:12px;margin:30px 0 35px}}"
            ),
            "<section id='parent'><div id='child'></div></section>",
        );
        let parent = box_for(&document, &tree, "parent");
        let child = box_for(&document, &tree, "child");
        let host = box_for(&document, &tree, "host");
        assert_eq!(
            parent.rect().y - host.content_rect().y,
            if top { 30.0 } else { 10.0 },
            "{declaration}"
        );
        assert_eq!(
            child.rect().y - parent.content_rect().y,
            if top { 0.0 } else { 30.0 },
            "{declaration}"
        );
        let height = 12.0 + if top { 0.0 } else { 30.0 } + if bottom { 0.0 } else { 35.0 };
        assert_eq!(parent.content_rect().height, height, "{declaration}");
        assert_eq!(parent.rect().height, height + decoration);
        assert_eq!(
            host.content_rect().height,
            if top { 30.0 } else { 10.0 } + height + decoration + if bottom { 35.0 } else { 15.0 }
        );
    }
}

#[test]
fn auto_zero_positive_and_percentage_heights_distinguish_through_and_bottom_collapse() {
    for (height, expected_gap, parent_height, diagnostics) in [
        ("auto", 40.0, 0.0, 0),
        ("0", 40.0, 0.0, 0),
        ("1px", 81.0, 1.0, 0),
        ("calc(0px)", 40.0, 0.0, 0),
        ("50%", 40.0, 0.0, 1),
    ] {
        let (document, tree) = layout(
            &format!(
                "#a{{margin-bottom:20px}}#parent{{height:{height};margin:30px 0 40px}}\
                 #child{{margin:50px 0 -10px}}#b{{margin-top:15px}}"
            ),
            "<p id='a'>First</p><section id='parent'><div id='child'></div></section>\
             <p id='b'>Last</p>",
        );
        assert_eq!(gap(&document, &tree, "a", "b"), expected_gap, "{height}");
        assert_eq!(
            box_for(&document, &tree, "parent").content_rect().height,
            parent_height
        );
        assert_eq!(tree.diagnostics().len(), diagnostics);
    }
    for (height, expected_parent, expected_host) in [
        ("auto", 12.0, 77.0),
        ("0", 0.0, 45.0),
        ("60px", 60.0, 105.0),
    ] {
        let (document, tree) = layout(
            &format!(
                "#parent{{height:{height};margin:10px 0 15px}}\
                 #child{{height:12px;margin:30px 0 35px}}"
            ),
            "<section id='parent'><div id='child'></div></section>",
        );
        assert_eq!(
            box_for(&document, &tree, "parent").content_rect().height,
            expected_parent
        );
        assert_eq!(
            box_for(&document, &tree, "host").content_rect().height,
            expected_host
        );
    }
}

#[test]
fn root_and_containment_stop_child_collapse_but_containment_external_margins_adjoin() {
    for (containment, height) in [("inline-size", 77.0), ("size", 0.0)] {
        let (document, tree) = layout(
            &format!(
                "#a{{margin-bottom:20px}}#parent{{container-type:{containment};margin:10px 0 15px}}\
                 #child{{height:12px;margin:30px 0 35px}}#b{{margin-top:25px}}"
            ),
            "<p id='a'>First</p><section id='parent'><div id='child'></div></section>\
             <p id='b'>Last</p>",
        );
        let parent = box_for(&document, &tree, "parent");
        assert_eq!(gap(&document, &tree, "a", "parent"), 20.0);
        assert_eq!(gap(&document, &tree, "parent", "b"), 25.0);
        assert_eq!(parent.content_rect().height, height);
        assert_eq!(
            box_for(&document, &tree, "child").rect().y - parent.content_rect().y,
            30.0
        );
    }
    let (document, tree) = layout(
        "html{margin:10px 0 15px}body{margin:30px 0 35px}#host{padding:0;height:12px}",
        "",
    );
    let html = tree
        .root()
        .children()
        .iter()
        .find(|child| child.tag_name() == "<html>")
        .unwrap();
    let body = html
        .children()
        .iter()
        .find(|child| child.tag_name() == "<body>")
        .unwrap();
    assert_eq!(html.rect().y, 34.0);
    assert_eq!(body.rect().y, 64.0);
    assert_eq!(html.content_rect().height, 77.0);
    assert_eq!(box_for(&document, &tree, "host").rect().height, 12.0);
    assert_eq!(tree.root().rect().height, 150.0);
}

#[test]
fn containment_keeps_its_children_sibling_collapsing_including_empty_chains() {
    let (document, tree) = layout(
        "#parent{container-type:inline-size;margin:10px 0 15px}\
         #a{margin:30px 0 20px}#empty{margin:-10px 0 50px}\
         #b{margin:15px 0 35px}",
        "<section id='parent'><p id='a'>First</p><div id='empty'></div><p id='b'>Last</p></section>",
    );
    let parent = box_for(&document, &tree, "parent");
    assert_eq!(gap(&document, &tree, "a", "b"), 40.0);
    assert_eq!(
        parent.content_rect().height,
        30.0 + 20.0 + 40.0 + 20.0 + 35.0
    );
    assert_eq!(
        box_for(&document, &tree, "a").rect().y - parent.content_rect().y,
        30.0
    );
}

#[test]
fn real_inline_text_breaks_and_replaced_content_separate_but_phantom_boxes_do_not() {
    for (content, through, expected_height) in [
        ("", true, 0.0),
        (" \t\n", true, 0.0),
        ("<span></span>", true, 0.0),
        ("<span style='margin:30px 0'></span>", true, 0.0),
        ("Text", false, 20.0),
        ("<span style='line-height:0'>Text</span>", false, 20.0),
        ("<br>", false, 40.0),
        ("<img style='width:0;height:0'>", false, 20.0),
        ("<span style='padding-top:1px'></span>", false, 20.0),
        ("<span style='padding-left:1px'></span>", false, 20.0),
        (
            "<span style='padding:0 1px;margin:0 -1px'></span>",
            false,
            20.0,
        ),
    ] {
        let (document, tree) = layout(
            "#a{margin-bottom:10px}#parent{font:16px/20px serif;margin:30px 0 40px}\
             #b{margin-top:20px}",
            &format!(
                "<p id='a'>First</p><section id='parent'>{content}</section><p id='b'>Last</p>"
            ),
        );
        assert_eq!(
            gap(&document, &tree, "a", "b"),
            if through {
                40.0
            } else {
                70.0 + expected_height
            },
            "{content}"
        );
        assert_eq!(
            box_for(&document, &tree, "parent").rect().height,
            expected_height,
            "{content}"
        );
    }
    // A genuine zero-height text line remains a separator.
    let (document, tree) = layout(
        "#a{margin-bottom:10px}#parent{font:16px/0 serif;margin:30px 0 40px}#b{margin-top:20px}",
        "<p id='a'>First</p><section id='parent'>Text</section><p id='b'>Last</p>",
    );
    assert_eq!(gap(&document, &tree, "a", "b"), 70.0);
}

#[test]
fn block_images_even_zero_sized_do_not_collapse_through() {
    let (document, tree) = layout(
        "#a{margin-bottom:10px}#image{display:block;width:0;height:0;margin:30px 0 40px}\
         #b{margin-top:20px}",
        "<p id='a'>First</p><img id='image'><p id='b'>Last</p>",
    );
    assert_eq!(box_for(&document, &tree, "image").rect().height, 0.0);
    assert_eq!(gap(&document, &tree, "a", "b"), 70.0);
}

#[test]
fn empty_inline_continuations_and_anonymous_boxes_collapse_through_only_when_phantom() {
    for (inline_style, gap_size) in [("", 40.0), ("padding:0 1px", 110.0)] {
        let (document, tree) = layout(
            &format!(
                "#host{{font:16px/20px serif}}em{{{inline_style}}}#a{{margin-bottom:10px}}\
                 #block{{display:block;height:0;margin:30px 0 40px}}#b{{margin-top:20px}}"
            ),
            "<p id='a'>First</p><em><span id='block'></span></em><p id='b'>Last</p>",
        );
        assert_eq!(gap(&document, &tree, "a", "b"), gap_size, "{inline_style}");
        let host = box_for(&document, &tree, "host");
        assert!(host.children()[1].is_anonymous());
        assert!(host.children()[3].is_anonymous());
    }
}

#[test]
fn inline_runs_block_only_the_parent_edge_that_they_touch() {
    for (contents, top, bottom, height) in [
        ("Leading<div id='child'></div>", 10.0, 35.0, 62.0),
        ("<div id='child'></div>Trailing", 30.0, 15.0, 67.0),
        ("Leading<div id='child'></div>Trailing", 10.0, 15.0, 117.0),
    ] {
        let (document, tree) = layout(
            "#host{font:16px/20px serif}#parent{margin:10px 0 15px}\
             #child{height:12px;margin:30px 0 35px}",
            &format!("<section id='parent'>{contents}</section>"),
        );
        let parent = box_for(&document, &tree, "parent");
        let host = box_for(&document, &tree, "host");
        assert_eq!(parent.rect().y - host.content_rect().y, top);
        assert_eq!(parent.content_rect().height, height);
        assert_eq!(host.content_rect().height, top + height + bottom);
    }
}

#[test]
fn outside_list_markers_do_not_prevent_empty_items_collapsing_through_and_pseudos_do() {
    for (pseudo, expected_gap) in [
        ("", 40.0),
        ("#empty::before{content:''}", 40.0),
        (
            "#empty::before{display:block;content:'';margin:50px 0 -10px}",
            40.0,
        ),
        (
            "#empty::before{display:block;content:'Text';margin:50px 0 -10px}",
            100.0,
        ),
    ] {
        let (document, tree) = layout(
            &format!(
                "#host{{font:16px/20px serif}}#a{{margin-bottom:10px}}\
                 #empty{{display:list-item;margin:30px 0 40px}}\
                 #empty::marker{{content:'!'}}#b{{margin-top:20px}}{pseudo}"
            ),
            "<p id='a'>First</p><div id='empty'></div><p id='b'>Last</p>",
        );
        assert_eq!(gap(&document, &tree, "a", "b"), expected_gap, "{pseudo}");
        let empty = box_for(&document, &tree, "empty");
        assert_eq!(empty.list_marker().unwrap().text, "!");
        assert_eq!(empty.list_marker().unwrap().y, empty.content_rect().y);
    }
}

#[test]
fn negative_overlap_affects_auto_height_without_flooring_each_flow_step() {
    for (css, expected_height) in [
        ("#b{margin-top:-30px}", 10.0),
        ("#b{margin-top:-60px}", 0.0),
        ("#b{margin-top:-30px;margin-bottom:-5px}", 5.0),
    ] {
        let (document, tree) = layout(css, "<p id='a'>First</p><p id='b'>Second</p>");
        assert_eq!(
            box_for(&document, &tree, "host").content_rect().height,
            expected_height
        );
        assert_eq!(
            gap(&document, &tree, "a", "b"),
            if css.contains("-60px") { -60.0 } else { -30.0 }
        );
        if !css.contains("-60px") {
            let b = box_for(&document, &tree, "b").rect();
            assert_eq!(
                tree.hit_test(b.x + b.width - 1.0, b.y + b.height - 1.0),
                Some(id(&document, "b")),
                "the later overlapping sibling remains the hit target"
            );
        }
    }
    let (document, tree) = layout(
        "#parent{margin-bottom:15px}#a{height:20px}#b{height:20px;margin-top:-30px;margin-bottom:-25px}",
        "<section id='parent'><div id='a'></div><div id='b'></div></section>",
    );
    assert_eq!(
        box_for(&document, &tree, "parent").content_rect().height,
        10.0
    );
    // The escaped bottom set is 15 - 25 = -10, not a prematurely
    // clamped cursor. Only the final used content height is nonnegative.
    assert_eq!(box_for(&document, &tree, "host").content_rect().height, 0.0);
}

#[test]
fn percent_math_and_auto_margins_resolve_once_and_relayout_is_stable() {
    let (document, mut tree) = layout(
        "#host{width:50%}#a{margin-bottom:calc(10% + 10px)}\
         #empty{margin:clamp(10px, 20%, 100px) auto -5%}#b{margin-top:auto}",
        "<p id='a'>First</p><div id='empty'></div><p id='b'>Second</p>",
    );
    let computed = box_for(&document, &tree, "empty").style().clone();
    for (viewport, expected_gap, top, bottom) in [
        (448.0, 30.0, 40.0, -10.0),
        (848.0, 60.0, 80.0, -20.0),
        (448.0, 30.0, 40.0, -10.0),
    ] {
        tree.layout(viewport, &load_bundled_font().unwrap());
        assert_eq!(gap(&document, &tree, "a", "b"), expected_gap);
        let empty = box_for(&document, &tree, "empty");
        assert_eq!(empty.used_margins().top.0, top);
        assert_eq!(empty.used_margins().bottom.0, bottom);
        assert_eq!(empty.style(), &computed);
        assert_eq!(box_for(&document, &tree, "b").used_margins().top.0, 0.0);
        let stable = tree.clone();
        tree.layout(viewport, &load_bundled_font().unwrap());
        assert_eq!(tree, stable);
    }
}

#[test]
fn invalid_vertical_margin_math_reports_once_per_resolution_and_not_per_collapse_edge() {
    let (_, mut tree) = layout(
        "#parent{margin-top:calc(1px / (1 - 100% / 200px));margin-bottom:20px}\
         #empty{margin:30px 0 -10px}",
        "<section id='parent'><div id='empty'></div></section>",
    );
    assert_eq!(tree.diagnostics().len(), 1);
    assert!(
        tree.diagnostics()[0]
            .message
            .contains("margin: invalid at used-value time")
    );
    let stable = tree.clone();
    tree.layout(448.0, &load_bundled_font().unwrap());
    assert_eq!(tree, stable);
}

#[test]
fn finalized_geometry_consumers_follow_collapsed_blocks_text_images_and_markers() {
    let (document, tree) = layout(
        "#a{margin-bottom:30px}#parent{margin-top:20px;background:blue}\
         #item{display:list-item;height:20px;margin-top:40px;background:red}\
         #item::before{display:block;content:'Before';margin:10px 0}\
         #item::marker{content:'!'}#image{display:block;width:30px;height:10px;margin-top:15px}",
        "<p id='a'>First</p><section id='parent'><div id='item'>Text</div>\
         <img id='image' alt='Alt'></section>",
    );
    let parent = box_for(&document, &tree, "parent");
    let item = box_for(&document, &tree, "item");
    assert_eq!(gap(&document, &tree, "a", "parent"), 40.0);
    assert_eq!(item.rect().y, parent.rect().y);
    let marker = item.list_marker().unwrap();
    assert_eq!(marker.text, "!");
    assert_eq!(marker.y, item.content_rect().y);
    let pseudo = &item.children()[0];
    assert_eq!(pseudo.pseudo_element(), Some(style::PseudoElement::Before));
    assert_eq!(pseudo.rect().y, item.rect().y);
    assert_eq!(pseudo.children()[0].text_fragments()[0].y, pseudo.rect().y);
    let image = box_for(&document, &tree, "image");
    assert_eq!(gap(&document, &tree, "item", "image"), 15.0);
    assert_eq!(image.text_fragments()[0].y, image.content_rect().y);
    assert_eq!(tree.node_rect(id(&document, "image")), Some(image.rect()));
    assert_eq!(
        tree.hit_test(image.rect().x + 1.0, image.rect().y + 1.0),
        Some(id(&document, "image"))
    );
    let list = paint::build_display_list(&tree, 448.0, 300.0, true);
    assert!(list.iter().any(|command| matches!(
        command,
        paint::DisplayItem::Rect { x, y, w, h, color: 0xff0000 }
            if (*x, *y, *w, *h) == (item.rect().x, item.rect().y, item.rect().width, item.rect().height)
    )));
}

#[test]
fn decoded_images_paint_at_final_collapsed_coordinates_without_collapsing_through() {
    use crate::media::{DecodedImage, ImageMap};
    use std::sync::Arc;

    let (document, _) = layout(
        "#a{margin-bottom:30px}#parent{margin:10px 0 15px}\
         #image{display:block;margin:40px auto 35px}#b{margin-top:20px}",
        "<p id='a'>First</p><section id='parent'><img id='image'></section><p id='b'>Last</p>",
    );
    let styles = style::resolve(&document);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(80, 40)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let decoded = Arc::new(DecodedImage::decode(bytes.get_ref()).unwrap());
    let images = ImageMap::from([(id(&document, "image"), decoded.clone())]);
    let mut tree = build_layout_tree_with_resources(&document, &styles.styles, &images);
    tree.layout(448.0, &load_bundled_font().unwrap());
    let parent = box_for(&document, &tree, "parent");
    let image = box_for(&document, &tree, "image");
    assert_eq!(gap(&document, &tree, "a", "parent"), 40.0);
    assert_eq!(image.rect().y, parent.rect().y);
    assert_eq!(parent.rect().height, 40.0);
    assert_eq!(gap(&document, &tree, "parent", "b"), 35.0);
    assert_eq!(image.used_margins().left.0, 60.0);
    let list = paint::build_display_list(&tree, 448.0, 300.0, false);
    assert!(list.iter().any(|command| matches!(
        command,
        paint::DisplayItem::Image { x, y, w, h, image: data }
            if (*x, *y, *w, *h) == (image.rect().x, image.rect().y, 80.0, 40.0)
                && Arc::ptr_eq(data, &decoded)
    )));
    assert_eq!(tree.node_rect(id(&document, "image")), Some(image.rect()));
    assert_eq!(
        tree.hit_test(image.rect().x + 1.0, image.rect().y + 1.0),
        Some(id(&document, "image"))
    );
}

#[test]
fn extreme_margins_remain_finite_with_linear_diagnostics_and_repeatable_layout() {
    fn assert_finite(layout: &LayoutBox) -> usize {
        assert!(layout.rect().x.is_finite() && layout.rect().y.is_finite());
        assert!(layout.rect().width.is_finite() && layout.rect().height.is_finite());
        assert!(layout.content_rect().y.is_finite());
        for fragment in layout.text_fragments() {
            assert!(fragment.x.is_finite() && fragment.y.is_finite());
        }
        1 + layout.children().iter().map(assert_finite).sum::<usize>()
    }
    let contents = (0..20).fold("<p id='b'>Last</p>".to_string(), |child, _| {
        format!("<div style='margin:3e38px 0;padding-top:1px'>{child}</div>")
    });
    let (_, mut tree) = layout(
        "#a{margin:3e38px 0}",
        &format!("<p id='a'>First</p>{contents}"),
    );
    let nodes = assert_finite(tree.root());
    assert!(!tree.diagnostics().is_empty());
    assert!(tree.diagnostics().len() <= 6 * nodes);
    let stable = tree.clone();
    tree.layout(448.0, &load_bundled_font().unwrap());
    assert_eq!(tree, stable);
    let (document, tree) = layout(
        "#a{margin-bottom:3e38px}#empty{margin:-3e38px 0 2e38px}\
         #b{margin-top:1e38px}",
        "<p id='a'>First</p><div id='empty'></div><p id='b'>Last</p>",
    );
    // Collapse in f64 before placement: large opposite extrema cancel exactly.
    assert_eq!(gap(&document, &tree, "a", "b"), 0.0);
    assert!(tree.diagnostics().is_empty());
}
