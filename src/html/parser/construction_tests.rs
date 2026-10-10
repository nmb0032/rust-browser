use super::*;
use crate::dom::{Namespace, NodeId, NodeKind};

fn find(document: &Document, name: &str) -> NodeId {
    document
        .preorder()
        .find(|id| {
            document
                .node(*id)
                .and_then(|node| node.as_element())
                .is_some_and(|element| element.tag_name() == name)
        })
        .unwrap()
}

#[test]
fn selectedcontent_clones_the_selected_option_and_skips_disabled_ancestors() {
    for (options, expected) in [
        ("<option>x<i>y</i><option>z", "xy"),
        ("<option>x<option selected><b>y</b>", "y"),
        (
            "<option disabled>x<optgroup disabled><option>y</optgroup><option>z",
            "z",
        ),
        ("<option selected>x<option selected>y", "y"),
    ] {
        let source = format!("<select><button><selectedcontent></button>{options}");
        for size in [1, 7, usize::MAX] {
            let mut parser = HtmlParser::new();
            let starts = source
                .char_indices()
                .map(|(i, _)| i)
                .step_by(size)
                .chain(std::iter::once(source.len()))
                .collect::<Vec<_>>();
            for pair in starts.windows(2) {
                parser.feed(&source[pair[0]..pair[1]]).unwrap();
            }
            let document = parser.finish().unwrap();
            let target = find(&document, "selectedcontent");
            let mut pending = vec![target];
            let mut text = String::new();
            while let Some(id) = pending.pop() {
                let node = document.node(id).unwrap();
                if let NodeKind::Text(value) = node.kind() {
                    text.push_str(value);
                }
                pending.extend(node.children().iter().rev().copied());
            }
            assert_eq!(text, expected);
            document.validate().unwrap();
        }
    }
    for source in [
        "<select multiple><selectedcontent>author</selectedcontent><option>x",
        "<select><option><selectedcontent>author</selectedcontent>",
        "<select><selectedcontent><selectedcontent>author</selectedcontent></selectedcontent>",
    ] {
        parse(source).unwrap().validate().unwrap();
    }
}

#[test]
fn live_and_finished_diagnostics_include_tree_recovery_and_omitted_counts() {
    let mut parser = HtmlParser::new();
    parser.feed("<p><div/>").unwrap();
    assert!(
        parser
            .errors()
            .iter()
            .any(|error| error.code == "missing-doctype")
    );
    assert!(
        parser
            .errors()
            .iter()
            .any(|error| error.code == "non-void-html-element-start-tag-with-trailing-solidus")
    );
    parser.feed(&"</unknown>".repeat(1000)).unwrap();
    assert_eq!(
        parser.errors().len(),
        crate::html::tokenizer::MAX_PARSE_ERRORS
    );
    assert!(parser.dropped_errors() > 0);
    let parsed = parser.finish_with_diagnostics().unwrap();
    parsed.document.validate().unwrap();
    assert_eq!(
        parsed.errors.len(),
        crate::html::tokenizer::MAX_PARSE_ERRORS
    );
    assert!(parsed.dropped_errors > 0);
}

#[test]
fn self_closing_flags_are_checked_once_in_every_insertion_mode() {
    for (markup, expected) in [
        ("<html/>", 1),
        ("<head/>", 1),
        ("<title/>x</title>", 1),
        ("<body/>", 1),
        ("<table/><tbody/><tr/><td/>x</table>", 4),
        ("<table><colgroup/><col/></table>", 1),
        ("<frameset/><frame/></frameset>", 1),
        ("<head><head/></head>", 1),
        ("<meta/><br/><input/><hr/>", 0),
        ("<svg><g/></svg><math><mi/></math>", 0),
    ] {
        let source = format!("<!doctype html>{markup}");
        for split in 0..=source.len() {
            let mut parser = HtmlParser::new();
            parser.feed(&source[..split]).unwrap();
            parser.feed(&source[split..]).unwrap();
            let parsed = parser.finish_with_diagnostics().unwrap();
            assert_eq!(
                parsed
                    .errors
                    .iter()
                    .filter(|error| error.code
                        == "non-void-html-element-start-tag-with-trailing-solidus")
                    .count(),
                expected,
                "{source:?}, split {split}"
            );
            parsed.document.validate().unwrap();
        }
    }
}

