use super::*;
use crate::dom::{Attribute, Namespace};

pub(super) fn append(
    document: &mut Document,
    parent: NodeId,
    tag: &str,
    attributes: &[(&str, &str)],
) -> NodeId {
    document
        .append_element(
            parent,
            ElementData::new(
                tag.into(),
                attributes
                    .iter()
                    .map(|(name, value)| Attribute::new((*name).into(), (*value).into()))
                    .collect(),
            ),
        )
        .unwrap()
}

pub(super) fn matches(document: &Document, node: NodeId, source: &str) -> bool {
    let selectors = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    selectors
        .iter()
        .any(|selector| selector.matches(document, node))
}

#[test]
fn computes_lexicographic_specificity_for_compounds_and_lists() {
    let selectors = parse("main .notice p#hero.active, *, h1").unwrap();
    assert_eq!(
        selectors[0].specificity(),
        Specificity {
            ids: 1,
            classes: 2,
            types: 2
        }
    );
    assert_eq!(selectors[1].specificity(), Specificity::default());
    assert!(selectors[0].specificity() > selectors[2].specificity());
    assert_eq!(parse("p/**/.notice").unwrap()[0].specificity().classes, 1);
}

#[test]
fn attributes_and_pseudo_classes_count_as_classes_and_combinators_add_nothing() {
    for source in [
        "main > p[data-x]:first-child + a.notice",
        "main p[data-x]:first-child ~ a.notice",
    ] {
        assert_eq!(
            parse(source).unwrap()[0].specificity(),
            Specificity {
                ids: 0,
                classes: 3,
                types: 3,
            }
        );
    }
}

