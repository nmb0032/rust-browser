use super::{BoxSizing, ComputedStyle, compat::DeclarationContext, declarations, resolve};
use crate::{
    css,
    css::values::Length,
    dom::{Document, DocumentMode, NodeId},
    html::parser,
};

fn target(document: &Document, id: &str) -> NodeId {
    document
        .preorder()
        .find(|&node| {
            document
                .node(node)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.attribute("id") == Some(id))
        })
        .unwrap()
}

fn style(document: &Document, id: &str) -> ComputedStyle {
    resolve(document)
        .styles
        .get(target(document, id))
        .unwrap()
        .clone()
}

fn accepts(mode: DocumentMode, source: &str) -> bool {
    let mut document = parser::parse("<p></p>").unwrap();
    document.set_mode(mode);
    let declaration = css::parse_declarations(source).unwrap().remove(0);
    declarations::parse_with_context(&declaration, 0, DeclarationContext::for_document(&document))
        .is_ok()
}

#[test]
fn compatibility_length_grammar_is_property_local_and_html_quirks_only() {
    for mode in [
        DocumentMode::NoQuirks,
        DocumentMode::LimitedQuirks,
        DocumentMode::Quirks,
    ] {
        let quirky = mode == DocumentMode::Quirks;
        for source in [
            "width:12",
            "height:+12",
            "font-size:1.5",
            "margin:-2 3",
            "padding:2 3",
            "border-width:2",
        ] {
            assert_eq!(accepts(mode, source), quirky, "{mode:?}: {source}");
        }
        for source in [
            "border:2 solid red",
            "border-top:2 solid red",
            "font:12 serif",
            "width:calc(12)",
            "width:calc(1px + 2)",
            "padding:-1",
            "height:-1",
            "border-width:-1",
        ] {
            assert!(!accepts(mode, source), "{mode:?}: {source}");
        }
        for source in [
            "width:0",
            "border:0 solid red",
            "font:0 serif",
            "line-height:2",
            "width:calc(1px * 2)",
            "color:rgb(1 2 3)",
        ] {
            assert!(accepts(mode, source), "{mode:?}: {source}");
        }
    }
    let mut xml = crate::xml::parse(
        "<html xmlns='http://www.w3.org/1999/xhtml'><p id='p' style='width:12;color:abc;font-size:12;-webkit-box-sizing:border-box;-webkit-text-fill-color:blue'/></html>",
    )
    .unwrap();
    for mode in [
        DocumentMode::NoQuirks,
        DocumentMode::LimitedQuirks,
        DocumentMode::Quirks,
    ] {
        xml.set_mode(mode);
        let s = style(&xml, "p");
        assert_eq!(s.width, None);
        assert_eq!(s.color, Some(0));
        assert_eq!(s.font_size, 16.0);
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
        assert_eq!(s.text_fill_color.legacy(), Some(0x0000ff));
    }
    let declaration = css::parse_declarations("width:12").unwrap().remove(0);
    assert!(declarations::parse(&declaration, 0).is_err());
    assert!(Length::parse("12").is_none());
    assert!(css::values::parse_color("abc").is_none());
}

#[test]
fn compatibility_hashless_colors_obey_integer_spelling_padding_and_context() {
    for (source, expected) in [
        ("abc", 0xaabbcc),
        ("AbCdEf", 0xabcdef),
        ("123", 0x000123),
        ("000123", 0x000123),
        ("+123", 0x000123),
        ("0123ab", 0x0123ab),
        ("1f", 0x00001f),
        ("0", 0),
        ("000000000", 0),
        ("999999", 0x999999),
        ("-0", 0),
        (r"\61 bc", 0xaabbcc),
        (r"0\61 bc", 0x000abc),
        (r"\31 23", 0x112233),
        ("00abc", 0x000abc),
        ("+0f00", 0x000f00),
    ] {
        let document = parser::parse(&format!("<p id='p' style='color:{source}'></p>")).unwrap();
        assert_eq!(style(&document, "p").color, Some(expected), "{source}");
        assert!(!accepts(DocumentMode::NoQuirks, &format!("color:{source}")));
        assert!(!accepts(
            DocumentMode::LimitedQuirks,
            &format!("color:{source}")
        ));
    }
    for source in [
        "ab",
        "abcd",
        "abcde",
        "abcdef0",
        "1.0",
        "1e2",
        "-1",
        "1234567",
        "123456a",
        "12g",
        "12px",
        "aabbccdd",
        "abc def",
        "000000000abcdef",
    ] {
        assert!(
            !accepts(DocumentMode::Quirks, &format!("color:{source}")),
            "{source}"
        );
    }
    for source in [
        "background:abc",
        "background:123",
        "border:solid abc",
        "border-top:solid abc",
        "-webkit-text-fill-color:abc",
        "color:color-mix(in srgb, abc, red)",
    ] {
        assert!(!accepts(DocumentMode::Quirks, source), "{source}");
    }
    assert!(accepts(DocumentMode::Quirks, "border-color:abc 123 1f red"));
}

