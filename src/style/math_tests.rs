use super::*;
use crate::{
    css::{
        media::MediaEnvironment,
        values::{Edges, Length, LineHeight, RelativeUnit},
    },
    dom::{Document, NodeId},
    html::parser,
    layout, paint,
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

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn root_font_and_line_height_dependencies_resolve_in_css_order() {
    let document = parser::parse(
        "<style>
        :root {font-size:2rem; line-height:2rlh}
        body {font-size:20px; line-height:30px}
        #target {font-size:calc(1rem + 50%); line-height:2lh;
        width:2rem; margin:1lh 1rlh; padding:1em}
        #child {font-size:10px; width:inherit; line-height:inherit; margin:inherit}
        </style><div id='target'><span id='child'>text</span></div>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let root = document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.tag_name() == "html")
        })
        .unwrap();
    let root = result.styles.get(root).unwrap();
    assert_eq!(root.font_size, 32.0);
    let initial_line = paint::load_bundled_font()
        .unwrap()
        .horizontal_line_metrics(16.0)
        .unwrap()
        .new_line_size;
    assert_eq!(root.used_line_height(), initial_line * 2.0);
    let target = result.styles.get(id(&document, "target")).unwrap();
    assert_eq!(target.font_size, 42.0);
    assert_eq!(target.line_height, LineHeight::Length(Length::Px(60.0)));
    assert_eq!(target.width, Some(Length::Px(64.0)));
    assert_eq!(target.margin.top, Length::Px(60.0).into());
    assert_eq!(target.margin.right, Length::Px(initial_line * 2.0).into());
    assert_eq!(target.padding, Edges::all(Length::Px(42.0)));
    let child = result.styles.get(id(&document, "child")).unwrap();
    assert_eq!(child.width, target.width);
    assert_eq!(child.line_height, target.line_height);
    assert_eq!(child.margin, target.margin);
    assert_eq!(
        target.specified_width().unwrap().0,
        Length::Relative(2.0, RelativeUnit::Rem)
    );
    near(
        target.computed_width().unwrap().use_value(999.0).unwrap().0,
        64.0,
    );
}

#[test]
fn configured_initial_font_size_controls_root_rem_keywords_and_initial() {
    let document = parser::parse("<style>:root {font-size:2rem} #a {font-size:medium} #b {font-size:initial} #c {font-size:1rem}</style><div id='a'></div><div id='b'></div><div id='c'></div>").unwrap();
    let result = resolve_with_environment(
        &document,
        MediaEnvironment {
            initial_font_size: 20.0,
            ..MediaEnvironment::default()
        },
    );
    assert!(result.stylesheets.diagnostics.is_empty());
    assert_eq!(
        result.styles.get(id(&document, "a")).unwrap().font_size,
        20.0
    );
    assert_eq!(
        result.styles.get(id(&document, "b")).unwrap().font_size,
        20.0
    );
    assert_eq!(
        result.styles.get(id(&document, "c")).unwrap().font_size,
        40.0
    );
}

#[test]
fn font_units_use_the_bundled_face_metrics_and_initial_root_special_cases() {
    let document = parser::parse(
        "<style>:root {font-size:2em; line-height:2lh; width:1rem}
        #target {font-size:20px; width:calc(3ch + 2ex)}</style><div id='target'></div>",
    )
    .unwrap();
    let result = resolve(&document);
    let font = paint::load_bundled_font().unwrap();
    let target = result.styles.get(id(&document, "target")).unwrap();
    near(
        target.width.as_ref().unwrap().resolve(20.0, 0.0),
        font.metrics('0', 20.0).advance_width * 3.0 + font.metrics('x', 20.0).bounds.height * 2.0,
    );
    let document = parser::parse("<style>:root {font-size:2lh; line-height:2rlh}</style>").unwrap();
    let result = resolve(&document);
    let root = document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.tag_name() == "html")
        })
        .unwrap();
    let initial_line = font.horizontal_line_metrics(16.0).unwrap().new_line_size;
    assert_eq!(
        result.styles.get(root).unwrap().font_size,
        initial_line * 2.0
    );
    assert_eq!(
        result.styles.get(root).unwrap().used_line_height(),
        initial_line * 2.0
    );
}

