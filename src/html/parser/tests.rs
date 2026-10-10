use super::*;
use crate::dom::{Node, NodeKind};

fn body(document: &Document) -> &Node {
    document
        .preorder()
        .find_map(|id| {
            document.node(id).filter(|node| {
                node.as_element()
                    .is_some_and(|element| element.tag_name() == "body")
            })
        })
        .unwrap()
}

fn signature(document: &Document) -> String {
    format!("{document:?}")
}

#[test]
fn streaming_parser_matches_at_every_character_boundary() {
    for source in [
        "",
        "plain text\r",
        "\r\r\n",
        "<!DOCTYPE html><!-- head > note --><p title=\"left > right\">café\r\n世界</p>",
        "<p>before<!-- ignored -->after<br>last</p>",
        "<p title='世界 > café' disabled data-value=hello>text</p>",
        "</unknown><div><p>nested</div>tail",
        "<p",
        "<p title=\"unfinished>",
        "<!-- unfinished",
        "<p=bad>",
        "<$><<p>",
        "<style>p { x: '<'; } </notstyle></STYLE><p>after",
        include_str!("../../../tests/pages/basic.html"),
        include_str!("../../../tests/pages/dom_dump.html"),
        include_str!("../../../tests/pages/styles.html"),
    ] {
        let expected = signature(&parse(source).unwrap());
        for split in source
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(source.len()))
        {
            let mut parser = HtmlParser::new();
            parser.feed(&source[..split]).unwrap();
            parser.feed(&source[split..]).unwrap();
            assert_eq!(
                signature(&parser.finish().unwrap()),
                expected,
                "DOM differed for {source:?} at byte {split}"
            );
        }
        let mut parser = HtmlParser::new();
        for character in source.chars() {
            parser.feed(&character.to_string()).unwrap();
        }
        assert_eq!(signature(&parser.finish().unwrap()), expected);
    }
}

#[test]
fn text_is_visible_before_finish_and_coalesces_without_changing_its_id() {
    let mut parser = HtmlParser::new();
    parser.feed("<p>caf").unwrap();
    let snapshot = parser.document().clone();
    let root = body(&snapshot);
    let paragraph = snapshot.node(root.children()[0]).unwrap();
    let text_id = paragraph.children()[0];
    let revision = parser.revision();
    let count = parser.node_count();
    parser.feed("é").unwrap();
    assert_eq!(parser.node_count(), count);
    assert!(parser.revision() > revision);
    assert!(matches!(snapshot.node(text_id).unwrap().kind(), NodeKind::Text(t) if t == "caf"));
    assert!(
        matches!(parser.document().node(text_id).unwrap().kind(), NodeKind::Text(t) if t == "café")
    );
    assert_eq!(
        signature(&parser.finish().unwrap()),
        signature(&parse("<p>café").unwrap())
    );
}

#[test]
fn partial_attributes_do_not_create_elements_and_snapshots_are_independent() {
    let mut parser = HtmlParser::new();
    parser.feed("<p>visible</p><p title=\"unfinished").unwrap();
    let snapshot = parser.document().clone();
    let expected = signature(&parse("<p>visible</p>").unwrap());
    assert_eq!(signature(&snapshot), expected);
    parser.feed("\">next</p>").unwrap();
    assert_eq!(signature(&snapshot), expected);
    assert_eq!(
        signature(&parser.finish().unwrap()),
        signature(&parse("<p>visible</p><p title=\"unfinished\">next</p>").unwrap())
    );
}

