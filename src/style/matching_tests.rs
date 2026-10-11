use std::{fmt::Write, sync::Arc};

use super::*;
use crate::dom::{Attribute, DocumentMode, ElementData, Namespace};

fn sheet(document: &Document, source: &str) -> super::super::AuthorStylesheet {
    super::super::AuthorStylesheet {
        node_id: document.root_id(),
        stylesheet: Arc::new(css::parse(source).unwrap()),
        url: Some(url::Url::parse("https://example.test/final.css").unwrap()),
    }
}

fn compare(
    document: &Document,
    sheets: StylesheetCollection,
    users: &[&str],
    ua: &str,
) -> StyleResolution {
    compare_with_environment(document, sheets, users, ua, MediaEnvironment::default())
}

fn compare_with_environment(
    document: &Document,
    sheets: StylesheetCollection,
    users: &[&str],
    ua: &str,
    environment: MediaEnvironment,
) -> StyleResolution {
    compare_with_environment_and_state(
        document,
        sheets,
        users,
        ua,
        environment,
        &InteractionState::default(),
    )
}

fn compare_with_environment_and_state(
    document: &Document,
    sheets: StylesheetCollection,
    users: &[&str],
    ua: &str,
    environment: MediaEnvironment,
    state: &InteractionState,
) -> StyleResolution {
    let mut diagnostics = Vec::new();
    let parsed_ua = css::parse(ua).unwrap();
    let baseline_ua = compile::<0>(&parsed_ua, None, &mut diagnostics, environment, true);
    let indexed_ua = compile::<1>(&parsed_ua, None, &mut diagnostics, environment, true);
    let optimized_ua = compile::<2>(&parsed_ua, None, &mut diagnostics, environment, true);
    assert!(diagnostics.is_empty());
    let baseline = resolve_internal_state::<0>(
        document,
        sheets.clone(),
        users,
        &baseline_ua,
        environment,
        state,
    );
    let indexed = resolve_internal_state::<1>(
        document,
        sheets.clone(),
        users,
        &indexed_ua,
        environment,
        state,
    );
    let optimized =
        resolve_internal_state::<2>(document, sheets, users, &optimized_ua, environment, state);
    for result in [&indexed, &optimized] {
        assert_eq!(result.styles.nodes, baseline.styles.nodes);
        assert_eq!(result.styles.pseudos, baseline.styles.pseudos);
        assert_eq!(
            result
                .stylesheets
                .diagnostics
                .iter()
                .map(|d| (d.node_id, d.message.as_str()))
                .collect::<Vec<_>>(),
            baseline
                .stylesheets
                .diagnostics
                .iter()
                .map(|d| (d.node_id, d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }
    optimized
}

#[test]
fn functional_selectors_sibling_has_pseudos_scopes_and_states_match_all_reference_modes() {
    let mut document = crate::html::parser::parse(
        "<!doctype html><style>
        @layer first, second;
        @layer first {section:has(> .a + .b) .leaf {color:red}}
        @layer second {:is(.a:has(+ .b), #missing) > .leaf {color:blue}}
        :where(section:has(> .b .inside)) .leaf {padding:7px}
        :nth-child(1 of :is(.leaf, #missing))::before {content:'before'; color:green}
        .a:has(+ .b .inside)::after {content:'after'; color:purple}
        section:not(:has(> .missing)) .leaf:nth-child(1 of section .leaf) {margin:3px}
        section:hover .leaf {font-size:21px}
        #a:has(+ .b:focus-within) .leaf::before {color:yellow}
        @scope (section:has(> .b)) { :scope .leaf::after {content:'scope'; color:red}}
        @media (min-width:500px) {.a:has(~ .b) .leaf {background:yellow}}
        </style><section id='section'><article class='a' id='a'><p class='leaf' id='leaf'>leaf</p></article>
        <aside class='b' id='b'><a href='/' class='inside' id='focus'>link</a></aside></section>",
    ).unwrap();
    let find = |document: &Document, name: &str| {
        document
            .preorder()
            .find(|&id| {
                document
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.attribute("id") == Some(name))
            })
            .unwrap()
    };
    let leaf = find(&document, "leaf");
    let b = find(&document, "b");
    let a = find(&document, "a");
    let section = find(&document, "section");
    let state = InteractionState {
        hovered: Some(leaf),
        focused: Some(find(&document, "focus")),
        ..InteractionState::default()
    };
    for width in [400.0, 800.0] {
        let result = compare_with_environment_and_state(
            &document,
            collect_stylesheets(&document),
            &[],
            include_str!("user_agent.css"),
            MediaEnvironment::new(width, 600.0),
            &state,
        );
        assert!(result.stylesheets.diagnostics.is_empty());
        assert_eq!(result.styles.get(leaf).unwrap().color, Some(0x0000ff));
        assert_eq!(
            result
                .styles
                .pseudo(leaf, PseudoElement::Before)
                .unwrap()
                .color,
            Some(0xffff00)
        );
        assert!(result.styles.pseudo(a, PseudoElement::After).is_some());
    }
    document.insert_before(section, b, Some(a)).unwrap();
    let result = compare_with_environment_and_state(
        &document,
        collect_stylesheets(&document),
        &[],
        include_str!("user_agent.css"),
        MediaEnvironment::default(),
        &state,
    );
    assert!(result.styles.pseudo(a, PseudoElement::After).is_none());
}
fn compare_css(document: &Document, source: &str) -> StyleResolution {
    let sheets = StylesheetCollection {
        stylesheets: vec![sheet(document, source)],
        ..StylesheetCollection::default()
    };
    compare(document, sheets, &[], include_str!("user_agent.css"))
}

fn matching_limit_document() -> (Document, NodeId) {
    let mut document = Document::new();
    let root = document.root_id();
    let outer = document
        .append_element(
            root,
            ElementData::new(
                "section".into(),
                vec![Attribute::new("id".into(), "outer".into())],
            ),
        )
        .unwrap();
    let target = document
        .append_element(
            outer,
            ElementData::new(
                "main".into(),
                vec![Attribute::new("id".into(), "target".into())],
            ),
        )
        .unwrap();
    for _ in 0..400 {
        document
            .append_element(target, ElementData::new("p".into(), Vec::new()))
            .unwrap();
    }
    (document, target)
}

#[test]
fn actual_matching_limits_report_and_fail_closed_in_selectors_scope_roots_and_limits() {
    let (document, target) = matching_limit_document();
    let result = compare_css(
        &document,
        "#target {color:green; width:12px}
        #target:not(:has(> :nth-child(999999 of p))) {color:red}
        #target:not(:not(:has(> :nth-child(999999 of p)))) {width:20px}
        @scope (#target:not(:has(> :nth-child(999999 of p)))) {:where(#target) {width:22px}}
        @scope (#outer) to (#target:not(:has(> :nth-child(999999 of p)))) {:where(#target) {margin:50px}}",
    );
    let style = result.styles.get(target).unwrap();
    assert_eq!(style.color, Some(0x008000));
    assert_eq!(style.width, Some(Length::Px(12.0)));
    assert_eq!(style.margin.top, Length::Px(0.0).into());
    assert_eq!(
        result.stylesheets.diagnostics.len(),
        super::super::matching::MAX_MATCH_DIAGNOSTICS + 1
    );
    for diagnostic in result.stylesheets.diagnostics.iter().take(4) {
        assert_eq!(diagnostic.node_id, Some(target));
        assert!(diagnostic.message.contains("100000-step limit"));
        assert!(diagnostic.message.contains("match rejected"));
    }
}

#[test]
fn matching_limit_diagnostics_are_capped_per_resolution_and_reset_on_restyle() {
    let (document, target) = matching_limit_document();
    let source = format!(
        "#target {{color:green}} {}",
        "#target:not(:has(> :nth-child(999999 of p))) {color:red}"
            .repeat(super::super::matching::MAX_MATCH_DIAGNOSTICS + 3)
    );
    let result = compare_css(&document, &source);
    assert_eq!(result.styles.get(target).unwrap().color, Some(0x008000));
    assert_eq!(
        result.stylesheets.diagnostics.len(),
        super::super::matching::MAX_MATCH_DIAGNOSTICS + 1
    );
    assert_eq!(
        result.stylesheets.diagnostics.last().unwrap().message,
        "additional selector matching diagnostics omitted"
    );
    let restyled = super::super::resolve_with_stylesheets(
        &document,
        StylesheetCollection {
            stylesheets: vec![sheet(&document, &source)],
            ..StylesheetCollection::default()
        },
    );
    assert_eq!(
        restyled.stylesheets.diagnostics,
        result.stylesheets.diagnostics
    );
}

#[test]
fn dynamic_scope_roots_and_limits_use_the_current_hover_and_focus_state_in_all_modes() {
    let document = crate::html::parser::parse(
        "<style>
        #target {color:black; width:10px}
        @scope (#root:hover) to (#stop:focus) {#target {color:red}}
        @scope (#root:focus) to (#stop:hover) {#target {width:20px}}
        </style><section id='root' tabindex='0'><div id='stop' tabindex='0'>
        <p id='target'>text</p></div></section>",
    )
    .unwrap();
    let find = |name| {
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
    };
    let root = find("root");
    let stop = find("stop");
    let target = find("target");
    for (hovered, focused, color, width) in [
        (None, None, 0x000000, 10.0),
        (Some(root), None, 0xff0000, 10.0),
        (Some(root), Some(stop), 0x000000, 10.0),
        (None, Some(root), 0x000000, 20.0),
        (Some(stop), Some(root), 0xff0000, 10.0),
        (None, None, 0x000000, 10.0),
    ] {
        let result = compare_with_environment_and_state(
            &document,
            collect_stylesheets(&document),
            &[],
            include_str!("user_agent.css"),
            MediaEnvironment::default(),
            &InteractionState {
                hovered,
                focused,
                ..InteractionState::default()
            },
        );
        assert!(result.stylesheets.diagnostics.is_empty());
        let style = result.styles.get(target).unwrap();
        assert_eq!(style.color, Some(color));
        assert_eq!(style.width, Some(Length::Px(width)));
    }
}

#[test]
fn math_units_and_variables_preserve_reference_matching_and_geometry() {
    let document = crate::html::parser::parse(
        "<style>
        :root {font-size:20px; --g:2rem} body {margin:0}
        .container > #target {width:calc(100% - var(--g)); padding:1lh}
        @media(max-width:500px) {.container > #target {width:clamp(100px,50%,300px)}}
        .missing #target {width:2px}
        </style><div class='container'><div id='target'>math</div></div>",
    )
    .unwrap();
    let target = document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some("target"))
        })
        .unwrap();
    for width in [200.0, 400.0, 800.0] {
        let result = compare_with_environment(
            &document,
            collect_stylesheets(&document),
            &[],
            include_str!("user_agent.css"),
            MediaEnvironment::new(width, 600.0),
        );
        assert!(result.stylesheets.diagnostics.is_empty());
        let expected = if width <= 500.0 {
            ((width - 48.0) * 0.5).clamp(100.0, 300.0)
        } else {
            width - 48.0 - 40.0
        };
        assert_eq!(
            result
                .styles
                .get(target)
                .unwrap()
                .computed_width()
                .unwrap()
                .use_value(width - 48.0)
                .unwrap()
                .0,
            expected
        );
    }
}

#[test]
fn computed_math_failures_have_identical_diagnostic_order_in_reference_modes() {
    let document = crate::html::parser::parse("<div id='target'></div>").unwrap();
    let result = compare_css(
        &document,
        "#target {font-size:calc(1px / 0); line-height:calc(1 / 0); width:calc(1px / 0); padding:calc(1px / 0); margin:calc(1px / 0)}",
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 11);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("invalid at computed-value time"))
    );
}

#[test]
fn media_gates_source_order_lists_bloom_and_origins_match_reference_at_multiple_widths() {
    let document = crate::html::parser::parse(
        "<!doctype html><style>\
        .target {color:red; width:10px}\
        @media only screen and (min-width:600px) {\
          #missing, .target {color:green; width:20px}\
          @media (max-width:700px) {\
            .branch > .target, #target.target {color:blue; width:30px}\
            .absent #target {color:purple}\
            .left + .branch .target {padding:3px}\
          }\
          .target {width:40px}\
        }\
        .target {width:50px}\
        @media (width:), (width:600px) {:not(.missing), #target {margin:2px}}\
        @media not (future-height:600px) {#target {color:purple}}\
        </style>\
        <style media='print, (max-width:500px)'>#target {background:yellow}</style>\
        <style media='(min-width:600px)'>.branch .target {background:green}</style>\
        <i class='left'></i><main class='branch'><p id='target' class='target'>target</p>\
        <p id='other' class='target'>other</p></main>",
    )
    .unwrap();
    for width in [400.0, 500.0, 599.5, 600.0, 600.5, 700.0, 700.5, 800.0] {
        let environment = MediaEnvironment::new(width, 600.0);
        let result = compare_with_environment(
            &document,
            collect_stylesheets(&document),
            &["@media (max-width:500px) {.target {padding:5px !important}}"],
            include_str!("user_agent.css"),
            environment,
        );
        let target = result.styles.get(find(&document, "target")).unwrap();
        assert_eq!(
            target.color,
            Some(if width < 600.0 {
                0xFF0000
            } else if width <= 700.0 {
                0x0000FF
            } else {
                0x008000
            })
        );
        assert_eq!(
            target.width,
            Some(Length::Px(if (600.0..=700.0).contains(&width) {
                30.0
            } else {
                50.0
            }))
        );
        assert_eq!(result.stylesheets.diagnostics.len(), 2);
    }
}

#[test]
fn variables_pending_shorthands_cycles_and_all_origins_match_reference() {
    let document = crate::html::parser::parse(
        "<!doctype html><style>\
        :root {--theme:red; --size:20px; --font:italic 20px/2 serif}\
        .branch {--alias:var(--theme); --cycle:var(--cycle)}\
        .absent #target {--theme:yellow} #missing, .target {color:var(--theme)}\
        @media (min-width:600px) {:root {--theme:blue; --size:40px}}\
        .left + .branch .target {width:var(--size); background:var(--cycle, green)}\
        .target {margin:var(--m); padding:var(--missing); font:var(--font); border:var(--border)}\
        #target {margin-left:9px !important; border-left-color:purple}\
        </style><i class='left'></i><main class='branch'>\
        <p id='target' class='target' style='--theme:purple !important; --border:solid; --m:2px 3px'>target</p>\
        <p id='other' class='target'>other</p></main>"
    ).unwrap();
    for width in [400.0, 600.0, 800.0] {
        let result = compare_with_environment(
            &document,
            collect_stylesheets(&document),
            &[".target {--m:4px; --border:1px solid green; --theme:green !important}"],
            "p {display:block; --m:1px; --border:2px solid blue; --theme:yellow !important}",
            MediaEnvironment::new(width, 600.0),
        );
        let target = result.styles.get(find(&document, "target")).unwrap();
        assert_eq!(target.color, Some(0xffff00));
        assert_eq!(
            target.width,
            Some(Length::Px(if width < 600.0 { 20.0 } else { 40.0 }))
        );
        assert_eq!(target.margin.left, Length::Px(9.0).into());
        assert_eq!(target.margin.top, Length::Px(2.0).into());
        assert_eq!(target.border_color.left, Some(0x800080));
        assert_eq!(target.border_width.top, 3.0);
        assert_eq!(target.background_color, Some(0x008000));
        let other = result.styles.get(find(&document, "other")).unwrap();
        assert_eq!(other.margin.top, Length::Px(4.0).into());
        assert_eq!(other.border_width.top, 1.0);
    }
}

fn find(document: &Document, id: &str) -> NodeId {
    document
        .preorder()
        .find(|&node| {
            document
                .node(node)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(id))
        })
        .unwrap()
}

fn append(
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
                    .map(|(k, v)| Attribute::new((*k).into(), (*v).into()))
                    .collect(),
            ),
        )
        .unwrap()
}

#[test]
fn adversarial_lists_negation_and_combinator_backtracking_match_reference() {
    let document = crate::html::parser::parse(
        "<!doctype html><main class='a'><div class='b'><div class='b'>\
         <p id='target' class='target a a' data-x='TEXT'>text</p></div></div></main>\
         <section id='siblings'><i class='left'></i><!--skip--><i class='middle'></i>\
         text<i></i><i class='middle'></i><p id='right' class='target'></p></section>\
         <aside class='left'></aside><nav class='ancestor'><span id='inside'></span></nav>",
    )
    .unwrap();
    let result = compare_css(
        &document,
        "* {color:black} [data-x] {color:yellow} :not(.absent) {padding:1px}\
         .absent #target {color:purple} #missing, .target {color:red}\
         .a > .b .target {color:green} .a > .b > .target {color:blue}\
         .left + .middle ~ .target {background-color:yellow}\
         .left + .middle + .target {background-color:blue}\
         .left + .ancestor #inside {color:purple}\
         section > .left + .middle ~ .target {padding:3px}\
         :not(.absent .target) {margin:2px}\
         :not(:not(.target)) {border:1px solid red}\
         [data-x=text i]:not(.absent, #missing) {background-color:green}\
         #target, .target, #target.target {color:purple}\
         #target {color:blue}\
         :not([data-x]), .target {width:40%}",
    );
    // The highest matching specificity in a list must survive deduplication.
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x800080)
    );
    assert_eq!(
        result
            .styles
            .get(find(&document, "right"))
            .unwrap()
            .background_color,
        Some(0xFFFF00)
    );
    // The left sibling of an ancestor must never become an ancestry requirement.
    assert_eq!(
        result.styles.get(find(&document, "inside")).unwrap().color,
        Some(0x800080)
    );
}

#[test]
fn all_origins_sheet_rule_declaration_order_inline_and_diagnostics_match_reference() {
    let document = crate::html::parser::parse(
        "<!doctype html><style>.a {color:blue} #x {padding:2px}</style>\
         <style>.a {color:yellow; color:green} * {margin:1px}</style>\
         <p id='x' class='a' style='color:purple !important; border:invalid'>text</p>\
         <img id='image' class='a' width='31'><p id='y' class='a'>text</p>",
    )
    .unwrap();
    let mut sheets = collect_stylesheets(&document);
    sheets.stylesheets.insert(
        0,
        sheet(
            &document,
            ".a {color:red !important} #x {color:blue !important}",
        ),
    );
    sheets.stylesheets.push(sheet(
        &document,
        ".a {color:red; color:blue} * {unsupported:value}",
    ));
    let result = compare(
        &document,
        sheets,
        &[
            "#x {color:red !important} .a {margin:5px}",
            "* {color:green !important; padding:3px !important}",
        ],
        "* {color:black} #x {color:yellow !important} .a {padding:4px !important}",
    );
    assert_eq!(
        result.styles.get(find(&document, "x")).unwrap().color,
        Some(0xFFFF00)
    );
    assert_eq!(
        result.styles.get(find(&document, "y")).unwrap().color,
        Some(0x008000)
    );
    assert_eq!(
        result.styles.get(find(&document, "image")).unwrap().width,
        Some(Length::Px(31.0))
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("https://example.test/final.css"))
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.node_id == Some(find(&document, "x")))
    );
}

