use super::*;
use crate::{
    css::values::{Edges, Length},
    dom::{Document, NodeId},
    html::parser,
};

#[test]
fn extreme_font_sizes_are_bounded_with_a_diagnostic() {
    let document = parser::parse("<p id='p' style='font-size:1000000px'>text</p>").unwrap();
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "p")).unwrap().font_size,
        512.0
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 1);
    assert!(
        result.stylesheets.diagnostics[0]
            .message
            .contains("rasterization limit")
    );
}

fn find(document: &Document, id: &str) -> NodeId {
    let mut pending = vec![document.root_id()];
    while let Some(node_id) = pending.pop() {
        let node = document.node(node_id).unwrap();
        if node
            .as_element()
            .is_some_and(|element| element.attribute("id") == Some(id))
        {
            return node_id;
        }
        pending.extend(node.children());
    }
    panic!("missing element {id}");
}

fn styled(source: &str, id: &str) -> ComputedStyle {
    let document = parser::parse(source).unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    result.styles.get(find(&document, id)).unwrap().clone()
}

#[test]
fn collects_style_text_in_document_order_and_reports_malformed_sheets() {
    let document = parser::parse(
        "<style>p {color:red}</style><div><style>p {color:blue}</style></div>\
         <style type='text/plain'>not css</style><style>broken</style>",
    )
    .unwrap();
    let result = collect_stylesheets(&document);
    assert_eq!(result.stylesheets.len(), 3);
    assert_eq!(result.diagnostics.len(), 1);
    let style = styled(
        "<style>p {color:red}</style><style>p {color:blue}</style><p id='p'>text</p>",
        "p",
    );
    assert_eq!(style.color, Some(0x0000FF));
}

#[test]
fn specificity_source_order_and_inline_priority_choose_winning_values() {
    let style = styled(
        "<style>#p {color:green} .c {color:red} p {color:blue; background-color:#abc}\
         #p {color:purple}</style><p id='p' class='c'>text</p>",
        "p",
    );
    assert_eq!(style.color, Some(0x800080));
    assert_eq!(style.background_color, Some(0xAABBCC));
    let inline = styled(
        "<style>#p {color:red}</style><p id='p' style='color:blue; color: green'>text</p>",
        "p",
    );
    assert_eq!(inline.color, Some(0x008000));
    let important = styled(
        "<style>p {color:red !important}</style><p id='p' style='color:blue'>text</p>",
        "p",
    );
    assert_eq!(important.color, Some(0xFF0000));
    let inline_important = styled(
        "<style>#p {color:red !important}</style><p id='p' style='color:blue !important'>text</p>",
        "p",
    );
    assert_eq!(inline_important.color, Some(0x0000FF));
}

#[test]
fn lists_use_the_specificity_of_the_matching_selector_only() {
    let style = styled(
        "<style>#other, p {color:red} .c {color:blue}</style><p id='p' class='c'>text</p>",
        "p",
    );
    assert_eq!(style.color, Some(0x0000FF));
}

#[test]
fn attribute_combinator_and_structural_selectors_reach_the_cascade() {
    let document = parser::parse(
        "<!doctype html><style>\
         main > p[data-kind=notice i]:first-child {color:red}\
         main > p + p:not([hidden], .skip) {color:blue; padding:3px}\
         main > p ~ span:last-child {background-color:yellow}\
         main > p[hidden] {display:none}\
         </style><main><!-- comment -->\
         <p id='first' data-kind='Notice'>first</p> text \
         <p id='next'>next</p><p id='hidden' hidden>hidden</p>\
         <span id='last'>last</span></main>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    assert_eq!(
        result.styles.get(find(&document, "first")).unwrap().color,
        Some(0xFF0000)
    );
    let next = result.styles.get(find(&document, "next")).unwrap();
    assert_eq!(next.color, Some(0x0000FF));
    assert_eq!(next.padding, Edges::all(Length::Px(3.0)));
    assert_eq!(
        result
            .styles
            .get(find(&document, "hidden"))
            .unwrap()
            .display,
        Display::None
    );
    assert_eq!(
        result
            .styles
            .get(find(&document, "last"))
            .unwrap()
            .background_color,
        Some(0xFFFF00)
    );
}

