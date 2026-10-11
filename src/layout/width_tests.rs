use super::*;
use crate::{
    css::values::{LengthOrAuto, UsedLength},
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

fn box_for(tree: &LayoutTree, node: NodeId) -> &LayoutBox {
    fn find(layout: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if layout.dom_node_id() == Some(node) && layout.pseudo_element().is_none() {
            return Some(layout);
        }
        layout.children().iter().find_map(|child| find(child, node))
    }
    find(tree.root(), node).unwrap()
}

fn layout(source: &str, viewport: f32) -> (Document, LayoutTree) {
    let document = parser::parse(source).unwrap();
    let styles = style::resolve(&document);
    assert!(
        styles.stylesheets.diagnostics.is_empty(),
        "{:?}",
        styles.stylesheets.diagnostics
    );
    let mut tree = build_layout_tree_with_styles(&document, &styles.styles);
    tree.layout(viewport, &load_bundled_font().unwrap());
    (document, tree)
}

fn assert_equation(child: &LayoutBox, parent: &LayoutBox, width: f32, left: f32, right: f32) {
    let margins = child.used_margins();
    assert_eq!(child.content_rect().width, width);
    assert_eq!(margins.left, UsedLength(left));
    assert_eq!(margins.right, UsedLength(right));
    assert_eq!(child.rect().x, parent.content_rect().x + left);
    assert_eq!(
        left + child.rect().width + right,
        parent.content_rect().width
    );
}

#[test]
fn all_normal_block_width_equation_branches_and_containing_direction() {
    // The border and padding contribute 20px to every equation.
    for (width, margins, ltr, rtl) in [
        (
            "40px",
            "10px 15px",
            (40.0, 15.0, 125.0),
            (40.0, 125.0, 15.0),
        ),
        ("40px", "0 auto", (40.0, 70.0, 70.0), (40.0, 70.0, 70.0)),
        (
            "40px",
            "0 15px 0 auto",
            (40.0, 125.0, 15.0),
            (40.0, 125.0, 15.0),
        ),
        (
            "40px",
            "0 auto 0 15px",
            (40.0, 15.0, 125.0),
            (40.0, 15.0, 125.0),
        ),
        ("180px", "0 auto", (180.0, 0.0, 0.0), (180.0, 0.0, 0.0)),
        ("220px", "0 auto", (220.0, 0.0, -40.0), (220.0, -40.0, 0.0)),
        (
            "180px",
            "0 30px 0 auto",
            (180.0, 0.0, 0.0),
            (180.0, -30.0, 30.0),
        ),
        (
            "180px",
            "0 auto 0 30px",
            (180.0, 30.0, -30.0),
            (180.0, 0.0, 0.0),
        ),
        (
            "220px",
            "0 10px 0 15px",
            (220.0, 15.0, -55.0),
            (220.0, -50.0, 10.0),
        ),
        (
            "40px",
            "0 -10px 0 auto",
            (40.0, 150.0, -10.0),
            (40.0, 150.0, -10.0),
        ),
        (
            "40px",
            "0 auto 0 -10px",
            (40.0, -10.0, 150.0),
            (40.0, -10.0, 150.0),
        ),
        ("auto", "0 auto", (180.0, 0.0, 0.0), (180.0, 0.0, 0.0)),
        (
            "auto",
            "0 30px 0 auto",
            (150.0, 0.0, 30.0),
            (150.0, 0.0, 30.0),
        ),
        (
            "auto",
            "0 auto 0 30px",
            (150.0, 30.0, 0.0),
            (150.0, 30.0, 0.0),
        ),
        (
            "auto",
            "0 10px 0 15px",
            (155.0, 15.0, 10.0),
            (155.0, 15.0, 10.0),
        ),
        (
            "auto",
            "0 -10px 0 -15px",
            (205.0, -15.0, -10.0),
            (205.0, -15.0, -10.0),
        ),
        (
            "auto",
            "0 150px 0 150px",
            (0.0, 150.0, 30.0),
            (0.0, 30.0, 150.0),
        ),
        ("-0px", "auto", (0.0, 90.0, 90.0), (0.0, 90.0, 90.0)),
        ("calc(-20px)", "auto", (0.0, 90.0, 90.0), (0.0, 90.0, 90.0)),
    ] {
        for (direction, (used_width, left, right)) in [("ltr", ltr), ("rtl", rtl)] {
            let opposite = if direction == "ltr" { "rtl" } else { "ltr" };
            let (document, tree) = layout(
                &format!(
                    "<!doctype html><style>body{{margin:0}}\
                #parent{{width:200px;direction:{direction}}}\
                #target{{width:{width};margin:{margins};padding:0 8px;border:2px solid;\
                direction:{opposite}}}</style>\
                <section id='parent'><div id='target'></div></section>"
                ),
                400.0,
            );
            let parent = box_for(&tree, id(&document, "parent"));
            let child = box_for(&tree, id(&document, "target"));
            assert_equation(child, parent, used_width, left, right);
            assert!(
                tree.diagnostics().is_empty(),
                "{direction}: {width}; {margins}"
            );
        }
    }
}

#[test]
fn zero_containing_width_and_decoration_overflow_use_nonnegative_content_width() {
    for (width, margins, expected) in [
        ("auto", "auto", (0.0, 0.0, -20.0)),
        ("0", "auto", (0.0, 0.0, -20.0)),
        ("10px", "auto", (10.0, 0.0, -30.0)),
        ("auto", "0 -30px", (40.0, -30.0, -30.0)),
        ("50%", "0", (0.0, 0.0, -20.0)),
    ] {
        let (document, tree) = layout(
            &format!(
                "<!doctype html><style>body{{margin:0}}\
            #parent{{width:0}} #target{{width:{width};margin:{margins};padding:0 10px}}\
            </style><section id='parent'><div id='target'></div></section>"
            ),
            0.0,
        );
        assert_equation(
            box_for(&tree, id(&document, "target")),
            box_for(&tree, id(&document, "parent")),
            expected.0,
            expected.1,
            expected.2,
        );
        assert!(tree.diagnostics().is_empty());
    }
}

#[test]
fn percentages_math_box_sizing_and_relayout_recompute_used_not_computed_margins() {
    for (sizing, expected_width, expected_left, border_width) in [
        ("content-box", 100.0, 32.0, 136.0),
        ("border-box", 64.0, 50.0, 100.0),
    ] {
        let (document, mut tree) = layout(
            &format!(
                "<!doctype html><style>body{{margin:0}}\
            #target{{box-sizing:{sizing};width:calc(50%);margin:auto;\
            padding:5px calc(5% + 5px);border:3px solid}}</style>\
            <div id='target'></div>"
            ),
            248.0,
        );
        let node = id(&document, "target");
        let child = box_for(&tree, node);
        assert_eq!(child.content_rect().width, expected_width);
        assert_eq!(child.rect().width, border_width);
        assert_eq!(child.rect().x, 24.0 + expected_left);
        assert_eq!(child.used_margins().left, UsedLength(expected_left));
        assert_eq!(child.used_margins().right, UsedLength(expected_left));
        assert_eq!(child.used_margins().top, UsedLength(0.0));
        assert_eq!(child.used_margins().bottom, UsedLength(0.0));
        let computed = child.style().clone();
        tree.layout(448.0, &load_bundled_font().unwrap());
        let resized = box_for(&tree, node);
        let (content_width, left) = if sizing == "content-box" {
            (200.0, 72.0)
        } else {
            (144.0, 100.0)
        };
        assert_eq!(resized.content_rect().width, content_width);
        assert_eq!(resized.used_margins().left, UsedLength(left));
        assert_eq!(resized.style(), &computed);
        assert_eq!(resized.style().margin.left, LengthOrAuto::Auto);
        let stable = tree.clone();
        tree.layout(448.0, &load_bundled_font().unwrap());
        assert_eq!(tree, stable);
    }
    let (document, tree) = layout(
        "<!doctype html><style>body{margin:0} #parent{width:200px}\
        #target{width:clamp(20px, 50%, 300px);margin:5% -10% calc(2% + 1px) auto}</style>\
        <section id='parent'><div id='target'></div></section>",
        400.0,
    );
    let child = box_for(&tree, id(&document, "target"));
    assert_equation(
        child,
        box_for(&tree, id(&document, "parent")),
        100.0,
        120.0,
        -20.0,
    );
    assert_eq!(child.used_margins().top, UsedLength(10.0));
    assert_eq!(child.used_margins().bottom, UsedLength(5.0));
}

#[test]
fn border_box_floor_still_balances_margins_and_list_items_use_block_equation() {
    let (document, tree) = layout(
        "<!doctype html><style>body{margin:0} section{width:200px}\
        #target{display:list-item;width:1px;box-sizing:border-box;\
        padding:0 10px;border:2px solid;margin:auto;height:10px}</style>\
        <section id='parent'><div id='target'></div></section>",
        400.0,
    );
    let child = box_for(&tree, id(&document, "target"));
    assert_equation(
        child,
        box_for(&tree, id(&document, "parent")),
        0.0,
        88.0,
        88.0,
    );
    assert!(child.list_marker().is_some());
}

#[test]
fn supported_replaced_images_resolve_width_before_margins_but_inline_auto_is_zero() {
    use crate::media::{DecodedImage, ImageMap};
    use std::sync::Arc;
    let document = parser::parse(
        "<!doctype html><style>body{margin:0}\
        section{width:200px} img{margin:auto;padding:0 8px;border:2px solid}\
        #block{display:block} #height{display:block;height:20px}\
        #fixed{display:block;width:100px;box-sizing:border-box}\
        #overflow{display:block;width:240px}\
        </style><section id='parent'><img id='block'><img id='height'>\
        <img id='fixed'><img id='overflow'><img id='inline'></section>",
    )
    .unwrap();
    let styles = style::resolve(&document);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(80, 40)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let image = Arc::new(DecodedImage::decode(bytes.get_ref()).unwrap());
    let images: ImageMap = ["block", "height", "fixed", "overflow", "inline"]
        .into_iter()
        .map(|name| (id(&document, name), image.clone()))
        .collect();
    let mut tree = build_layout_tree_with_resources(&document, &styles.styles, &images);
    tree.layout(400.0, &load_bundled_font().unwrap());
    let parent = box_for(&tree, id(&document, "parent"));
    for (name, width, height, left, right) in [
        ("block", 80.0, 40.0, 50.0, 50.0),
        ("height", 40.0, 20.0, 70.0, 70.0),
        ("fixed", 80.0, 40.0, 50.0, 50.0),
        ("overflow", 240.0, 120.0, 0.0, -60.0),
    ] {
        let child = box_for(&tree, id(&document, name));
        assert_equation(child, parent, width, left, right);
        assert_eq!(child.content_rect().height, height);
    }
    let inline = box_for(&tree, id(&document, "inline"));
    assert_eq!(inline.used_margins().left, UsedLength(0.0));
    assert_eq!(inline.used_margins().right, UsedLength(0.0));
    assert_eq!(inline.content_rect().width, 80.0);
    assert_eq!(inline.rect().x, parent.content_rect().x);
    assert!(tree.diagnostics().is_empty());
}

#[test]
fn replaced_percentage_math_fallback_and_rtl_overconstraint_share_block_equations() {
    for direction in ["ltr", "rtl"] {
        let (document, tree) = layout(
            &format!(
                "<!doctype html><style>body{{margin:0}}\
            section{{width:200px;direction:{direction}}}\
            img{{display:block;margin:auto;padding:0 8px;border:2px solid}}\
            #percent{{width:50%;box-sizing:border-box}}\
            #math{{width:calc(25% + 10px);margin:0 -10% 0 auto}}\
            #overflow{{width:240px;direction:ltr}}\
            </style><section id='parent'><img id='fallback'>\
            <img id='percent'><img id='math'><img id='overflow'></section>"
            ),
            400.0,
        );
        let parent = box_for(&tree, id(&document, "parent"));
        for (name, width, left, right) in [
            ("fallback", 16.0, 82.0, 82.0),
            ("percent", 80.0, 50.0, 50.0),
            ("math", 60.0, 140.0, -20.0),
            (
                "overflow",
                240.0,
                if direction == "ltr" { 0.0 } else { -60.0 },
                if direction == "ltr" { -60.0 } else { 0.0 },
            ),
        ] {
            let image = box_for(&tree, id(&document, name));
            assert_equation(image, parent, width, left, right);
            assert_eq!(image.used_margins().top, UsedLength(0.0));
            assert_eq!(image.used_margins().bottom, UsedLength(0.0));
        }
        assert!(tree.diagnostics().is_empty());
    }
}

#[test]
fn used_centering_is_fractional_until_actual_geometry_quantization() {
    let (document, tree) = layout(
        "<!doctype html><style>body{margin:0} section{width:201px}\
        #target{width:50%;margin:0 auto}</style>\
        <section id='parent'><div id='target'></div></section>",
        400.0,
    );
    let child = box_for(&tree, id(&document, "target"));
    assert_equation(
        child,
        box_for(&tree, id(&document, "parent")),
        100.5,
        50.25,
        50.25,
    );
    assert_eq!(child.rect().x, 74.25);
    assert_eq!(child.actual_rect(1.0).unwrap().left, 74);
    assert_eq!(child.actual_rect(1.0).unwrap().right, 175);
}

#[test]
fn centered_anonymous_runs_inline_continuations_and_pseudos_keep_correct_geometry() {
    let (document, tree) = layout(
        "<!doctype html><style>body{margin:0} section{width:200px;margin:auto}\
        em{margin:auto;padding:0 5px} #block{display:block;width:80px;margin:auto}\
        section::before{display:block;content:'Generated';width:100px;margin:auto}</style>\
        <section id='parent'>Leading<em id='inline'>Before<span id='block'>Block</span>\
        After</em>Trailing</section>",
        448.0,
    );
    let parent = box_for(&tree, id(&document, "parent"));
    assert_eq!(parent.rect().x, 124.0);
    assert_eq!(parent.children().len(), 4);
    let pseudo = &parent.children()[0];
    assert_eq!(pseudo.rect().x, 174.0);
    for child in [&parent.children()[1], &parent.children()[3]] {
        assert!(child.is_anonymous());
        assert_eq!(child.content_rect().width, 200.0);
        assert_eq!(child.rect().x, 124.0);
        assert_eq!(child.used_margins().left, UsedLength(0.0));
        let inline = child
            .children()
            .iter()
            .find(|child| child.tag_name() == "<em>")
            .unwrap();
        assert_eq!(inline.used_margins().left, UsedLength(0.0));
        assert_eq!(inline.used_margins().right, UsedLength(0.0));
        assert_eq!(inline.style().margin.left, LengthOrAuto::Auto);
    }
    let block = box_for(&tree, id(&document, "block"));
    assert_equation(block, parent, 80.0, 60.0, 60.0);
    assert!(tree.node_rect(id(&document, "inline")).is_some());
}

#[test]
fn centered_geometry_drives_paint_hits_actual_values_and_resize() {
    let (document, mut tree) = layout(
        "<!doctype html><style>body{margin:0}\
        #target{width:100px;height:40px;margin:auto;padding:10px;\
        border:2px solid blue;background:red}</style><div id='target'></div>",
        448.0,
    );
    let node = id(&document, "target");
    let child = box_for(&tree, node);
    assert_eq!(
        child.rect(),
        Rect {
            x: 162.0,
            y: 24.0,
            width: 124.0,
            height: 64.0
        }
    );
    assert_eq!(tree.node_rect(node), Some(child.rect()));
    assert_eq!(tree.hit_test(170.0, 40.0), Some(node));
    assert_ne!(tree.hit_test(160.0, 40.0), Some(node));
    let list = paint::build_display_list(&tree, 448.0, 160.0, false);
    let mut pixels = vec![0xffffff; 448 * 160];
    paint::rasterize_display_list(
        &list,
        &load_bundled_font().unwrap(),
        &mut pixels,
        448,
        160,
        1.0,
        0.0,
    );
    assert_eq!(pixels[40 * 448 + 170], 0xff0000);
    assert_eq!(pixels[40 * 448 + 162], 0x0000ff);
    assert_eq!(pixels[40 * 448 + 160], 0xffffff);
    let values = crate::devtools::format_values(&tree, 1.25);
    assert!(values.contains("specified Margin(Left): Auto"));
    assert!(values.contains("left: UsedLength(138.0)"));
    assert!(values.contains("computed margins: Edges { top: Auto"));
    assert_eq!(child.actual_rect(1.25).unwrap().left, 202);
    tree.layout(248.0, &load_bundled_font().unwrap());
    assert_eq!(box_for(&tree, node).rect().x, 62.0);
    assert_eq!(box_for(&tree, node).used_margins().left, UsedLength(38.0));
    assert_ne!(tree.hit_test(270.0, 40.0), Some(node));
}

#[test]
fn math_and_arithmetic_failures_report_diagnostics_and_never_store_infinity() {
    let (document, mut tree) = layout(
        "<!doctype html><style>body{margin:0}\
        #target{width:calc(1px / (1 - 100% / 200px));margin:0 auto}</style>\
        <div id='target'></div>",
        248.0,
    );
    assert!(
        box_for(&tree, id(&document, "target"))
            .used_width()
            .0
            .is_finite()
    );
    let child = box_for(&tree, id(&document, "target"));
    assert_eq!(child.used_width(), UsedLength(0.0));
    assert_eq!(child.used_margins().left, UsedLength(100.0));
    assert_eq!(child.used_margins().right, UsedLength(100.0));
    assert!(child.style().width.is_some());
    assert!(
        tree.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("width:"))
    );
    tree.layout(f32::INFINITY, &load_bundled_font().unwrap());
    assert!(tree.root().rect().width.is_finite());
    assert!(
        tree.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("viewport-width"))
    );
    let (document, tree) = layout(
        "<!doctype html><style>body{margin:0}\
        #target{width:auto;margin:0 -3e38px 0 -3e38px}</style><div id='target'></div>",
        400.0,
    );
    let child = box_for(&tree, id(&document, "target"));
    assert!(child.used_width().0.is_finite());
    assert!(child.rect().x.is_finite());
    assert!(
        tree.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("width equation"))
    );
}