#[test]
fn html_quirks_foreign_xml_and_namespaced_attributes_match_reference() {
    let css = "ROOT .target {color:red} root .target {color:blue}\
        #Hero .target {padding:2px} #hero .target {padding:3px}\
        .Parent .target {margin:2px} .parent .target {margin:3px}\
        linearGradient {background-color:yellow} lineargradient {background-color:green}\
        [viewBox] {width:20px} [viewbox] {width:30px}\
        [type=text] {color:purple} [type=TEXT s] {color:green}\
        [href] {border:2px solid blue}\
        .Parent + .parent .target {font-size:18px}";
    for mode in [
        DocumentMode::NoQuirks,
        DocumentMode::LimitedQuirks,
        DocumentMode::Quirks,
    ] {
        let mut document = Document::new();
        document.set_mode(mode);
        let root = document.root_id();
        for names in 0..4 {
            let attributes = vec![
                Attribute::new("id".into(), "Hero".into()),
                Attribute::new("class".into(), "Parent".into()),
            ];
            let element = match names {
                0 => ElementData::new("root".into(), attributes),
                1 => ElementData::namespaced("root".into(), attributes, Namespace::Svg, None),
                2 => ElementData::xml("root".into(), attributes, Namespace::Html, None),
                _ => ElementData::xml(
                    "root".into(),
                    attributes,
                    Namespace::Other("urn:test".into()),
                    Some("t".into()),
                ),
            };
            let parent = document.append_element(root, element).unwrap();
            for namespace in [
                Namespace::Html,
                Namespace::Svg,
                Namespace::MathMl,
                Namespace::None,
            ] {
                document
                    .append_element(
                        parent,
                        ElementData::namespaced(
                            "linearGradient".into(),
                            vec![
                                Attribute::new("class".into(), "target".into()),
                                Attribute::new("viewBox".into(), "TEXT".into()),
                                Attribute::new("type".into(), "TEXT".into()),
                                Attribute::namespaced(
                                    "href".into(),
                                    "url".into(),
                                    "urn:xlink".into(),
                                    Some("x".into()),
                                ),
                            ],
                            namespace,
                            None,
                        ),
                    )
                    .unwrap();
            }
        }
        compare_css(&document, css);
    }
    let xml = crate::xml::parse(
        "<ROOT xmlns='urn:test' xmlns:x='urn:attributes' id='Hero' class='Parent'>\
         <linearGradient class='target' viewBox='TEXT' type='TEXT' x:href='url'/>\
         <root class='parent'><lineargradient class='target'/></root></ROOT>",
    )
    .unwrap();
    compare_css(&xml, css);
}