#[test]
fn negation_uses_the_maximum_argument_specificity_without_an_extra_class() {
    for (source, expected) in [
        (
            "p:not(*)",
            Specificity {
                ids: 0,
                classes: 0,
                types: 1,
            },
        ),
        (
            "p:not(.a, #unused)",
            Specificity {
                ids: 1,
                classes: 0,
                types: 1,
            },
        ),
        (
            ":not(main > p.notice, .a.b)",
            Specificity {
                ids: 0,
                classes: 2,
                types: 0,
            },
        ),
        (
            ":not(:not(#x)):not([x])",
            Specificity {
                ids: 1,
                classes: 1,
                types: 0,
            },
        ),
    ] {
        assert_eq!(
            parse(source).unwrap()[0].specificity(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn matches_ancestors_not_siblings_and_preserves_class_and_id_case() {
    let mut document = Document::new();
    let root = document.root_id();
    let outer = append(&mut document, root, "main", &[("class", "notice extra")]);
    let inner = append(&mut document, outer, "div", &[]);
    let paragraph = append(&mut document, inner, "p", &[("id", "Hero")]);
    assert!(matches(&document, paragraph, "MAIN.notice #Hero"));
    assert!(!matches(&document, paragraph, "main #hero"));
    assert!(!matches(&document, paragraph, ".not"));
    let sibling = append(&mut document, root, "p", &[]);
    assert!(!matches(&document, sibling, "main p"));
    assert!(!matches(&document, root, "*"));
}

#[test]
fn attribute_operators_match_presence_exact_words_dash_prefix_suffix_and_substring() {
    let mut document = Document::new();
    let root = document.root_id();
    let element = append(
        &mut document,
        root,
        "a",
        &[
            ("data-label", "hello, world > [x]"),
            ("data-words", " one\ttwo\nthree\rfour\x0Cfive "),
            ("data-language", "en-US"),
            ("data-empty", ""),
            ("href", "/dir/file.html"),
        ],
    );
    for source in [
        "[data-empty]",
        "[DATA-LABEL]",
        "[data-label='hello, world > [x]']",
        "[data-words~=one]",
        "[data-words~='two']",
        "[data-words~=three]",
        "[data-words~=four]",
        "[data-words~=five]",
        "[data-language|=en]",
        "[data-language|='en-US']",
        "[href^='/dir/']",
        "[href$='.html']",
        "[href*='file']",
        "[data-empty='']",
        "[data-empty|='']",
        "a[href][data-language|=en]",
    ] {
        assert!(matches(&document, element, source), "{source}");
    }
    for source in [
        "[missing]",
        "[missing='']",
        "[data-label=hello]",
        "[data-words~=on]",
        "[data-words~='one two']",
        "[data-words~='']",
        "[data-language|=e]",
        "[data-language|=en-US-extra]",
        "[href^='file']",
        "[href$='/dir/']",
        "[href*='missing']",
        "[href^='']",
        "[href$='']",
        "[href*='']",
        "[data-empty~='']",
    ] {
        assert!(!matches(&document, element, source), "{source}");
    }
}

#[test]
fn attribute_flags_are_ascii_only_and_override_html_value_case_rules() {
    let mut document = Document::new();
    let root = document.root_id();
    let element = append(
        &mut document,
        root,
        "div",
        &[
            ("data-value", "AbC-\u{e9}"),
            ("type", "TEXT"),
            ("class", "Hero"),
        ],
    );
    for source in [
        "[data-value='abc-\u{e9}' i]",
        "[data-value^=ab I]",
        "[data-value$='C-\u{e9}' i]",
        "[data-value*='Bc' i]",
        "[data-value|=abc i]",
        "[class~=hero i]",
        "[TYPE=text]",
        "[type=text i]",
        "[type=TEXT s]",
    ] {
        assert!(matches(&document, element, source), "{source}");
    }
    for source in [
        "[data-value='abc-\u{e9}']",
        "[data-value='abc-\u{e9}' s]",
        "[data-value='abc-\u{c9}' i]",
        "[data-value*='\u{c9}' i]",
        "[class~=hero]",
        "[type=text s]",
        "[type=text S]",
    ] {
        assert!(!matches(&document, element, source), "{source}");
    }
}

#[test]
fn selectors_use_css_whitespace_not_all_ascii_whitespace() {
    let mut document = Document::new();
    let root = document.root_id();
    let element = append(&mut document, root, "p", &[("class", "a\x0Bb c\x0Cd")]);
    assert!(!matches(&document, element, ".a"));
    assert!(!matches(&document, element, "[class~=a]"));
    assert!(matches(&document, element, ".a\\b b"));
    assert!(matches(&document, element, ".c.d"));
    assert!(matches(&document, element, "[class~=d]"));
    assert!(parse("p\x0Bspan").is_err());
}

#[test]
fn comments_and_escapes_follow_token_boundaries_instead_of_string_splitting() {
    let mut document = Document::new();
    let root = document.root_id();
    let element = append(
        &mut document,
        root,
        "p",
        &[
            ("id", "123"),
            ("class", "a,b a+b a.b"),
            ("data-x", "a/*b*/,c"),
        ],
    );
    for source in [
        "p/**/.a\\,b",
        "#\\31 23",
        ".a\\+b",
        ".a\\.b",
        "[data\\2d x='a/*b*/,c']",
        "[data-x='a/*b*/,c'/**/s]",
        ":n\\6f t(.missing)",
    ] {
        assert!(matches(&document, element, source), "{source}");
    }
    assert_eq!(parse("[data-x='a,b'], .a\\,b").unwrap().len(), 2);
    assert!(parse("p/**/span").is_err());
    assert!(parse(".a/**/b").is_err());
    assert!(parse("p/*").is_err());
    assert!(parse("[data-x='unterminated").is_err());
}

#[test]
fn child_combinators_require_the_immediate_parent_and_accept_optional_whitespace() {
    let mut document = Document::new();
    let root = document.root_id();
    let outer = append(&mut document, root, "main", &[]);
    let inner = append(&mut document, outer, "div", &[]);
    let paragraph = append(&mut document, inner, "p", &[]);
    for source in ["main > div > p", "main>div>p", "main/**/>/**/div >p"] {
        assert!(matches(&document, paragraph, source), "{source}");
    }
    assert!(matches(&document, paragraph, "main p"));
    assert!(!matches(&document, paragraph, "main > p"));
    assert!(!matches(&document, inner, "p > div"));
}

#[test]
fn sibling_combinators_ignore_non_elements_but_not_intervening_elements() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    let first = append(&mut document, parent, "h1", &[]);
    document.append_text(parent, " text ".into()).unwrap();
    document.append_comment(parent, "comment".into()).unwrap();
    document
        .append_processing_instruction(parent, "pi".into(), "".into())
        .unwrap();
    let next = append(&mut document, parent, "p", &[]);
    let last = append(&mut document, parent, "span", &[]);
    let other_parent = append(&mut document, root, "div", &[]);
    let unrelated = append(&mut document, other_parent, "p", &[]);
    for source in ["h1 + p", "h1+p", "main > h1/**/+/**/p"] {
        assert!(matches(&document, next, source), "{source}");
    }
    assert!(matches(&document, last, "h1 ~ span"));
    assert!(matches(&document, last, "h1~p+span"));
    assert!(!matches(&document, last, "h1 + span"));
    assert!(!matches(&document, first, "p ~ h1"));
    assert!(!matches(&document, unrelated, "h1 ~ p"));
    assert!(!matches(&document, unrelated, "h1 + p"));
}

#[test]
fn mixed_combinators_backtrack_to_an_earlier_ancestor_or_sibling() {
    let mut document = Document::new();
    let root = document.root_id();
    let a = append(&mut document, root, "div", &[("class", "a")]);
    let outer_b = append(&mut document, a, "div", &[("class", "b")]);
    let inner_b = append(&mut document, outer_b, "div", &[("class", "b")]);
    let target = append(&mut document, inner_b, "p", &[("class", "target")]);
    assert!(matches(&document, target, ".a > .b .target"));
    assert!(!matches(&document, target, ".a > .b > .target"));
    assert!(matches(
        &document,
        target,
        ":not(.missing, .a > .b > .target)"
    ));
    assert!(!matches(&document, target, ":not(.a > .b .target)"));

    let parent = append(&mut document, root, "section", &[]);
    append(&mut document, parent, "i", &[("class", "a")]);
    append(&mut document, parent, "i", &[("class", "b")]);
    append(&mut document, parent, "i", &[]);
    append(&mut document, parent, "i", &[("class", "b")]);
    let target = append(&mut document, parent, "p", &[("class", "target")]);
    assert!(matches(&document, target, ".a + .b ~ .target"));
    assert!(!matches(&document, target, ".a + .b + .target"));
}

#[test]
fn structural_child_pseudo_classes_count_only_element_siblings() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    document.append_text(parent, "leading".into()).unwrap();
    document.append_comment(parent, "comment".into()).unwrap();
    let first = append(&mut document, parent, "p", &[]);
    let middle = append(&mut document, parent, "span", &[]);
    let last = append(&mut document, parent, "p", &[]);
    document.append_text(parent, "trailing".into()).unwrap();
    for (node, first_expected, last_expected) in [
        (first, true, false),
        (middle, false, false),
        (last, false, true),
    ] {
        assert_eq!(matches(&document, node, ":FIRST-child"), first_expected);
        assert_eq!(matches(&document, node, ":last-child"), last_expected);
        assert!(!matches(&document, node, ":only-child"));
    }
    let parent = append(&mut document, root, "div", &[]);
    document.append_comment(parent, "comment".into()).unwrap();
    let only = append(&mut document, parent, "p", &[]);
    assert!(matches(
        &document,
        only,
        ":first-child:last-child:only-child"
    ));
    document.detach(only).unwrap();
    assert!(matches(
        &document,
        only,
        ":first-child:last-child:only-child"
    ));
    assert!(!matches(&document, only, ":root"));
}

#[test]
fn type_positions_compare_both_namespace_and_tag() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    let first = append(&mut document, parent, "p", &[]);
    let only_span = append(&mut document, parent, "span", &[]);
    let svg_p = document
        .append_element(
            parent,
            ElementData::namespaced("p".into(), vec![], Namespace::Svg, None),
        )
        .unwrap();
    let last = append(&mut document, parent, "p", &[]);
    assert!(matches(&document, first, ":first-of-type"));
    assert!(!matches(&document, last, ":first-of-type"));
    assert!(matches(&document, last, ":last-of-type"));
    assert!(!matches(&document, first, ":last-of-type"));
    assert!(!matches(&document, first, ":only-of-type"));
    assert!(matches(&document, only_span, ":only-of-type"));
    assert!(matches(&document, svg_p, ":only-of-type"));
}

#[test]
fn root_and_empty_distinguish_document_fragments_elements_text_and_comments() {
    let mut document = Document::new();
    let root = document.root_id();
    document
        .append_doctype(root, crate::dom::DoctypeData::default())
        .unwrap();
    let html = append(&mut document, root, "html", &[]);
    let empty = append(&mut document, html, "div", &[]);
    document.append_comment(empty, "comment".into()).unwrap();
    document.append_text(empty, String::new()).unwrap();
    assert!(matches(&document, html, ":root"));
    assert!(!matches(&document, root, ":root"));
    assert!(!matches(&document, empty, ":root"));
    assert!(matches(&document, empty, ":empty"));
    document.append_text(empty, " ".into()).unwrap();
    assert!(!matches(&document, empty, ":empty"));
    assert!(!matches(&document, html, ":empty"));
    let mut fragment = Document::new_fragment();
    let root = fragment.root_id();
    let element = append(&mut fragment, root, "p", &[]);
    assert!(!matches(&fragment, element, ":root"));
    assert!(matches(&fragment, element, ":empty:only-child"));
}

#[test]
fn negation_accepts_compounds_lists_complex_selectors_and_nested_negation() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    let element = append(
        &mut document,
        parent,
        "p",
        &[("class", "notice"), ("data-x", "a,b")],
    );
    for source in [
        "p:not(.missing)",
        "p:not(span, #unused)",
        "p:not(main > span, .notice.missing)",
        "p:not([data-x='x,y'])",
        "p:not(:not(.notice))",
        "main > :not(:last-of-type.missing)",
    ] {
        assert!(matches(&document, element, source), "{source}");
    }
    for source in [
        "p:not(*)",
        "p:not(.notice)",
        "p:not(.missing, p.notice[data-x])",
        "p:not(main > .notice)",
        "p:not([data-x='a,b'], .missing)",
        "p:not(:not(.missing))",
    ] {
        assert!(!matches(&document, element, source), "{source}");
    }
    assert_eq!(parse(":not(.a, .b), [data-x='a,b']").unwrap().len(), 2);
}

