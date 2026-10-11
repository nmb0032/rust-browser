use super::*;
use crate::{
    css::values::{ColorSpace, ResolvedColor},
    html::parser,
    paint,
};

fn find(document: &crate::dom::Document, id: &str) -> crate::dom::NodeId {
    document
        .preorder()
        .find(|&node| {
            document
                .node(node)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(id))
        })
        .unwrap()
}

fn channels(actual: ResolvedColor, expected: [f64; 3]) {
    for (a, e) in actual.channels.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-6, "{actual:?} != {expected:?}");
    }
}

#[test]
fn root_color_inheritance_currentcolor_and_relative_decoration_dependencies() {
    let document = parser::parse(
        "<html style='color:currentcolor'><body>
        <div id='parent' style='color:color(display-p3 .8 .3 .1 / .6);
            background:rgb(from currentcolor calc(r / 2) g b);
            border:2px solid color-mix(in display-p3, currentcolor, transparent)'>
            <span id='inherit'>inherited</span>
            <span id='child' style='color:color(display-p3 .2 .4 .8 / .8);
                background:inherit; border:inherit'>child</span>
            <span id='relative' style='color:color(from currentcolor display-p3 r g calc(b + .1));
                background:currentcolor'>relative text</span>
        </div></body></html>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let get = |id| result.styles.get(find(&document, id)).unwrap();
    assert_eq!(get("inherit").text_color, get("parent").text_color);
    assert_eq!(get("inherit").text_color.space, ColorSpace::DisplayP3);
    assert_eq!(get("child").background.space, ColorSpace::Srgb);
    let child_srgb = get("child").text_color.convert(ColorSpace::Srgb);
    channels(
        get("child").background,
        [
            child_srgb.channels[0] / 2.0,
            child_srgb.channels[1],
            child_srgb.channels[2],
        ],
    );
    assert_eq!(get("child").background.alpha, 0.8);
    channels(get("child").borders.top, [0.2, 0.4, 0.8]);
    assert_eq!(get("child").borders.top.alpha, 0.4);
    channels(get("relative").text_color, [0.8, 0.3, 0.2]);
    assert_eq!(get("relative").background, get("relative").text_color);
    assert_eq!(get("parent").text_color.alpha, 0.6);
    assert!(get("parent").specified_colors.contains_key("Color"));
}

#[test]
fn variable_shorthands_modern_colors_alpha_and_origin_precedence() {
    let document = parser::parse(
        "<style>:root{--tone:oklch(.7 .2 40 / .4)}
        #target{color:var(--tone);background:color-mix(in oklab, var(--tone) 25%, blue);
            border:3px solid rgb(from currentcolor r g b / .2)}
        </style><div id='target' style='--tone:color(display-p3 1 0 0 / .5);
        color:var(--tone) !important'>typed</div>",
    )
    .unwrap();
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &["#target{color:lab(50 20 30 / .7) !important}"],
    );
    assert!(result.stylesheets.diagnostics.is_empty());
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.text_color.space, ColorSpace::Lab);
    channels(style.text_color, [50.0, 20.0, 30.0]);
    assert_eq!(style.text_color.alpha, 0.7);
    assert_eq!(style.background.space, ColorSpace::Oklab);
    assert_eq!(style.background.alpha, 0.875);
    assert_eq!(style.borders.left.alpha, 0.2);
    assert_eq!(style.border_width.left, 3.0);
}

#[test]
fn malformed_modern_shorthands_are_atomic_but_invalid_variable_winners_unset() {
    let document = parser::parse(
        "<div id='bad' style='border:4px solid blue;
        border:8px dashed color(display-p3 1 0);background:red;
        background:color-mix(in srgb, red, blue) green'>bad</div>
        <div id='pending' style='--bad:color(display-p3 1 0);
        border:4px solid blue;border:var(--bad);color:red;color:var(--bad)'>pending</div>
        <div id='math' style='color:blue;color:rgb(from red calc(r / 0) g b);
        background:lab(50 1e300 0)'>math</div>",
    )
    .unwrap();
    let result = resolve(&document);
    let get = |id| result.styles.get(find(&document, id)).unwrap();
    assert_eq!(get("bad").borders.top, ResolvedColor::from_rgb(0x0000ff));
    assert_eq!(get("bad").border_width.top, 4.0);
    assert_eq!(get("bad").background, ResolvedColor::from_rgb(0xff0000));
    assert_eq!(get("pending").text_color, ResolvedColor::BLACK);
    assert_eq!(get("pending").border_width.top, 0.0);
    assert_eq!(get("math").text_color, ResolvedColor::BLACK);
    assert_eq!(get("math").background, ResolvedColor::TRANSPARENT);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("color calculation"))
    );
}