#[test]
fn negation_argument_specificity_and_matching_list_specificity_choose_winners() {
    let style = styled(
        "<!doctype html><style>\
         p:not(.excluded, #unused) {color:red}\
         .notice {color:blue}\
         #unused, p:not([hidden]) {background-color:yellow}\
         .notice {background-color:green}\
         </style><p id='p' class='notice'>text</p>",
        "p",
    );
    assert_eq!(style.color, Some(0xFF0000));
    assert_eq!(style.background_color, Some(0xFFFF00));
}

#[test]
fn invalid_selector_lists_and_negations_drop_only_their_rules_with_diagnostics() {
    let document = parser::parse(
        "<!doctype html><style>\
         p {color:red}\
         p, [data-x=] {color:blue}\
         p:not(.excluded, :unsupported) {color:purple}\
         p[data-x]:not(.excluded) {padding:4px}\
         </style><p id='p' data-x>text</p>",
    )
    .unwrap();
    let result = resolve(&document);
    let style = result.styles.get(find(&document, "p")).unwrap();
    assert_eq!(style.color, Some(0xFF0000));
    assert_eq!(style.padding, Edges::all(Length::Px(4.0)));
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
    assert!(result.stylesheets.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .message
            .contains("invalid or unsupported selector")
    }));
}

#[test]
fn invalid_values_do_not_displace_valid_declarations_and_errors_are_available() {
    let document = parser::parse(
        "<style>p {color: red; color: bad; padding: 1px -2px; width: -5px}\
         p:hover {color:blue} @media print {p {color:yellow}}</style><p id='p'>text</p>",
    )
    .unwrap();
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "p")).unwrap().color,
        Some(0xFF0000)
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 5);
}

#[test]
fn syntax_recovery_keeps_valid_embedded_and_inline_styles_and_rule_order() {
    let document = parser::parse(
        "<style>p {color:red; broken; width:1px}\
         p, :bad {color:purple} p {color:blue; padding:2px; padding:bad}</style>\
         <p id='p' style='margin:3px; broken; color:green; width:4px'>text</p>",
    )
    .unwrap();
    let result = resolve(&document);
    let style = result.styles.get(find(&document, "p")).unwrap();
    assert_eq!(style.color, Some(0x008000));
    assert_eq!(style.width, Some(Length::Px(4.0)));
    assert_eq!(style.padding, Edges::all(Length::Px(2.0)));
    assert_eq!(style.margin, Edges::all(Length::Px(3.0)));
    assert_eq!(result.stylesheets.diagnostics.len(), 4);
    assert!(result.stylesheets.diagnostics.iter().any(|diagnostic| {
        diagnostic.node_id == Some(find(&document, "p"))
            && diagnostic
                .message
                .starts_with("inline style: CSS parse error")
    }));
}

#[test]
fn recovered_styles_report_omitted_syntax_diagnostics() {
    let limit = crate::css::tokenizer::MAX_PARSE_ERRORS;
    let document = parser::parse(&format!(
        "<style>p {{{}color:red}}</style><p id='p' style='{}width:2px'>text</p>",
        "broken;".repeat(limit + 2),
        "broken;".repeat(limit + 3)
    ))
    .unwrap();
    let result = resolve(&document);
    assert_eq!(result.stylesheets.diagnostics.len(), 2 * (limit + 1));
    assert_eq!(
        result.styles.get(find(&document, "p")).unwrap().color,
        Some(0xFF0000)
    );
    assert_eq!(
        result.styles.get(find(&document, "p")).unwrap().width,
        Some(Length::Px(2.0))
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message == "2 additional CSS parse errors omitted" })
    );
    assert!(result.stylesheets.diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "inline style: 3 additional CSS parse errors omitted"
    }));
}