#[test]
fn pending_table_errors_keep_reference_null_and_newline_source_positions() {
    use crate::html::tokenizer::{ParseError, SourcePosition};
    let source = "<!doctype html><table> \0&#88;\r\nY</table>";
    let expected = [
        ("non-space-in-table", 22, 1, 23),
        ("unexpected-token-in-table", 22, 1, 23),
        ("unexpected-null-character", 23, 1, 24),
        ("unexpected-null-in-table", 23, 1, 24),
        ("unexpected-token-in-table", 29, 1, 30),
        ("unexpected-token-in-table", 29, 1, 30),
        ("unexpected-token-in-table", 31, 2, 1),
    ]
    .map(|(code, byte_offset, line, column)| ParseError {
        code,
        position: SourcePosition {
            byte_offset,
            line,
            column,
        },
    });
    for split in 0..=source.len() {
        let mut parser = HtmlParser::new();
        parser.feed(&source[..split]).unwrap();
        parser.feed(&source[split..]).unwrap();
        let parsed = parser.finish_with_diagnostics().unwrap();
        assert_eq!(parsed.errors, expected, "split {split}");
        assert_eq!(parsed.dropped_errors, 0);
        parsed.document.validate().unwrap();
    }
    let source = format!("<!doctype html><table>{}</table>", "x".repeat(1000));
    let mut parser = HtmlParser::new();
    parser.feed(&source).unwrap();
    let parsed = parser.finish_with_diagnostics().unwrap();
    assert_eq!(parsed.errors.len(), MAX_PARSE_ERRORS);
    assert_eq!(parsed.errors.len() + parsed.dropped_errors, 1001);
}

#[test]
fn foreign_fragment_end_tags_stop_at_the_root_or_reprocess_in_html() {
    let mut context = FragmentContext::html("path");
    context.element = ElementData::namespaced("path".into(), Vec::new(), Namespace::Svg, None);
    for (source, expected) in [
        ("</path>X", vec!["misnested-foreign-end-tag"]),
        (
            "<g></path>X",
            vec![
                "misnested-foreign-end-tag",
                "unexpected-end-tag",
                "eof-with-open-elements",
            ],
        ),
    ] {
        let mut parser = HtmlParser::fragment(context.clone()).unwrap();
        parser.feed(source).unwrap();
        let parsed = parser.finish_with_diagnostics().unwrap();
        assert_eq!(
            parsed
                .errors
                .iter()
                .map(|error| error.code)
                .collect::<Vec<_>>(),
            expected
        );
        parsed.document.validate().unwrap();
    }
}

#[test]
fn forms_in_template_tables_do_not_replace_the_outer_form_pointer() {
    let source = "<!doctype html><form id=outer><template><table><form id=inner><tr><td>x</td></tr></table></template></form>";
    let document = parse(source).unwrap();
    let contents = document
        .node(find(&document, "template"))
        .unwrap()
        .as_element()
        .unwrap()
        .template_contents()
        .unwrap();
    let table = document.node(contents).unwrap().children()[0];
    let children = document.node(table).unwrap().children();
    let inner = document.node(children[0]).unwrap().as_element().unwrap();
    assert_eq!(inner.tag_name(), "form");
    assert_eq!(inner.attribute("id"), Some("inner"));
    assert!(document.node(children[0]).unwrap().children().is_empty());
    let mut parser = HtmlParser::new();
    parser.feed(source).unwrap();
    let parsed = parser.finish_with_diagnostics().unwrap();
    assert_eq!(
        parsed
            .errors
            .iter()
            .map(|error| error.code)
            .collect::<Vec<_>>(),
        vec!["form-in-table"]
    );
    parsed.document.validate().unwrap();
}

#[test]
fn fragments_use_form_ancestors_and_hostile_nesting_keeps_arena_links_valid() {
    let mut context = FragmentContext::html("div");
    context
        .ancestors
        .push(ElementData::new("form".into(), Vec::new()));
    let document = parse_fragment("<form><input></form>", context).unwrap();
    assert!(matches!(
        document.node(document.root_id()).unwrap().kind(),
        NodeKind::DocumentFragment
    ));
    assert!(!document.preorder().any(|id| {
        document
            .node(id)
            .and_then(|node| node.as_element())
            .is_some_and(|element| element.tag_name() == "form")
    }));
    document.validate().unwrap();
    for source in [
        format!("{}text{}", "<div>".repeat(10000), "</div>".repeat(10000)),
        format!("<table>{}</table>", "x".repeat(100000)),
        format!("{}{}", "<b><i>".repeat(200), "</b></i>".repeat(200)),
    ] {
        let document = parse(&source).unwrap();
        document.validate().unwrap();
    }
    let document = parse("<template><svg><g/></svg></template>").unwrap();
    let dump = crate::devtools::format_dom(&document);
    assert!(dump.contains("#document-fragment"));
    assert!(dump.contains("<svg g>"));
}

#[test]
fn document_modes_and_implied_paragraphs_match_html_rules() {
    for (source, mode) in [
        ("Hello", DocumentMode::Quirks),
        ("<!doctype html>Hello", DocumentMode::NoQuirks),
        (
            "<!doctype html PUBLIC '-//W3C//DTD XHTML 1.0 Transitional//EN' 'x'>",
            DocumentMode::LimitedQuirks,
        ),
        (
            "<!doctype html PUBLIC '-//W3C//DTD HTML 4.01 Transitional//EN'>",
            DocumentMode::Quirks,
        ),
    ] {
        assert_eq!(parse(source).unwrap().mode(), mode);
    }
    let document = parse("<p>a<p>b<ul><li>one<li>two</ul>").unwrap();
    let body = document.node(find(&document, "body")).unwrap();
    assert_eq!(body.children().len(), 3);
    for child in &body.children()[..2] {
        assert_eq!(
            document
                .node(*child)
                .unwrap()
                .as_element()
                .unwrap()
                .tag_name(),
            "p"
        );
    }
    assert_eq!(
        document
            .node(find(&document, "ul"))
            .unwrap()
            .children()
            .len(),
        2
    );
}

