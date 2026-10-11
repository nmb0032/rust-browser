use crate::{
    css::{self, Rule, media::MediaEnvironment, values::Length},
    html::parser,
};

use super::*;

fn find(document: &crate::dom::Document, name: &str) -> crate::dom::NodeId {
    document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

#[test]
fn nested_media_preserves_source_order_and_inactive_ancestors_gate_descendants() {
    let document = parser::parse(
        "<!doctype html><style>\
        p {color:red; width:10px}\
        @media (min-width:600px) {\
          p {color:green; width:20px}\
          @media (max-width:800px) {p {color:blue; width:30px}}\
          p {width:40px}\
        }\
        @media (max-width:500px) {\
          @media (min-width:600px) {p {color:purple; width:100px}}\
        }\
        p {width:50px}\
        </style><p id='target'>target</p>",
    )
    .unwrap();
    for (width, color) in [(400.0, 0xFF0000), (600.0, 0x0000FF), (801.0, 0x008000)] {
        let result = resolve_with_environment(&document, MediaEnvironment::new(width, 600.0));
        let target = result.styles.get(find(&document, "target")).unwrap();
        assert_eq!(target.color, Some(color), "{width}");
        assert_eq!(target.width, Some(Length::Px(50.0)));
        assert!(result.stylesheets.diagnostics.is_empty());
        assert!(matches!(
            result.stylesheets.stylesheets[0].stylesheet.rules()[1],
            Rule::At(_)
        ));
    }
}

#[test]
fn style_media_attributes_share_query_evaluation_and_initial_font_units() {
    let document = parser::parse(
        "<!doctype html><style>html {font-size:100px} p {color:black}</style>\
        <style media='(max-width:37.5em)'>p {color:red}</style>\
        <style media='only screen and (min-width:37.5rem)'>p {color:blue}</style>\
        <style media='print'>p {color:purple}</style>\
        <style media='only (width:600px)'>p {color:yellow}</style>\
        <p id='target'>target</p>",
    )
    .unwrap();
    for (width, color) in [(400.0, 0xFF0000), (600.0, 0x0000FF), (800.0, 0x0000FF)] {
        let result = resolve_with_environment(&document, MediaEnvironment::new(width, 600.0));
        assert_eq!(
            result.styles.get(find(&document, "target")).unwrap().color,
            Some(color)
        );
        assert_eq!(result.stylesheets.stylesheets.len(), 5);
        assert_eq!(result.stylesheets.diagnostics.len(), 1);
        assert!(
            result.stylesheets.diagnostics[0]
                .message
                .contains("'only' requires a media type")
        );
    }
}

#[test]
fn malformed_unsupported_and_empty_media_never_leak_declarations_and_ast_is_retained() {
    let document = parser::parse(
        "<style>\
        p {color:red}\
        @media (width:), (width:600px) {p {color:blue}}\
        @media not (width:20%) {p {color:purple}}\
        @media not (future-height:600px) {p {color:purple}}\
        @media /*empty*/ {p {color:purple}}\
        @media (width:600px);\
        @media screen {color:purple; @supports (display:grid) {p {color:purple}}}\
        </style><p id='target'>target</p>",
    )
    .unwrap();
    for (width, color) in [(400.0, 0xFF0000), (600.0, 0x0000FF)] {
        let result = resolve_with_environment(&document, MediaEnvironment::new(width, 600.0));
        assert_eq!(
            result.styles.get(find(&document, "target")).unwrap().color,
            Some(color)
        );
        assert_eq!(
            result.stylesheets.stylesheets[0].stylesheet.rules().len(),
            7
        );
        let messages = result
            .stylesheets
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "nonempty query",
            "requires a rule block",
            "unsupported media feature",
            "invalid width media-query length",
            "unsupported @supports declaration",
            "directly inside a conditional rule",
        ] {
            assert!(
                messages.iter().any(|message| message.contains(expected)),
                "{messages:?}"
            );
        }
    }
}

#[test]
fn user_origin_media_queries_honor_environment_importance_and_recover_lists() {
    let document = parser::parse(
        "<style>p {color:green !important}</style><p id='target' style='width:100px'>target</p>",
    )
    .unwrap();
    for (width, color) in [(400.0, 0x008000), (600.0, 0x0000FF)] {
        let result = resolve_with_user_stylesheets_and_environment(
            &document,
            collect_stylesheets(&document),
            &["@media (width:), (min-width:600px) {p {color:blue !important; width:50px}}"],
            MediaEnvironment::new(width, 600.0),
        );
        let target = result.styles.get(find(&document, "target")).unwrap();
        assert_eq!(target.color, Some(color));
        assert_eq!(target.width, Some(Length::Px(100.0)));
        assert_eq!(result.stylesheets.diagnostics.len(), 1);
        assert!(result.stylesheets.diagnostics[0].node_id.is_none());
        assert!(
            result.stylesheets.diagnostics[0]
                .message
                .starts_with("user stylesheet: media query")
        );
    }
    assert_eq!(
        css::media::INITIAL_FONT_SIZE,
        ComputedStyle::default().font_size
    );
}