#[test]
fn saturated_bloom_and_folded_hash_collisions_never_replace_selector_matching() {
    let mut document = Document::new();
    let root = document.root_id();
    let classes = (0..4096)
        .map(|i| format!("c{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let ancestor = append(&mut document, root, "div", &[("class", &classes)]);
    let target = append(
        &mut document,
        ancestor,
        "p",
        &[("id", "target"), ("class", "Case")],
    );
    let sibling = append(
        &mut document,
        root,
        "p",
        &[("id", "sibling"), ("class", "case")],
    );
    let mut filter = crate::css::selectors::matching::AncestorBloom::default();
    filter.push(document.node(ancestor).unwrap().as_element().unwrap());
    let missing = selectors::parse(".notpresent p").unwrap().remove(0);
    assert!(filter.may_contain(&missing.ancestor_requirements()));
    assert!(!missing.matches(&document, target));
    let result = compare_css(
        &document,
        "p {color:red} .Case {color:green} .case {color:blue} .notpresent p {color:purple}",
    );
    assert_eq!(result.styles.get(target).unwrap().color, Some(0x008000));
    assert_eq!(result.styles.get(sibling).unwrap().color, Some(0x0000FF));
    filter.pop(document.node(ancestor).unwrap().as_element().unwrap());
    assert!(!filter.may_contain(&missing.ancestor_requirements()));
}

#[test]
fn ancestry_push_pop_repeated_keys_deep_trees_fragments_and_mutations_match_reference() {
    let mut document = Document::new_fragment();
    let root = document.root_id();
    let outer = append(&mut document, root, "div", &[("class", "present present")]);
    let inner = append(&mut document, outer, "div", &[("class", "present")]);
    let target = append(&mut document, inner, "p", &[("id", "target")]);
    let other = append(&mut document, root, "section", &[]);
    append(&mut document, other, "p", &[("id", "other")]);
    let css = ".present p {color:red} .absent p {color:blue} section > p {padding:3px}\
        .present + section > p {background-color:yellow} :not(.present p) {margin:2px}";
    assert_eq!(
        compare_css(&document, css)
            .styles
            .get(target)
            .unwrap()
            .color,
        Some(0xFF0000)
    );
    document.insert_before(other, target, None).unwrap();
    document.detach(outer).unwrap();
    document
        .merge_attributes(other, vec![Attribute::new("class".into(), "absent".into())])
        .unwrap();
    assert_eq!(
        compare_css(&document, css)
            .styles
            .get(target)
            .unwrap()
            .color,
        Some(0x0000FF)
    );
    let mut parent = other;
    for _ in 0..4096 {
        parent = append(&mut document, parent, "div", &[("class", "repeat")]);
    }
    let leaf = append(&mut document, parent, "span", &[("id", "leaf")]);
    let result = compare_css(
        &document,
        ".repeat span {color:red} .missing span {color:blue} .repeat > span {padding:2px}",
    );
    assert_eq!(result.styles.get(leaf).unwrap().color, Some(0xFF0000));
}

// Deterministic generator; no test dependencies and failures reproduce exactly.
struct Random(u64);

impl Random {
    fn pick(&mut self, count: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 32) as usize) % count
    }
}