#[test]
fn viewport_variants_have_explicit_sizes_and_restyle_on_environment_changes() {
    let document = parser::parse("<div id='target' style='width:calc(10svh + 10lvh + 10dvh); margin:10vw; padding:10vmin'></div>").unwrap();
    let environment = MediaEnvironment {
        small_viewport: Some((300.0, 400.0)),
        large_viewport: Some((500.0, 700.0)),
        dynamic_viewport: Some((400.0, 600.0)),
        ..MediaEnvironment::new(400.0, 600.0)
    };
    let result = resolve_with_environment(&document, environment);
    let target = result.styles.get(id(&document, "target")).unwrap();
    assert_eq!(target.width, Some(Length::Px(170.0)));
    assert_eq!(target.margin, Edges::all(Length::Px(50.0).into()));
    assert_eq!(target.padding, Edges::all(Length::Px(50.0)));
    for (width, height) in [(400.0, 600.0), (800.0, 900.0)] {
        let result = resolve_with_environment(&document, MediaEnvironment::new(width, height));
        let target = result.styles.get(id(&document, "target")).unwrap();
        near(
            target.width.as_ref().unwrap().resolve(16.0, 0.0),
            height * 0.3,
        );
        let crate::css::values::LengthOrAuto::Length(length) = &target.margin.left else {
            panic!("expected a length margin");
        };
        near(length.resolve(16.0, 0.0), width * 0.1);
    }
}

#[test]
fn variables_shorthands_and_computed_invalid_values_follow_the_cascade() {
    let document = parser::parse("<style>
        :root {--g:2rem; --w:calc(100% - var(--g))}
        #target {width:var(--w); padding:clamp(1px,10%,20px) calc(1em + 2px);
          margin:calc(-2px) max(1px,2px); border:calc(1rem / 2) solid red;
          font:calc(1rem + 4px)/calc(1 + .5) serif}
        #bad {font-size:30px; font-size:calc(1px / 0); width:30px; width:calc(1px / 0)}
        #syntax {width:20px; width:calc(1px + 2); border:2px solid red; border:calc(1px + 1%) solid}
        #varbad {--v:calc(1px + 1s); width:20px; width:var(--v)}
        </style><div id='target'></div><div id='bad'></div><div id='syntax'></div><div id='varbad'></div>").unwrap();
    let result = resolve(&document);
    let target = result.styles.get(id(&document, "target")).unwrap();
    near(target.width.as_ref().unwrap().resolve(20.0, 200.0), 168.0);
    assert_eq!(target.font_size, 20.0);
    assert_eq!(target.line_height, LineHeight::Number(1.5));
    assert_eq!(target.padding.right, Length::Px(22.0));
    near(target.padding.top.resolve(20.0, 100.0), 10.0);
    assert_eq!(target.margin.top, Length::Px(-2.0).into());
    assert_eq!(target.margin.right, Length::Px(2.0).into());
    assert_eq!(target.border_width, Edges::all(8.0));
    let bad = result.styles.get(id(&document, "bad")).unwrap();
    assert_eq!(bad.font_size, 16.0);
    assert_eq!(bad.width, None);
    let syntax = result.styles.get(id(&document, "syntax")).unwrap();
    assert_eq!(syntax.width, Some(Length::Px(20.0)));
    assert_eq!(syntax.border_width, Edges::all(2.0));
    assert_eq!(
        result.styles.get(id(&document, "varbad")).unwrap().width,
        None
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("invalid at computed-value time"))
    );
}

#[test]
fn math_line_height_lengths_are_computed_before_inheritance() {
    let document = parser::parse("<div style='font:20px/calc(2 * 3px) serif'><span id='target' style='font-size:40px'>text</span></div>").unwrap();
    let result = resolve(&document);
    assert!(result.stylesheets.diagnostics.is_empty());
    assert_eq!(
        result
            .styles
            .get(id(&document, "target"))
            .unwrap()
            .used_line_height(),
        6.0
    );
}

