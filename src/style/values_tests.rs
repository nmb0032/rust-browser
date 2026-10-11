use super::*;
use crate::{
    css::values::{
        BackgroundImage, BorderStyle, Edges, FamilyName, FontFamily, FontStyle, FontWeight, Length,
        LineHeight,
    },
    dom::{Document, NodeId},
    html::parser,
};

fn find(document: &Document, name: &str) -> NodeId {
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

fn style(source: &str) -> ComputedStyle {
    let document = parser::parse(&format!("<p id='target' style='{source}'>text</p>")).unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    result
        .styles
        .get(find(&document, "target"))
        .unwrap()
        .clone()
}

#[test]
fn token_values_decode_escapes_preserve_functions_and_do_not_concatenate_comments() {
    let computed = style(
        r"font-size:20p\78; margin:1e\6d  -2px 30% 0; padding:1px/**/2px;
        border:2px s\6f lid r\67 b(100%, 0%, 50%);
        border-color:rgb(1, /*inside*/ 2, 3) rgb(4 /*inside*/ 5 6) transparent \62 lue;
        background:n\6f ne rgb(10,20,30)",
    );
    assert_eq!(
        computed.margin,
        Edges {
            top: Length::Px(20.0),
            right: Length::Px(-2.0),
            bottom: Length::Percent(30.0),
            left: Length::Px(0.0)
        }
    );
    assert_eq!(
        computed.padding,
        Edges {
            top: Length::Px(1.0),
            right: Length::Px(2.0),
            bottom: Length::Px(1.0),
            left: Length::Px(2.0)
        }
    );
    assert_eq!(computed.border_width, Edges::all(2.0));
    assert_eq!(computed.border_style, Edges::all(BorderStyle::Solid));
    assert_eq!(
        computed.border_color,
        Edges {
            top: Some(0x010203),
            right: Some(0x040506),
            bottom: None,
            left: Some(0x0000ff)
        }
    );
    assert_eq!(computed.background_color, Some(0x0a141e));
    let document = parser::parse(
        "<p id='target' style='margin:4px; margin:1/**/px;
        border:solid blue; border:s/**/olid red'>text</p>",
    )
    .unwrap();
    let result = resolve(&document);
    let computed = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(computed.margin, Edges::all(Length::Px(4.0)));
    assert_eq!(computed.border_color, Edges::all(Some(0x0000ff)));
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
}

#[test]
fn border_shorthands_reset_omitted_components_and_side_forms_cascade_independently() {
    let computed = style(
        "color:green; border:9px dashed blue; border:solid;
        border-left:thick red; border-left-style:double; border-top-color:purple;
        border-right:2px solid blue !important; border-right:hidden",
    );
    assert_eq!(
        computed.border_width,
        Edges {
            top: 3.0,
            right: 2.0,
            bottom: 3.0,
            left: 5.0
        }
    );
    assert_eq!(
        computed.border_style,
        Edges {
            top: BorderStyle::Solid,
            right: BorderStyle::Solid,
            bottom: BorderStyle::Solid,
            left: BorderStyle::Double
        }
    );
    assert_eq!(
        computed.border_color,
        Edges {
            top: Some(0x800080),
            right: Some(0x0000ff),
            bottom: Some(0x008000),
            left: Some(0xff0000)
        }
    );
    for invisible in ["none", "hidden"] {
        let computed = style(&format!("border:8px solid red; border:{invisible}"));
        assert_eq!(computed.border_width, Edges::all(0.0));
        assert_eq!(computed.border_color, Edges::all(Some(0)));
    }
    assert_eq!(style("border-width:7px").border_width, Edges::all(0.0));
    assert_eq!(style("border-style:solid").border_width, Edges::all(3.0));
    assert_eq!(
        style("border:solid; border-width:initial").border_width,
        Edges::all(3.0)
    );
}

#[test]
fn invalid_shorthands_are_atomic_and_do_not_erase_previous_longhands() {
    for invalid in [
        "border:1px 2px solid red",
        "border:solid dashed red",
        "border:red blue",
        "border:2% solid red",
        "border:-2px solid red",
        "border:solid nonsense",
        "border:inset red",
        "border:solid inherit",
        "border-top:1px solid red blue",
        "border-color:rgb(1, 2, 3) bogus",
        "border-style:solid none bad",
        "border-color:rgb(1+2+3)",
        "border-color:rgb(1/**/2/**/3)",
        "border-color:rgb(1, 2%, 3)",
        "border-color:rgb(1, 2, 3 / 0.5)",
        "border-color:rgb(1e999, 2, 3)",
        "margin:1px 2px 3px 4px 5px",
        "margin:rgb(1, 2, 3)",
        "padding:1px -2px",
        "padding:1 px",
        "padding:initial 2px",
        "margin:1e99px",
        "padding:NaNpx",
        "background:red blue",
        "background:none none",
        "background:red, blue",
        "background:url(x) red",
        "background:linear-gradient(red, blue)",
        "background:none center / cover red",
        "background-image:url(x)",
        "font:italic italic 20px serif",
        "font:bold 700 20px serif",
        "font:normal normal normal 20px serif",
        "font:20px",
        "font:20px / serif",
        "font:20px / -1 serif",
        "font:20px / 1.5 / serif",
        "font:caption",
        "font:small-caps 20px serif",
        "font:20px serif,",
        "font:20px initial",
        "font:20px serif sans-serif",
        "font:20px 42",
        "font:20px / calc(2 * 3px * 1px) serif",
        "font:italic -20px serif",
        "font-family:foo()",
        "font-weight:0",
        "font-weight:1001",
        "font-size:1e99px",
        "line-height:1e99",
    ] {
        let document = parser::parse(&format!("<p id='target' style='border:4px solid green;
            margin:3px; padding:2px; background:red; font:italic bold 18px/2 serif; {invalid}'>text</p>")).unwrap();
        let result = resolve(&document);
        assert_eq!(
            result.stylesheets.diagnostics.len(),
            1,
            "{invalid}: {:?}",
            result.stylesheets.diagnostics
        );
        let computed = result.styles.get(find(&document, "target")).unwrap();
        assert_eq!(computed.border_width, Edges::all(4.0), "{invalid}");
        assert_eq!(
            computed.border_style,
            Edges::all(BorderStyle::Solid),
            "{invalid}"
        );
        assert_eq!(
            computed.border_color,
            Edges::all(Some(0x008000)),
            "{invalid}"
        );
        assert_eq!(computed.background_color, Some(0xff0000), "{invalid}");
        assert_eq!(computed.margin, Edges::all(Length::Px(3.0)), "{invalid}");
        assert_eq!(computed.padding, Edges::all(Length::Px(2.0)), "{invalid}");
        assert_eq!(computed.font_size, 18.0, "{invalid}");
        assert_eq!(computed.font_weight, FontWeight(700.0), "{invalid}");
        assert_eq!(computed.font_style, FontStyle::Italic, "{invalid}");
        assert_eq!(computed.line_height, LineHeight::Number(2.0), "{invalid}");
        assert_eq!(
            computed.font_family,
            FontFamily(vec![FamilyName::Generic("serif".into())]),
            "{invalid}"
        );
    }
}

#[test]
fn font_shorthand_parses_optional_components_and_resets_supported_longhands() {
    for prefix in [
        "italic 650",
        "650 italic",
        "italic normal",
        "normal 650",
        "normal normal",
        "",
    ] {
        let computed = style(&format!(
            r#"font:{prefix} 20px / 150% "Noto Sans", Times New Roman, s\65 rif"#
        ));
        assert_eq!(computed.font_size, 20.0, "{prefix}");
        assert_eq!(computed.line_height, LineHeight::Length(Length::Px(30.0)));
        assert_eq!(
            computed.font_family.0,
            [
                FamilyName::Named("Noto Sans".into()),
                FamilyName::Named("Times New Roman".into()),
                FamilyName::Generic("serif".into()),
            ]
        );
    }
    let computed = style(
        "font-style:oblique; font-weight:900; line-height:40px;
        font-family:monospace; font:24px serif; font-weight:550.5; line-height:1.1",
    );
    assert_eq!(computed.font_style, FontStyle::Normal);
    assert_eq!(computed.font_weight, FontWeight(550.5));
    assert!(!computed.bold && !computed.italic);
    assert_eq!(computed.line_height, LineHeight::Number(1.1));
    assert_eq!(
        computed.font_family,
        FontFamily(vec![FamilyName::Generic("serif".into())])
    );
    assert_eq!(
        style("font:oblique 1000 125%/1.5 monospace").font_size,
        20.0
    );
    assert_eq!(style("font:large serif").font_size, 18.0);
    assert_eq!(style("font:0/0 serif").font_size, 0.0);
    assert_eq!(
        style("font:20px serif; font:16px serif !important; font-size:30px").font_size,
        16.0
    );
}

#[test]
fn font_inheritance_distinguishes_unitless_line_height_from_absolute_computed_lengths() {
    for (source, expected, used) in [
        ("1.5", LineHeight::Number(1.5), 60.0),
        ("150%", LineHeight::Length(Length::Px(30.0)), 30.0),
        ("1.5em", LineHeight::Length(Length::Px(30.0)), 30.0),
        ("normal", LineHeight::Normal, 50.0),
    ] {
        let document = parser::parse(&format!(
            "<div style='font:20px/{source} serif'>
            <span id='target' style='font-size:40px'>text</span></div>"
        ))
        .unwrap();
        let result = resolve(&document);
        assert!(result.stylesheets.diagnostics.is_empty());
        let computed = result.styles.get(find(&document, "target")).unwrap();
        assert_eq!(computed.line_height, expected);
        assert_eq!(computed.used_line_height(), used);
    }
    let document = parser::parse(
        "<div style='font-weight:500'><span id='target'
        style='font:bolder smaller/2 serif'>text</span></div>",
    )
    .unwrap();
    let result = resolve(&document);
    let computed = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(computed.font_weight, FontWeight(700.0));
    assert!((computed.font_size - 16.0 / 1.2).abs() < 0.001);
    assert_eq!(computed.line_height, LineHeight::Number(2.0));
}

#[test]
fn background_color_and_no_image_subset_reset_and_order_consistently() {
    assert_eq!(
        style("background-color:blue; background:none").background_color,
        None
    );
    assert_eq!(
        style("background:blue; background-color:red").background_color,
        Some(0xff0000)
    );
    assert_eq!(
        style("background:red !important; background:blue").background_color,
        Some(0xff0000)
    );
    assert_eq!(
        style("background:rgb(255, 0, 0) none").background_image,
        BackgroundImage::None
    );
    assert_eq!(
        style("background:none; background:initial").background_color,
        None
    );
}

#[test]
fn user_important_shorthand_keywords_expand_to_all_new_inherited_and_initial_values() {
    let document = parser::parse("<style>#target {border:9px solid red !important;
        font:italic bold 32px/3 monospace !important; background:red !important}</style>
        <div id='parent' style='font:oblique 650 20px/1.7 serif; border:4px dotted blue; color:green;
        background:currentcolor'><span id='target' style='border:8px dashed purple !important;
        font:30px sans-serif !important; background:blue !important; color:red'>text</span></div>").unwrap();
    for keyword in ["inherit", "initial", "unset"] {
        let result = resolve_with_user_stylesheets(
            &document,
            collect_stylesheets(&document),
            &[&format!(
                "#target {{border:{keyword} !important; font:{keyword} !important;
                background:{keyword} !important;}}"
            )],
        );
        assert!(
            result.stylesheets.diagnostics.is_empty(),
            "{:?}",
            result.stylesheets.diagnostics
        );
        let computed = result.styles.get(find(&document, "target")).unwrap();
        let parent = result.styles.get(find(&document, "parent")).unwrap();
        if keyword == "initial" {
            assert_eq!(computed.font_size, 16.0);
            assert_eq!(computed.font_style, FontStyle::Normal);
            assert_eq!(computed.line_height, LineHeight::Normal);
        } else {
            assert_eq!(computed.font_size, parent.font_size);
            assert_eq!(computed.font_weight, parent.font_weight);
            assert_eq!(computed.font_style, parent.font_style);
            assert_eq!(computed.font_family, parent.font_family);
            assert_eq!(computed.line_height, parent.line_height);
        }
        if keyword == "inherit" {
            assert_eq!(computed.border_style, parent.border_style);
            assert_eq!(computed.border_width, parent.border_width);
            assert_eq!(computed.background_color, Some(0xff0000));
        } else {
            assert_eq!(computed.border_style, Edges::all(BorderStyle::None));
            assert_eq!(computed.border_width, Edges::all(0.0));
            assert_eq!(computed.background_color, None);
        }
    }
}

#[test]
fn typed_style_dump_is_stable_in_document_order_and_includes_hidden_elements_not_text() {
    let document = parser::parse(
        "<style>span {display:none}</style><main id='parent' style='font:20px/1.5 serif'>
        <span id='hidden' style='width:25%; padding:1em; border:2px solid rgb(1,2,3)'>text</span>
        <p id='last'>text</p></main>",
    )
    .unwrap();
    let resolution = resolve(&document);
    let dump = crate::devtools::format_style(&document, &resolution.styles);
    assert_eq!(
        dump,
        crate::devtools::format_style(&document, &resolve(&document).styles)
    );
    assert_eq!(
        dump.matches("Element ").count(),
        document
            .preorder()
            .filter(|&id| document.node(id).unwrap().as_element().is_some())
            .count()
    );
    assert!(dump.find("id=Some(\"parent\")").unwrap() < dump.find("id=Some(\"hidden\")").unwrap());
    assert!(dump.find("id=Some(\"hidden\")").unwrap() < dump.find("id=Some(\"last\")").unwrap());
    assert!(dump.contains("display: None"));
    assert!(dump.contains("width: Percent(25.0)"));
    assert!(dump.contains("padding-left: Px(20.0)"));
    assert!(dump.contains("font-weight: FontWeight(400.0)"));
    assert!(dump.contains("line-height: Number(1.5)"));
    assert!(dump.contains("border-left-color: Rgb(66051)"));
    assert!(!dump.contains("#text"));
}

#[test]
fn typed_style_dump_snapshot_covers_all_computed_longhands() {
    use crate::dom::{Attribute, ElementData};
    let mut document = Document::new();
    document
        .append_element(
            document.root_id(),
            ElementData::new(
                "p".into(),
                vec![
                    Attribute::new("id".into(), "target".into()),
                    Attribute::new(
                        "style".into(),
                        r#"font:italic 650 20px/1.5 "Example Font", serif;
            width:25%; margin:1em 2%; border:2px solid rgb(1,2,3);
            background:none rgb(10,20,30)"#
                            .into(),
                    ),
                ],
            ),
        )
        .unwrap();
    let result = resolve(&document);
    assert!(result.stylesheets.diagnostics.is_empty());
    assert_eq!(
        crate::devtools::format_style(&document, &result.styles),
        include_str!("../../tests/snapshots/style.expected")
    );
}