#[test]
fn inheritance_and_relative_sizes_are_computed_before_dependent_lengths() {
    let document = parser::parse(
        "<div style='color:navy; font-size:20px; font-family:serif; background-color:yellow'>\
         <span id='p' style='padding:1em; font-size:150%; border-width:0.1em'>text</span>\
         <span id='other'>text</span></div>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(result.stylesheets.diagnostics.is_empty());
    let style = result.styles.get(find(&document, "p")).unwrap();
    assert_eq!(style.font_size, 30.0);
    assert_eq!(style.color, Some(0x000080));
    assert_eq!(style.font_family, "serif");
    assert_eq!(style.background_color, None);
    assert_eq!(style.padding, Edges::all(Length::Px(30.0)));
    assert_eq!(style.border_width, Edges::all(3.0));
    let sibling = result.styles.get(find(&document, "other")).unwrap();
    assert_eq!(sibling.font_size, 20.0);
    assert_eq!(sibling.padding, Edges::default());
    let text = document.node(find(&document, "p")).unwrap().children()[0];
    assert_eq!(result.styles.get(text).unwrap().color, style.color);
}

#[test]
fn shorthand_and_longhand_declarations_cascade_per_edge() {
    let style = styled(
        "<p id='p' style='margin:1px 2px 3px 4px; margin-left:9px;\
         padding-top:8px; padding:10%; border-width:1px 2px; border-left-width:4px;\
         color:red; border-color:currentcolor blue transparent'>text</p>",
        "p",
    );
    assert_eq!(
        style.margin,
        Edges {
            top: Length::Px(1.0),
            right: Length::Px(2.0),
            bottom: Length::Px(3.0),
            left: Length::Px(9.0)
        }
    );
    assert_eq!(style.padding, Edges::all(Length::Percent(10.0)));
    assert_eq!(style.border_width.left, 4.0);
    assert_eq!(style.border_color.top, Some(0xFF0000));
    assert_eq!(style.border_color.right, Some(0x0000FF));
    assert_eq!(style.border_color.bottom, None);
}

#[test]
fn wide_keywords_and_currentcolor_use_computed_values() {
    let style = styled(
        "<div style='color:green; font-size:20px; margin:1em'>\
         <p id='p' style='font-size:2em; margin:inherit; color:unset;\
         padding:initial; background-color:currentcolor; border-color:initial'>text</p></div>",
        "p",
    );
    assert_eq!(style.font_size, 40.0);
    assert_eq!(style.margin, Edges::all(Length::Px(20.0)));
    assert_eq!(style.background_color, Some(0x008000));
    assert_eq!(style.border_color, Edges::all(Some(0x008000)));
}

#[test]
fn ua_styles_are_overridden_by_author_rules_without_losing_emphasis_defaults() {
    let style = styled(
        "<style>h1 {display:inline; font-size:12px; margin:0}</style><h1 id='p'>text</h1>",
        "p",
    );
    assert_eq!(style.display, Display::Inline);
    assert_eq!(style.font_size, 12.0);
    assert_eq!(style.margin, Edges::default());
    assert!(style.bold);
    let emphasis = styled("<strong><em id='p'>text</em></strong>", "p");
    assert!(emphasis.bold && emphasis.italic);
}

#[test]
fn inline_declaration_parser_handles_trivia_strings_and_missing_final_semicolon() {
    let declarations = crate::css::parse_declarations(
        "; /* leading */ COLOR: red; font-family: 'A;B'; padding: 2px !important",
    )
    .unwrap();
    assert_eq!(declarations.len(), 3);
    assert_eq!(declarations[1].value(), "'A;B'");
    assert!(declarations[2].is_important());
    assert!(crate::css::parse_declarations("color red").is_err());
    assert!(crate::css::parse_declarations("color:red} p {color:blue}").is_err());
}

#[test]
fn style_raw_text_survives_all_character_chunk_boundaries() {
    let source = "<style>/* <tag> café */ p {font-family:'a<b'; color:red} </stylesheet> </STYLE >\
                  <p id='p'>text</p>";
    let expected = parser::parse(source).unwrap();
    for (boundary, _) in source
        .char_indices()
        .chain(std::iter::once((source.len(), '\0')))
    {
        let mut parser = parser::HtmlParser::new();
        parser.feed(&source[..boundary]).unwrap();
        parser.feed(&source[boundary..]).unwrap();
        let actual = parser.finish().unwrap();
        assert_eq!(
            format!("{actual:?}"),
            format!("{expected:?}"),
            "boundary {boundary}"
        );
    }
    let collected = collect_stylesheets(&expected);
    // The fake closing tag is raw text, so CSS rejects it rather than HTML creating nodes.
    assert_eq!(collected.diagnostics.len(), 1);
}

#[test]
fn a_late_stylesheet_restyles_existing_elements_in_progressive_documents() {
    let mut parser = parser::HtmlParser::new();
    parser.feed("<p id='p'>text</p><style>p {color:").unwrap();
    let before = resolve(parser.document());
    let id = find(parser.document(), "p");
    assert_eq!(before.styles.get(id).unwrap().color, Some(0));
    assert!(before.stylesheets.diagnostics.is_empty());
    parser.feed("red}</style>").unwrap();
    let after = resolve(parser.document());
    assert_eq!(after.styles.get(id).unwrap().color, Some(0xFF0000));
    assert!(after.stylesheets.diagnostics.is_empty());
}