#[test]
fn randomized_documents_selector_lists_and_mixed_chains_match_reference() {
    let compounds = [
        "*",
        "div",
        "DIV",
        "p",
        "span",
        "linearGradient",
        "#i0",
        "#I1",
        ".c0",
        ".C1",
        ".c2.c3",
        "[data-x]",
        "[data-x='TEXT' i]",
        ":empty",
        ":first-child",
        ":last-child",
        ":only-of-type",
        ":not(.c0)",
        ":not(.c1, #i0)",
        ":not(.c2 > p)",
        ":not(:not(.c3))",
        "p:not([hidden])",
    ];
    let combinators = [" ", " > ", " + ", " ~ "];
    for seed in 1..=16 {
        let mut random = Random(seed);
        let mut document = Document::new();
        document.set_mode(
            [
                DocumentMode::NoQuirks,
                DocumentMode::Quirks,
                DocumentMode::LimitedQuirks,
            ][random.pick(3)],
        );
        let mut nodes = vec![document.root_id()];
        for i in 0..100 {
            let parent = nodes[random.pick(nodes.len())];
            let tag = ["div", "p", "span", "linearGradient"][random.pick(4)];
            let mut attributes = vec![
                Attribute::new(
                    "id".into(),
                    format!(
                        "{}{}",
                        if random.pick(2) == 0 { "i" } else { "I" },
                        random.pick(5)
                    ),
                ),
                Attribute::new(
                    "class".into(),
                    format!(
                        "c{}\tC{}\x0Cc{}",
                        random.pick(4),
                        random.pick(4),
                        random.pick(4)
                    ),
                ),
                Attribute::new(
                    "data-x".into(),
                    ["TEXT", "text", "other"][random.pick(3)].into(),
                ),
            ];
            if random.pick(3) == 0 {
                attributes.push(Attribute::new(
                    "style".into(),
                    "color:purple; margin:1px !important".into(),
                ));
            }
            let element = match random.pick(4) {
                0 => ElementData::namespaced(tag.into(), attributes, Namespace::Svg, None),
                1 => ElementData::xml(tag.into(), attributes, Namespace::Html, None),
                _ => ElementData::new(tag.into(), attributes),
            };
            let node = document.append_element(parent, element).unwrap();
            nodes.push(node);
            if i % 3 == 0 {
                document.append_text(parent, " ".into()).unwrap();
            }
            if i % 7 == 0 {
                document.append_comment(parent, "ignored".into()).unwrap();
            }
        }
        let mut source = String::new();
        for rule in 0..180 {
            for member in 0..=random.pick(2) {
                if member != 0 {
                    source.push_str(", ");
                }
                for part in 0..=random.pick(3) {
                    if part != 0 {
                        source.push_str(combinators[random.pick(combinators.len())]);
                    }
                    source.push_str(compounds[random.pick(compounds.len())]);
                }
            }
            let color = ["red", "blue", "green", "purple"][random.pick(4)];
            writeln!(
                source,
                " {{color:{color}{}; padding:{}px; border:{}px solid {color}}}",
                if rule % 5 == 0 { " !important" } else { "" },
                random.pick(5),
                random.pick(4)
            )
            .unwrap();
        }
        let sheets = StylesheetCollection {
            stylesheets: vec![
                sheet(&document, &source),
                sheet(&document, ".c0 {color:yellow} * {font-size:18px}"),
            ],
            ..StylesheetCollection::default()
        };
        compare(
            &document,
            sheets,
            &[".c1 {color:green !important}", "p {margin:2px}"],
            "div {display:block} .c2 {color:red !important} * {color:black}",
        );
    }
}