#[test]
fn parses_nested_elements_text_and_attributes() {
    let document = parse(r#"<P CLASS="intro">Hello <a href='/next'>there</a></P>"#).unwrap();
    let root = body(&document);
    let paragraph = document.node(root.children()[0]).unwrap();
    assert_eq!(paragraph.as_element().unwrap().tag_name(), "p");
    assert_eq!(
        paragraph.as_element().unwrap().attribute("class"),
        Some("intro")
    );
    assert_eq!(paragraph.children().len(), 2);
    assert!(matches!(
        document.node(paragraph.children()[0]).unwrap().kind(),
        NodeKind::Text(text) if text == "Hello "
    ));
    let link = document.node(paragraph.children()[1]).unwrap();
    assert_eq!(link.as_element().unwrap().tag_name(), "a");
    assert_eq!(link.as_element().unwrap().attribute("href"), Some("/next"));
    assert!(matches!(
        document.node(link.children()[0]).unwrap().kind(),
        NodeKind::Text(text) if text == "there"
    ));
}

#[test]
fn newlines_unicode_and_quoted_greater_than_are_preserved() {
    let document = parse("<p title=\"left > right\">café\r\n世界\rnext\nlast</p>").unwrap();
    let root = body(&document);
    let paragraph = document.node(root.children()[0]).unwrap();
    assert_eq!(
        paragraph.as_element().unwrap().attribute("title"),
        Some("left > right")
    );
    assert!(matches!(
        document.node(paragraph.children()[0]).unwrap().kind(),
        NodeKind::Text(text) if text == "café\n世界\nnext\nlast"
    ));
}

#[test]
fn boolean_attributes_and_void_elements_preserve_tree_structure() {
    let document = parse("<div>before<input disabled value=hello><br>after</div>").unwrap();
    let root = body(&document);
    let div = document.node(root.children()[0]).unwrap();
    assert_eq!(div.children().len(), 4);
    let input = document
        .node(div.children()[1])
        .unwrap()
        .as_element()
        .unwrap();
    assert_eq!(input.attribute("disabled"), Some(""));
    assert_eq!(input.attribute("value"), Some("hello"));
    assert_eq!(
        document
            .node(div.children()[2])
            .unwrap()
            .as_element()
            .unwrap()
            .tag_name(),
        "br"
    );
    assert!(
        matches!(document.node(div.children()[3]).unwrap().kind(), NodeKind::Text(t) if t == "after")
    );
}

#[test]
fn comments_and_doctypes_are_retained_at_their_insertion_locations() {
    let document = parse("<!DOCTYPE html><!-- head > note --><p>content</p><!-- end -->").unwrap();
    let root = document.node(document.root_id()).unwrap();
    assert_eq!(root.children().len(), 3);
    assert!(
        matches!(document.node(root.children()[0]).unwrap().kind(), NodeKind::Doctype(data) if data.name == "html")
    );
    assert!(
        matches!(document.node(root.children()[1]).unwrap().kind(), NodeKind::Comment(text) if text == " head > note ")
    );
    let body = body(&document);
    assert!(
        matches!(document.node(body.children()[1]).unwrap().kind(), NodeKind::Comment(text) if text == " end ")
    );
}

#[test]
fn malformed_core_markup_recovers_with_diagnostics_instead_of_aborting() {
    for (source, code) in [
        ("<p", "eof-in-tag"),
        ("<p title=\"unfinished>", "eof-in-tag"),
        ("<$>", "invalid-first-character-of-tag-name"),
        ("<<p>", "invalid-first-character-of-tag-name"),
        ("<!-- unfinished", "eof-in-comment"),
    ] {
        let mut parser = HtmlParser::new();
        parser.feed(source).unwrap();
        let parsed = parser.finish_with_diagnostics().unwrap();
        assert_eq!(parsed.errors[0].code, code);
        for split in 0..=source.len() {
            let mut parser = HtmlParser::new();
            parser.feed(&source[..split]).unwrap();
            parser.feed(&source[split..]).unwrap();
            let chunked = parser.finish_with_diagnostics().unwrap();
            assert_eq!(chunked.errors, parsed.errors, "{source:?}, split {split}");
            assert_eq!(signature(&chunked.document), signature(&parsed.document));
        }
    }
    let document = parse("<p=bad>").unwrap();
    let root = body(&document);
    assert_eq!(
        document
            .node(root.children()[0])
            .unwrap()
            .as_element()
            .unwrap()
            .tag_name(),
        "p=bad"
    );
}

#[test]
fn ancestor_closing_and_html_self_closing_behavior_are_preserved() {
    let document = parse("</unknown><div><p/>nested</div>tail").unwrap();
    let root = body(&document);
    let div = document.node(root.children()[0]).unwrap();
    assert_eq!(root.children().len(), 2);
    assert_eq!(div.children().len(), 1);
    let paragraph = document.node(div.children()[0]).unwrap();
    assert_eq!(paragraph.as_element().unwrap().tag_name(), "p");
    assert!(
        matches!(document.node(paragraph.children()[0]).unwrap().kind(), NodeKind::Text(t) if t == "nested")
    );
    assert!(
        matches!(document.node(root.children()[1]).unwrap().kind(), NodeKind::Text(t) if t == "tail")
    );
}