#[test]
fn names_and_default_attribute_values_are_case_sensitive_in_foreign_and_xml_elements() {
    let mut document = Document::new();
    let root = document.root_id();
    for element in [
        ElementData::namespaced(
            "linearGradient".into(),
            vec![Attribute::new("viewBox".into(), "TEXT".into())],
            Namespace::Svg,
            None,
        ),
        ElementData::xml(
            "linearGradient".into(),
            vec![Attribute::new("viewBox".into(), "TEXT".into())],
            Namespace::Html,
            None,
        ),
    ] {
        let node = document.append_element(root, element).unwrap();
        assert!(matches(&document, node, "linearGradient[viewBox=TEXT]"));
        assert!(!matches(&document, node, "lineargradient"));
        assert!(!matches(&document, node, "[viewbox]"));
        assert!(!matches(&document, node, "[viewBox=text]"));
        assert!(matches(&document, node, "[viewBox=text i]"));
    }
    let xml = document
        .append_element(
            root,
            ElementData::xml(
                "input".into(),
                vec![Attribute::new("type".into(), "TEXT".into())],
                Namespace::Html,
                None,
            ),
        )
        .unwrap();
    assert!(!matches(&document, xml, "[type=text]"));
    assert!(matches(&document, xml, "[type=text i]"));
    let namespaced = document
        .append_element(
            root,
            ElementData::namespaced(
                "a".into(),
                vec![Attribute::namespaced(
                    "href".into(),
                    "next".into(),
                    "http://www.w3.org/1999/xlink".into(),
                    Some("xlink".into()),
                )],
                Namespace::Svg,
                None,
            ),
        )
        .unwrap();
    assert!(!matches(&document, namespaced, "[href]"));
}