#[test]
fn compatibility_escaped_names_and_retained_asts_are_recompiled_per_document_context() {
    let source = r"<style>p{\77 idth:12;\63 olor:abc;-w\65 bkit-box-sizing:border-box}
        @media (width >= 1px){@supports (width:12){p{padding:1 2}}}
        </style><p id='p'></p>";
    let mut document = parser::parse(source).unwrap();
    let sheets = super::collect_stylesheets(&document);
    let ast = std::sync::Arc::clone(&sheets.stylesheets[0].stylesheet);
    for mode in [
        DocumentMode::Quirks,
        DocumentMode::NoQuirks,
        DocumentMode::LimitedQuirks,
        DocumentMode::Quirks,
    ] {
        document.set_mode(mode);
        let result = super::resolve_with_stylesheets(&document, sheets.clone());
        assert!(std::sync::Arc::ptr_eq(
            &ast,
            &result.stylesheets.stylesheets[0].stylesheet
        ));
        let p = result.styles.get(target(&document, "p")).unwrap();
        assert_eq!(
            p.width,
            (mode == DocumentMode::Quirks).then_some(Length::Px(12.0))
        );
        assert_eq!(p.box_sizing, BoxSizing::BorderBox);
        assert_eq!(
            p.padding.left,
            Length::Px(if mode == DocumentMode::Quirks {
                2.0
            } else {
                0.0
            })
        );
    }
}