#[test]
fn native_color_data_reaches_fragments_and_display_list_without_byte_quantization() {
    let document = parser::parse(
        "<div style='color:oklch(.6 .2 45 / .4);
        background:color(display-p3 1 0 0 / .5);
        border:3px solid lab(50 20 30 / .6)'>text</div>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(result.stylesheets.diagnostics.is_empty());
    let mut layout = crate::layout::build_layout_tree_with_styles(&document, &result.styles);
    layout.layout(800.0, &paint::load_bundled_font().unwrap());
    let list = paint::build_display_list(&layout, 800.0, 600.0, false);
    assert!(list.iter().any(|item| matches!(item,
        paint::DisplayItem::ColorRect { color, .. } if color.space == ColorSpace::DisplayP3 && color.alpha == 0.5)));
    assert!(list.iter().any(|item| matches!(item,
        paint::DisplayItem::ColorRect { color, .. } if color.space == ColorSpace::Lab && color.alpha == 0.6)));
    assert!(list.iter().any(|item| matches!(item,
        paint::DisplayItem::ColorText { color, text, .. } if color.space == ColorSpace::Oklch && color.alpha == 0.4 && text == "text")));
}

#[test]
fn inherited_transparent_colors_retain_origin_coordinates_for_relative_functions() {
    let document = parser::parse(
        "<div style='color:color(display-p3 .8 .2 .1 / 0)'>
        <span id='child' style='color:color(from currentcolor display-p3 r g b / 1)'>visible</span>
        </div>",
    )
    .unwrap();
    let result = resolve(&document);
    let color = result
        .styles
        .get(find(&document, "child"))
        .unwrap()
        .text_color;
    assert_eq!(color.space, ColorSpace::DisplayP3);
    channels(color, [0.8, 0.2, 0.1]);
    assert_eq!(color.alpha, 1.0);
}

#[test]
fn typed_color_dumps_are_deterministic_and_preserve_existing_named_color_dump_shape() {
    let document = parser::parse(
        "<div id='wide' style='color:color(display-p3 .8 .2 .1 / .7);
        background:lab(50 10 20)'>wide</div><p style='color:red'>legacy</p>",
    )
    .unwrap();
    let result = resolve(&document);
    let dump = crate::devtools::format_style(&document, &result.styles);
    assert_eq!(
        dump,
        crate::devtools::format_style(&document, &resolve(&document).styles)
    );
    assert!(dump.contains("space: DisplayP3"), "{dump}");
    assert!(dump.contains("space: Lab"), "{dump}");
    assert!(dump.contains("color: Rgb(16711680)"), "{dump}");
}

#[cfg(feature = "style-bench-reference")]
#[test]
fn all_reference_matching_modes_preserve_typed_colors_and_dependencies() {
    let document = parser::parse(
        "<style>:root{--c:color(display-p3 .8 .2 .1 / .6)}
        .parent{color:var(--c);background:rgb(from currentcolor r g b / .4)}
        .parent > span{background:inherit; color:oklch(.7 .2 120)}
        </style><div class='parent'><span>typed</span></div>",
    )
    .unwrap();
    let production = resolve(&document);
    for mode in [
        BenchmarkMatching::Unindexed,
        BenchmarkMatching::Indexed,
        BenchmarkMatching::IndexedWithBloom,
    ] {
        let result = resolve_for_benchmark(&document, mode);
        assert_eq!(result.styles.nodes, production.styles.nodes);
        assert_eq!(
            result.stylesheets.diagnostics,
            production.stylesheets.diagnostics
        );
    }
}
