use super::*;
use crate::{
    css::values::{Edges, Length, LengthOrAuto},
    dom::{Document, NodeId},
    html::parser,
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

fn resolved(source: &str) -> (Document, StyleResolution) {
    let document = parser::parse(source).unwrap();
    let result = resolve(&document);
    (document, result)
}

fn margin(source: &str) -> Edges<LengthOrAuto> {
    let (document, result) = resolved(&format!(
        "<!doctype html><span id='target' style='{source}'></span>"
    ));
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{source}: {:?}",
        result.stylesheets.diagnostics
    );
    result
        .styles
        .get(id(&document, "target"))
        .unwrap()
        .margin
        .clone()
}

#[test]
fn auto_margin_shorthands_expand_atomically_and_longhands_remain_typed() {
    let auto = LengthOrAuto::Auto;
    let px = |value| LengthOrAuto::Length(Length::Px(value));
    for (source, expected) in [
        ("margin:auto", Edges::all(auto.clone())),
        (
            "margin:2px auto",
            Edges {
                top: px(2.0),
                right: auto.clone(),
                bottom: px(2.0),
                left: auto.clone(),
            },
        ),
        (
            "margin:auto -3px 4px",
            Edges {
                top: auto.clone(),
                right: px(-3.0),
                bottom: px(4.0),
                left: px(-3.0),
            },
        ),
        (
            "margin:auto 2px 3px 4px",
            Edges {
                top: auto.clone(),
                right: px(2.0),
                bottom: px(3.0),
                left: px(4.0),
            },
        ),
        (
            r"margin: /*a*/ \61 uTo /**/ 2px; margin-left:AuTo",
            Edges {
                top: auto.clone(),
                right: px(2.0),
                bottom: auto.clone(),
                left: auto.clone(),
            },
        ),
        (
            "margin:0; margin-top:auto; margin-right:auto; margin-bottom:auto; margin-left:auto",
            Edges::all(auto),
        ),
    ] {
        assert_eq!(margin(source), expected, "{source}");
    }
    for invalid in [
        "auto garbage",
        "auto 1px initial",
        "auto 2px 3px 4px 5px",
        "auto, 1px",
        "auto / 1px",
        "auto 1e99px",
    ] {
        let (document, result) = resolved(&format!(
            "<!doctype html><span id='target' style='margin:7px; margin:{invalid}'></span>"
        ));
        assert_eq!(
            result.styles.get(id(&document, "target")).unwrap().margin,
            Edges::all(px(7.0)),
            "{invalid}"
        );
        assert_eq!(result.stylesheets.diagnostics.len(), 1, "{invalid}");
    }
}

#[test]
fn margin_lengths_percentages_math_and_vars_keep_specified_and_computed_stages() {
    let (document, result) = resolved(
        "<!doctype html><span id='target' style='font-size:20px; --m:auto -10% calc(1em - 3px) 2em;\
        margin:var(--m);margin-left:var(--left, auto)'></span>",
    );
    assert!(result.stylesheets.diagnostics.is_empty());
    let style = result.styles.get(id(&document, "target")).unwrap();
    assert_eq!(
        style.margin,
        Edges {
            top: LengthOrAuto::Auto,
            right: Length::Percent(-10.0).into(),
            bottom: Length::Px(17.0).into(),
            left: LengthOrAuto::Auto,
        }
    );
    assert_eq!(style.specified_margins["Margin(Top)"], LengthOrAuto::Auto);
    assert_eq!(style.specified_margins["Margin(Left)"], LengthOrAuto::Auto);
    assert!(matches!(
        style.specified_margins["Margin(Bottom)"],
        LengthOrAuto::Length(Length::Math(_))
    ));
    assert_eq!(
        margin("--m:auto 2px; margin:var(--m); margin-right:3px !important"),
        Edges {
            top: LengthOrAuto::Auto,
            right: Length::Px(3.0).into(),
            bottom: LengthOrAuto::Auto,
            left: Length::Px(2.0).into()
        }
    );
    assert_eq!(
        margin("margin:calc(-2px) max(1px,2px) auto"),
        Edges {
            top: Length::Px(-2.0).into(),
            right: Length::Px(2.0).into(),
            bottom: LengthOrAuto::Auto,
            left: Length::Px(2.0).into()
        }
    );
}