#[test]
fn quirks_case_folding_applies_to_ids_and_classes_but_not_attribute_values() {
    let mut document = Document::new();
    document.set_mode(DocumentMode::Quirks);
    let root = document.root_id();
    let element = append(
        &mut document,
        root,
        "p",
        &[("id", "Hero"), ("class", "Notice")],
    );
    assert!(matches(&document, element, "#hero.notice"));
    assert!(!matches(&document, element, "[id=hero]"));
    assert!(!matches(&document, element, "[class~=notice]"));
    assert!(matches(&document, element, "[id=hero i][class~=notice i]"));
}

#[test]
fn matching_uses_current_tree_order_after_reordering_and_detaching_nodes() {
    let mut document = Document::new();
    let root = document.root_id();
    let parent = append(&mut document, root, "main", &[]);
    let a = append(&mut document, parent, "a", &[]);
    let b = append(&mut document, parent, "b", &[]);
    let c = append(&mut document, parent, "c", &[]);
    assert!(matches(&document, b, "a + b"));
    document.insert_before(parent, c, Some(b)).unwrap();
    assert!(!matches(&document, b, "a + b"));
    assert!(matches(&document, b, "c + b:last-child"));
    assert!(matches(&document, b, "a ~ b"));
    document.detach(a).unwrap();
    assert!(matches(&document, c, "c:first-child"));
    assert!(!matches(&document, b, "a ~ b"));
}