#[test]
fn adoption_agency_reparents_nodes_and_foster_parenting_preserves_order() {
    let document = parse("<b>before<p>inside</b>after</p>").unwrap();
    let body = document.node(find(&document, "body")).unwrap();
    let paragraph = document.node(find(&document, "p")).unwrap();
    assert_eq!(body.children().len(), 2);
    assert_eq!(
        document
            .node(paragraph.children()[0])
            .unwrap()
            .as_element()
            .unwrap()
            .tag_name(),
        "b"
    );
    assert!(
        matches!(document.node(paragraph.children()[1]).unwrap().kind(), NodeKind::Text(text) if text == "after")
    );
    let document = parse("<table>before<tr><td>cell</table>after").unwrap();
    let body = document.node(find(&document, "body")).unwrap();
    assert!(
        matches!(document.node(body.children()[0]).unwrap().kind(), NodeKind::Text(text) if text == "before")
    );
    assert_eq!(
        document.node(find(&document, "tbody")).unwrap().parent(),
        Some(find(&document, "table"))
    );
}

#[test]
fn foreign_content_and_template_contents_have_distinct_roots() {
    let document = parse("<svg viewbox='0 0 1 1'><lineargradient/><foreignObject><p>html</p></foreignObject><![CDATA[x<y]]></svg><template><p>hidden</p></template>").unwrap();
    let svg = document
        .node(find(&document, "svg"))
        .unwrap()
        .as_element()
        .unwrap();
    assert_eq!(svg.namespace(), &Namespace::Svg);
    assert_eq!(svg.attribute("viewBox"), Some("0 0 1 1"));
    assert_eq!(svg.attribute("viewbox"), None);
    assert_eq!(
        document
            .node(find(&document, "linearGradient"))
            .unwrap()
            .as_element()
            .unwrap()
            .namespace(),
        &Namespace::Svg
    );
    assert_eq!(
        document
            .node(find(&document, "p"))
            .unwrap()
            .as_element()
            .unwrap()
            .namespace(),
        &Namespace::Html
    );
    let template = document.node(find(&document, "template")).unwrap();
    assert!(template.children().is_empty());
    let content = template.as_element().unwrap().template_contents().unwrap();
    assert!(matches!(
        document.node(content).unwrap().kind(),
        NodeKind::DocumentFragment
    ));
    assert_eq!(document.node(content).unwrap().children().len(), 1);
}

#[test]
fn rcdata_script_escapes_and_fragment_contexts_remain_literal_where_required() {
    let document = parse(
        "<textarea>\n&lt;p&gt;&amp;</textarea><script><!--<script></script>--></script><p>after",
    )
    .unwrap();
    let textarea = document.node(find(&document, "textarea")).unwrap();
    assert!(
        matches!(document.node(textarea.children()[0]).unwrap().kind(), NodeKind::Text(text) if text == "<p>&")
    );
    let script = document.node(find(&document, "script")).unwrap();
    assert!(
        matches!(document.node(script.children()[0]).unwrap().kind(), NodeKind::Text(text) if text == "<!--<script></script>-->")
    );
    let fragment = parse_fragment("<tr><td>cell", FragmentContext::html("table")).unwrap();
    assert_eq!(
        fragment.node(fragment.root_id()).unwrap().children().len(),
        1
    );
    assert_eq!(
        fragment.node(find(&fragment, "tbody")).unwrap().parent(),
        Some(fragment.root_id())
    );
    let fragment =
        parse_fragment("</textarea>&amp;<b>", FragmentContext::html("textarea")).unwrap();
    assert!(
        matches!(fragment.node(fragment.node(fragment.root_id()).unwrap().children()[0]).unwrap().kind(), NodeKind::Text(text) if text == "</textarea>&<b>")
    );
}

#[test]
fn scripting_flag_controls_noscript_and_processing_instructions_follow_living_standard() {
    for scripting in [false, true] {
        let mut parser = HtmlParser::with_options(ParserOptions { scripting });
        parser
            .feed("<!doctype html><body><noscript><b>fallback</b></noscript>")
            .unwrap();
        let document = parser.finish().unwrap();
        let noscript = document.node(find(&document, "noscript")).unwrap();
        assert_eq!(
            matches!(
                document.node(noscript.children()[0]).unwrap().kind(),
                NodeKind::Text(_)
            ),
            scripting
        );
    }
    let document = parse("<?build ready?><!doctype html><p>x<?note value?>").unwrap();
    let root = document.node(document.root_id()).unwrap();
    assert!(
        matches!(document.node(root.children()[0]).unwrap().kind(), NodeKind::ProcessingInstruction { target, data } if target == "build" && data == "ready")
    );
}