#[test]
fn compatibility_context_survives_vars_and_atomic_pending_shorthands() {
    for mode in [
        DocumentMode::NoQuirks,
        DocumentMode::LimitedQuirks,
        DocumentMode::Quirks,
    ] {
        let mut document = parser::parse("<style>
            p {--n:12;--hex:abc;--edges:1 2 3 4;width:var(--n);color:var(--hex);padding:var(--edges)}
            #bad {--edges:1 2 bogus; padding:4px;padding:var(--edges)}
            #border {border:1px solid red; border:var(--n) solid red}
            </style><p id='p'></p><p id='bad'></p><p id='border'></p>").unwrap();
        document.set_mode(mode);
        let s = style(&document, "p");
        assert_eq!(
            s.width,
            (mode == DocumentMode::Quirks).then_some(Length::Px(12.0))
        );
        assert_eq!(
            s.color,
            Some(if mode == DocumentMode::Quirks {
                0xaabbcc
            } else {
                0
            })
        );
        assert_eq!(
            s.padding.left,
            Length::Px(if mode == DocumentMode::Quirks {
                4.0
            } else {
                0.0
            })
        );
        assert_eq!(style(&document, "bad").padding.left, Length::Px(0.0));
        assert_eq!(style(&document, "border").border_width.left, 0.0);
    }
}

#[test]
fn compatibility_aliases_share_identity_cascade_and_rollback() {
    let document = parser::parse(
        "<!doctype html><style>
        @layer a,b;
        @layer a {p {-webkit-box-sizing:border-box}}
        @layer b {p {box-sizing:content-box; -webkit-box-sizing:revert-layer}}
        #order {box-sizing:content-box;-WEBKIT-BOX-SIZING:border-box}
        #important {-webkit-box-sizing:content-box!important;box-sizing:border-box}
        #variable {--box:border-box;-webkit-box-sizing:var(--box)}
        </style><p id='layer'></p><p id='order'></p><p id='important'></p><p id='variable'></p>",
    )
    .unwrap();
    for id in ["layer", "order", "variable"] {
        assert_eq!(
            style(&document, id).box_sizing,
            BoxSizing::BorderBox,
            "{id}"
        );
    }
    assert_eq!(
        style(&document, "important").box_sizing,
        BoxSizing::ContentBox
    );
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    for name in [
        "-webkit-width",
        "-webkit-color",
        "-webkit-flex",
        "-webkit-appearance",
        "-webkit-text-size-adjust",
        "text-fill-color",
    ] {
        assert!(declarations::properties(name).is_none(), "{name}");
    }
}

#[test]
fn compatibility_alias_important_origins_and_variable_rollback_use_canonical_priority() {
    let document = parser::parse(
        "<!doctype html><style>
        @layer a,b;
        @layer a {p{box-sizing:border-box!important}}
        @layer b {p{-webkit-box-sizing:content-box!important}}
        #rollback {--back:revert;-webkit-box-sizing:var(--back)}
        #invalid {box-sizing:border-box;-webkit-box-sizing:var(--missing)}
        </style><p id='layer'></p><p id='rollback'></p><p id='invalid'></p>",
    )
    .unwrap();
    assert_eq!(style(&document, "layer").box_sizing, BoxSizing::BorderBox);
    let result = super::resolve_with_user_stylesheets(
        &document,
        super::collect_stylesheets(&document),
        &["#rollback {box-sizing:content-box!important}"],
    );
    assert_eq!(
        result
            .styles
            .get(target(&document, "rollback"))
            .unwrap()
            .box_sizing,
        BoxSizing::ContentBox
    );
    let document = parser::parse("<!doctype html><p id='p' style='box-sizing:border-box;-webkit-box-sizing:var(--missing)'></p>").unwrap();
    assert_eq!(style(&document, "p").box_sizing, BoxSizing::ContentBox);
    let result = super::resolve_with_user_stylesheets(
        &document,
        super::collect_stylesheets(&document),
        &["p {-webkit-box-sizing:border-box!important}"],
    );
    assert_eq!(
        result
            .styles
            .get(target(&document, "p"))
            .unwrap()
            .box_sizing,
        BoxSizing::BorderBox
    );
    let document = parser::parse("<!doctype html><style>p{box-sizing:content-box;-webkit-box-sizing:var(--missing,revert)}</style><p id='p'></p>").unwrap();
    let result = super::resolve_with_user_stylesheets(
        &document,
        super::collect_stylesheets(&document),
        &["p {box-sizing:border-box}"],
    );
    assert_eq!(
        result
            .styles
            .get(target(&document, "p"))
            .unwrap()
            .box_sizing,
        BoxSizing::BorderBox
    );
}

#[test]
fn compatibility_fill_inherits_currentcolor_without_changing_other_colors() {
    let document = parser::parse("<!doctype html><style>
        body {color:red}
        #p {color:blue;border:1px solid currentcolor;background:currentcolor}
        #filled {-webkit-text-fill-color:green}
        #child {color:blue}
        #reset {-webkit-text-fill-color:initial;color:blue}
        </style><p id='p'>text</p><div id='filled'><span id='child'>child</span><span id='reset'>reset</span></div>").unwrap();
    let p = style(&document, "p");
    assert_eq!(p.color, Some(0x0000ff));
    assert_eq!(p.text_fill_color.legacy(), Some(0x0000ff));
    assert_eq!(p.border_color.left, Some(0x0000ff));
    assert_eq!(p.background_color, Some(0x0000ff));
    assert_eq!(
        style(&document, "child").text_fill_color.legacy(),
        Some(0x008000)
    );
    assert_eq!(
        style(&document, "reset").text_fill_color.legacy(),
        Some(0x0000ff)
    );
}

#[test]
fn compatibility_supports_uses_quirks_but_typed_registration_does_not() {
    for mode in [
        DocumentMode::NoQuirks,
        DocumentMode::LimitedQuirks,
        DocumentMode::Quirks,
    ] {
        let mut document = parser::parse(
            "<style>
            @property --bad-length {syntax:'<length>';inherits:false;initial-value:12}
            @property --bad-color {syntax:'<color>';inherits:false;initial-value:abc}
            @property --good {syntax:'<length>';inherits:false;initial-value:3px}
            @supports (width:12) and (color:abc) {p{height:5px}}
            @supports (-webkit-box-sizing:border-box) {p{box-sizing:border-box}}
            p{width:var(--good)}
            </style><p id='p'></p>",
        )
        .unwrap();
        document.set_mode(mode);
        let result = resolve(&document);
        let p = result.styles.get(target(&document, "p")).unwrap();
        assert_eq!(
            p.height,
            (mode == DocumentMode::Quirks).then_some(Length::Px(5.0))
        );
        assert_eq!(p.width, Some(Length::Px(3.0)));
        assert_eq!(p.box_sizing, BoxSizing::BorderBox);
        assert_eq!(
            result
                .stylesheets
                .diagnostics
                .iter()
                .filter(|d| d.message.contains("initial-value does not match"))
                .count(),
            2
        );
    }
}