#[test]
fn rejects_invalid_or_unsupported_forms_without_partial_matching() {
    for source in [
        "",
        " ",
        "p,",
        ",p",
        "p,,a",
        ".",
        "#",
        "#123",
        ".123",
        "p*",
        "> p",
        "p >",
        "p >> a",
        "p + ~ a",
        "p +",
        "p ~",
        "p:unknown",
        "p: first-child",
        "p:first-child()",
        "p:not ()",
        "p:not()",
        "p:not(,a)",
        "p:not(a,)",
        "p:not(a,,b)",
        "p:not(> a)",
        "p:not(a",
        "p:not(a))",
        "p:not(.a, :unknown)",
        "p, :not(a, :unknown)",
        "[]",
        "[x",
        "[x=]",
        "[x=123]",
        "[x='a' q]",
        "[x='a' i s]",
        "[x i]",
        "[x~ =a]",
        "[x!=a]",
        "[x='a'",
        "[x=a]]",
        "svg|a",
        "[xlink|href]",
        ":nth-child(2.0)",
        ":has(:has(p))",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn nesting_limit_is_enforced_and_long_combinator_chains_do_not_recurse() {
    let nested = |depth| format!("{}p{}", ":not(".repeat(depth), ")".repeat(depth));
    assert!(parse(&nested(MAX_SELECTOR_NESTING)).is_ok());
    assert!(parse(&nested(MAX_SELECTOR_NESTING + 1)).is_err());
    assert!(parse(&nested(10_000)).is_err());

    let mut document = Document::new();
    let mut parent = document.root_id();
    for _ in 0..2_000 {
        parent = append(&mut document, parent, "p", &[]);
    }
    let source = vec!["p"; 2_000].join(">");
    assert!(matches(&document, parent, &source));
    assert!(!matches(&document, parent, &format!("div>{source}")));
    assert!(matches(&document, parent, &nested(MAX_SELECTOR_NESTING)));
}

#[test]
fn state_triggers_fire_only_for_changed_elements_matching_the_state_compound() {
    let mut document = Document::new();
    let root = document.root_id();
    let html = append(&mut document, root, "html", &[]);
    let body = append(&mut document, html, "body", &[]);
    let link = append(&mut document, body, "a", &[("class", "nav")]);
    let para = append(&mut document, body, "p", &[]);
    let span = append(&mut document, para, "span", &[]);
    let triggers = |source: &str| {
        let mut triggers = StateTriggers::default();
        for selector in parse(source).unwrap() {
            triggers.add(&selector);
        }
        triggers
    };
    let hover = |id| InteractionState {
        hovered: Some(id),
        ..InteractionState::default()
    };
    let none = InteractionState::default();

    let links = triggers("a.nav:hover, .menu:focus span");
    assert!(!links.affected(&document, &hover(span), &hover(para)));
    assert!(links.affected(&document, &hover(span), &hover(link)));
    assert!(!links.affected(&document, &none, &hover(para)));
    // Shared ancestors keep their hover state; only the differing chains count.
    assert!(!triggers("body:hover").affected(&document, &hover(span), &hover(link)));
    assert!(triggers("body:hover").affected(&document, &none, &hover(span)));
    // Nested and relative state selectors are indexed by their inner compounds.
    assert!(triggers("p:has(span:hover)").affected(&document, &none, &hover(span)));
    assert!(!triggers("p:not(a:hover)").affected(&document, &none, &hover(span)));
    assert!(triggers(":is(:hover) > em").affected(&document, &none, &hover(span)));
    assert!(triggers("*:focus-visible").affected(
        &document,
        &InteractionState {
            focused: Some(link),
            ..none.clone()
        },
        &InteractionState {
            focused: Some(link),
            focus_visible: true,
            ..none.clone()
        }
    ));
    assert!(!triggers("a:hover").affected(
        &document,
        &none,
        &InteractionState {
            focused: Some(link),
            ..none.clone()
        }
    ));
}