#[test]
fn numeric_math_is_deferred_until_its_font_context_is_known() {
    let document = parser::parse("<div id='parent' style='font-size:40px; line-height:calc(1em / 10px); font-weight:calc(1em / 1px * 20)'><span id='child' style='font-size:10px'>text</span></div><div id='negative' style='line-height:calc(-2); font-weight:calc(2000)'></div>").unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let parent = result.styles.get(id(&document, "parent")).unwrap();
    assert_eq!(parent.line_height, LineHeight::Number(4.0));
    assert_eq!(parent.used_line_height(), 160.0);
    assert_eq!(parent.font_weight.0, 800.0);
    assert_eq!(
        result
            .styles
            .get(id(&document, "child"))
            .unwrap()
            .used_line_height(),
        40.0
    );
    let negative = result.styles.get(id(&document, "negative")).unwrap();
    assert_eq!(negative.used_line_height(), 0.0);
    assert_eq!(negative.font_weight.0, 1000.0);
}

#[test]
fn percentage_math_reaches_used_geometry_and_actual_raster_edges_on_resize() {
    let document = parser::parse(
        "<style>:root {font-size:20px} body {margin:0}
        #calc {width:calc(100% - 2rem); padding:calc(10% - 2px); margin:0}
        #clamp {width:clamp(100px,50%,300px); margin:0; background:red}
        #negative {width:calc(-20px); padding:calc(-2px); border:calc(-1px) solid}
        </style><div id='calc'>math</div><div id='clamp'>clamp</div><div id='negative'></div>",
    )
    .unwrap();
    let font = paint::load_bundled_font().unwrap();
    for width in [200.0, 400.0, 800.0, 400.0] {
        let result = resolve_with_environment(&document, MediaEnvironment::new(width, 600.0));
        assert!(
            result.stylesheets.diagnostics.is_empty(),
            "{:?}",
            result.stylesheets.diagnostics
        );
        let computed = result.styles.get(id(&document, "calc")).unwrap();
        assert!(computed.width.as_ref().unwrap().has_percentage());
        let mut tree = layout::build_layout_tree_with_styles(&document, &result.styles);
        tree.layout(width, &font);
        fn find(node: &layout::LayoutBox, id: NodeId) -> Option<&layout::LayoutBox> {
            if node.dom_node_id() == Some(id) {
                Some(node)
            } else {
                node.children().iter().find_map(|child| find(child, id))
            }
        }
        let calc = find(tree.root(), id(&document, "calc")).unwrap();
        near(calc.used_width().0, width - 48.0 - 40.0);
        let clamp = find(tree.root(), id(&document, "clamp")).unwrap();
        near(
            clamp.used_width().0,
            ((width - 48.0) * 0.5).clamp(100.0, 300.0),
        );
        let negative = find(tree.root(), id(&document, "negative")).unwrap();
        assert_eq!(negative.used_width().0, 0.0);
        assert_eq!(negative.rect().width, 0.0);
        let actual = clamp.actual_rect(1.5).unwrap();
        assert_eq!(actual.left, (clamp.rect().x * 1.5).floor() as i32);
        assert_eq!(
            actual.right,
            ((clamp.rect().x + clamp.rect().width) * 1.5).ceil() as i32
        );
        let stages = crate::devtools::format_values(&tree, 1.5);
        assert!(stages.contains("specified Width: SpecifiedLength(Math("));
        assert!(stages.contains("used content-width: UsedLength("));
        assert!(stages.contains("actual border-rect @ 1.5x"));
        let list = paint::build_display_list(&tree, width, 600.0, false);
        assert!(list.iter().any(|item|matches!(item,paint::DisplayItem::Rect{w,color:0xff0000,..} if (*w-clamp.used_width().0).abs()<0.001)));
    }
}
