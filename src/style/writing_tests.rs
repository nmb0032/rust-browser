use super::*;
use crate::{
    css::values::{Length, LengthOrAuto},
    dom::Document,
    html::parser,
};

fn id(doc: &Document, name: &str) -> crate::dom::NodeId {
    doc.preorder()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}
fn style(css: &str) -> ComputedStyle {
    let doc = parser::parse(&format!(
        "<!doctype html><span id=target style='{css}'>text</span>"
    ))
    .unwrap();
    let result = resolve(&doc);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    result.styles.get(id(&doc, "target")).unwrap().clone()
}

#[test]
fn logical_and_physical_declarations_share_priority_after_axis_computation() {
    let s = style("margin-left:1px;margin-inline-start:2px;direction:ltr");
    assert_eq!(s.margin.left, LengthOrAuto::Length(Length::Px(2.0)));
    let s = style("margin-inline-start:2px;margin-left:1px;direction:ltr");
    assert_eq!(s.margin.left, LengthOrAuto::Length(Length::Px(1.0)));
    let s = style("margin-left:1px!important;margin-inline-start:2px;direction:ltr");
    assert_eq!(s.margin.left, LengthOrAuto::Length(Length::Px(1.0)));
    let s = style(
        "margin-inline-start:2px;direction:rtl;writing-mode:vertical-rl;padding-block:3px 4px;border-inline:2px solid red;inset-block-start:5px;inline-size:60px;block-size:40px",
    );
    assert_eq!(s.margin.bottom, LengthOrAuto::Length(Length::Px(2.0)));
    assert_eq!(s.padding.right, Length::Px(3.0));
    assert_eq!(s.padding.left, Length::Px(4.0));
    assert_eq!(s.border_width.top, 2.0);
    assert_eq!(s.border_width.bottom, 2.0);
    assert_eq!(s.inset.right, LengthOrAuto::Length(Length::Px(5.0)));
    assert_eq!(s.height, Some(Length::Px(60.0)));
    assert_eq!(s.width, Some(Length::Px(40.0)));
}

#[test]
fn inheritance_reads_the_parent_logical_axis_before_remapping_to_the_child() {
    let doc=parser::parse("<!doctype html><div style='margin-inline-start:7px;unicode-bidi:isolate;writing-mode:horizontal-tb'><span id=target style='writing-mode:vertical-rl;margin-inline-start:inherit'>x</span></div>").unwrap();
    let result = resolve(&doc);
    let s = result.styles.get(id(&doc, "target")).unwrap();
    assert_eq!(s.margin.top, LengthOrAuto::Length(Length::Px(7.0)));
    assert_eq!(s.unicode_bidi, UnicodeBidi::Normal);
    assert_eq!(
        style("unicode-bidi:bidi-override;direction:rtl;writing-mode:vertical-rl;all:initial")
            .unicode_bidi,
        UnicodeBidi::BidiOverride
    );
    assert_eq!(
        style("unicode-bidi:isolate;direction:rtl;all:initial").direction,
        Direction::Rtl
    );
}

#[test]
fn variables_layers_and_inherited_writing_modes_map_at_computation_not_parsing() {
    let doc=parser::parse("<!doctype html><style>@layer a,b;@layer a{#target{padding-inline-start:9px}}@layer b{#target{padding-top:4px;padding-inline-start:revert-layer}}div{writing-mode:vertical-lr;direction:rtl}</style><div><span id=target style='--gap:3px;margin-block:var(--gap);max-inline-size:70px;overflow-inline:hidden'>x</span></div>").unwrap();
    let result = resolve(&doc);
    let s = result.styles.get(id(&doc, "target")).unwrap();
    assert_eq!(s.writing_mode, WritingMode::VerticalLr);
    assert_eq!(s.padding.bottom, Length::Px(9.0));
    assert_eq!(s.padding.top, Length::Px(4.0));
    assert_eq!(s.margin.left, LengthOrAuto::Length(Length::Px(3.0)));
    assert_eq!(s.max_height, Some(Length::Px(70.0)));
    assert_eq!(s.overflow_y, Overflow::Hidden);
}

#[test]
fn viewport_units_use_root_axes_and_container_units_use_each_eligible_container_axis() {
    let doc=parser::parse("<!doctype html><style>html{writing-mode:vertical-rl}#target{writing-mode:horizontal-tb;width:10vi;height:10vb;margin-left:10cqi;margin-top:10cqb}</style><div id=target>x</div>").unwrap();
    let environment = crate::css::media::MediaEnvironment {
        viewport_width: 400.0,
        viewport_height: 300.0,
        ..Default::default()
    };
    let result = resolve_with_environment(&doc, environment);
    let s = result.styles.get(id(&doc, "target")).unwrap();
    assert_eq!(s.width, Some(Length::Px(30.0)));
    assert_eq!(s.height, Some(Length::Px(40.0)));
    assert_eq!(s.margin.left, LengthOrAuto::Length(Length::Px(30.0)));
    assert_eq!(s.margin.top, LengthOrAuto::Length(Length::Px(40.0)));
    let doc=parser::parse("<!doctype html><style>#container{writing-mode:vertical-rl;container-type:inline-size;width:120px;height:80px}#target{writing-mode:horizontal-tb;width:50cqi;height:10cqb}</style><div id=container><div id=target>x</div></div>").unwrap();
    let first = resolve_with_environment(&doc, environment);
    let font = crate::text::FontContext::bundled();
    let mut tree = crate::layout::build_layout_tree_with_styles(&doc, &first.styles);
    tree.layout_viewport(400.0, 300.0, &font);
    let snapshot = ContainerSnapshot::from_layout(&tree, environment);
    let result = resolve_with_container_environment(
        &doc,
        collect_stylesheets(&doc),
        &[],
        environment,
        &Default::default(),
        &snapshot,
    );
    let s = result.styles.get(id(&doc, "target")).unwrap();
    assert_eq!(s.width, Some(Length::Px(40.0)));
    assert_eq!(s.height, Some(Length::Px(30.0)));
}