#[test]
fn invalid_pending_margin_shorthands_unset_all_sides_without_reviving_older_values() {
    for value in ["auto invalid", "auto initial", "auto 2px 3px 4px 5px"] {
        let (document, result) = resolved(&format!(
            "<!doctype html><span id='target' style='--m:{value};margin:auto;\
            margin:var(--m);margin-left:auto !important'></span>"
        ));
        let style = result.styles.get(id(&document, "target")).unwrap();
        assert_eq!(
            style.margin,
            Edges {
                left: LengthOrAuto::Auto,
                ..Edges::default()
            }
        );
        assert!(
            result
                .stylesheets
                .diagnostics
                .iter()
                .any(|d| d.message.contains("computed")),
            "{value}"
        );
    }
}

#[test]
fn css_wide_and_origin_layer_rollback_preserve_auto_instead_of_zero() {
    for (keyword, expected) in [
        (
            "inherit",
            Edges {
                top: Length::Px(20.0).into(),
                right: LengthOrAuto::Auto,
                bottom: Length::Px(20.0).into(),
                left: LengthOrAuto::Auto,
            },
        ),
        ("initial", Edges::default()),
        ("unset", Edges::default()),
    ] {
        let (document, result) = resolved(&format!(
            "<!doctype html><div style='font-size:20px;margin:1em auto'>\
            <span id='target' style='font-size:40px;margin:{keyword}'></span></div>"
        ));
        assert!(result.stylesheets.diagnostics.is_empty());
        assert_eq!(
            result.styles.get(id(&document, "target")).unwrap().margin,
            expected,
            "{keyword}"
        );
    }
    let (document, result) = resolved(
        "<!doctype html><style>@layer base,theme;\
        @layer base{#target{margin:2px auto}}\
        @layer theme{#target{margin:9px;margin:var(--rollback,revert-layer)}}\
        </style><span id='target'></span>",
    );
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    assert_eq!(
        result.styles.get(id(&document, "target")).unwrap().margin,
        margin("margin:2px auto")
    );
    let document =
        parser::parse("<!doctype html><span id='target' style='margin:9px;margin:revert'></span>")
            .unwrap();
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &["#target{margin:auto}"],
    );
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    assert_eq!(
        result.styles.get(id(&document, "target")).unwrap().margin,
        Edges::all(LengthOrAuto::Auto)
    );
}

#[test]
fn direction_is_minimally_inherited_and_excluded_from_all() {
    for source in [
        "",
        "direction:inherit",
        "direction:unset",
        "all:initial",
        "all:unset",
        "all:inherit",
    ] {
        let (document, result) = resolved(&format!(
            "<!doctype html><div style='direction:rtl'>\
            <span id='target' style='{source}'></span></div>"
        ));
        assert!(result.stylesheets.diagnostics.is_empty());
        assert_eq!(
            result
                .styles
                .get(id(&document, "target"))
                .unwrap()
                .direction,
            Direction::Rtl,
            "{source}"
        );
    }
    let (document, result) = resolved(
        "<!doctype html><div style='direction:rtl'>\
        <span id='target' style='--d:ltr;direction:var(--d);direction:initial'></span></div>",
    );
    assert_eq!(
        result
            .styles
            .get(id(&document, "target"))
            .unwrap()
            .direction,
        Direction::Ltr
    );
}

#[test]
fn supports_auto_margins_without_admitting_auto_padding_or_border_lengths() {
    let (document, result) = resolved(
        "<!doctype html><style>#target{color:red}\
        @supports (margin:0 auto) and (margin-left:auto) and (direction:rtl)\
        and (not (padding:auto)) and (not (border-width:auto)){#target{color:blue}}\
        </style><span id='target'></span>",
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("unsupported @supports declaration"))
    );
    assert_eq!(
        result.styles.get(id(&document, "target")).unwrap().color,
        Some(0x0000ff)
    );
    for property in [
        "padding",
        "padding-left",
        "border-width",
        "border-left-width",
    ] {
        let declaration = crate::css::parse_declarations(&format!("{property}:auto")).unwrap();
        assert!(
            super::declarations::parse(&declaration[0], 0).is_err(),
            "{property}"
        );
    }
}