#[test]
fn invalid_used_margin_math_falls_back_only_at_layout_and_still_solves_auto_margin() {
    let (document, tree) = layout(
        "<!doctype html><style>body{margin:0}\
        #target{width:100px;margin:0 auto 0 calc(1px / (1 - 100% / 200px))}</style>\
        <div id='target'></div>",
        248.0,
    );
    let child = box_for(&tree, id(&document, "target"));
    assert_eq!(child.used_width(), UsedLength(100.0));
    assert_eq!(child.used_margins().left, UsedLength(0.0));
    assert_eq!(child.used_margins().right, UsedLength(100.0));
    assert!(matches!(
        child.style().margin.left,
        LengthOrAuto::Length(crate::css::values::Length::Math(_))
    ));
    assert_eq!(child.style().margin.right, LengthOrAuto::Auto);
    assert!(tree.diagnostics().iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("margin: invalid at used-value time")
    }));
}

#[test]
fn extreme_finite_margins_report_overflow_without_poisoning_block_or_inline_geometry() {
    fn assert_finite(layout: &LayoutBox) {
        for rect in [layout.rect(), layout.content_rect()] {
            assert!(
                [rect.x, rect.y, rect.width, rect.height]
                    .into_iter()
                    .all(f32::is_finite),
                "{}: {rect:?}",
                layout.tag_name()
            );
        }
        let margins = layout.used_margins();
        assert!(
            [margins.top, margins.right, margins.bottom, margins.left]
                .into_iter()
                .all(|margin| margin.0.is_finite())
        );
        for child in layout.children() {
            assert_finite(child);
        }
    }
    let (_, tree) = layout(
        "<!doctype html><style>body{margin:0} section{margin:3e38px 0}\
        em{margin:0 3e38px;padding:0 3e38px}\
        img{margin:3e38px;padding:3e38px}</style>\
        <section><em>Inline</em><img></section>\
        <section>Another block</section>",
        400.0,
    );
    assert_finite(tree.root());
    assert!(
        tree.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("non-finite layout result"))
    );
}
