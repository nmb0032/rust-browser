use super::*;
use crate::{css, dom::Document, html::parser};

fn id(document: &Document, name: &str) -> crate::dom::NodeId {
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
fn whitespace_shorthand_and_longhand_grammar_is_typed_atomic_and_partial() {
    use TextWrapMode::{Nowrap, Wrap};
    use WhiteSpaceCollapse::{BreakSpaces, Collapse, Preserve, PreserveBreaks};
    use declarations::{Property, Value};
    let parse = |name: &str, source: &str| {
        declarations::parse(
            &css::parse_declarations(&format!("{name}:{source}")).unwrap()[0],
            0,
        )
    };
    for (source, collapse, wrap) in [
        ("normal", Collapse, Wrap),
        ("nowrap", Collapse, Nowrap),
        ("pre", Preserve, Nowrap),
        ("pre-wrap", Preserve, Wrap),
        ("pre-line", PreserveBreaks, Wrap),
        ("break-spaces", BreakSpaces, Wrap),
        ("preserve nowrap", Preserve, Nowrap),
        ("wrap preserve-breaks", PreserveBreaks, Wrap),
        ("collapse", Collapse, Wrap),
        ("preserve", Preserve, Wrap),
        ("nowrap break-spaces", BreakSpaces, Nowrap),
    ] {
        let values = parse("white-space", source).unwrap();
        assert_eq!(values.len(), 2);
        assert!(
            matches!(values[0].value, Value::WhiteSpaceCollapse(v) if v == collapse),
            "{source}"
        );
        assert!(
            matches!(values[1].value, Value::TextWrapMode(v) if v == wrap),
            "{source}"
        );
    }
    for source in [
        "normal nowrap",
        "pre wrap",
        "preserve preserve",
        "wrap nowrap",
        "preserve nowrap discard-before",
        "preserve-spaces",
        "discard",
        "balance",
        "break-spaces preserve",
        "none",
        "nowrap trim-inner",
        "preserve nowrap none",
    ] {
        assert!(parse("white-space", source).is_err(), "{source}");
    }
    for source in ["discard", "preserve-spaces"] {
        assert!(parse("white-space-collapse", source).is_err());
    }
    for source in ["balance", "pretty", "stable"] {
        assert!(parse("text-wrap-mode", source).is_err());
    }
    assert!(parse("white-space-trim", "none").is_err());
    for wide in ["inherit", "initial", "unset", "revert", "revert-layer"] {
        let values = parse("white-space", wide).unwrap();
        assert_eq!(values.len(), 2);
        assert!(values.iter().all(|v| matches!(v.value, Value::Wide(_))));
    }
    for property in [Property::WhiteSpaceCollapse, Property::TextWrapMode] {
        assert!(property.inherited());
        assert_eq!(
            metadata::property(property.name()).unwrap().support,
            metadata::Support::Partial
        );
    }
    let meta = metadata::property("white-space").unwrap();
    assert_eq!(meta.longhands, ["white-space-collapse", "text-wrap-mode"]);
    assert_eq!(meta.support, metadata::Support::Partial);
    assert_eq!(
        metadata::property("white-space-trim").unwrap().support,
        metadata::Support::Unimplemented
    );
}

#[test]
fn whitespace_variables_wide_values_invalid_atomicity_inheritance_and_supports() {
    let document = parser::parse(
        "<style>#host{white-space:pre;--valid:pre-line;--bad:preserve discard-before}\
         #valid{white-space:var(--valid)}#invalid{white-space:normal;white-space:var(--bad)}\
         #parsebad{white-space:break-spaces;white-space:preserve discard-before}\
         #initial{white-space:initial}#inherit{white-space:inherit}#unset{white-space:unset}\
         #all{all:initial}#pair{white-space-collapse:preserve-breaks;text-wrap-mode:wrap}\
         #shorthand{white-space-collapse:break-spaces;text-wrap-mode:nowrap;white-space:normal}\
         @supports (white-space:break-spaces){#valid{color:blue}}\
         @supports (white-space:preserve-spaces){#valid{color:red}}\
         #host::before{content:' ';white-space:pre-wrap}</style>\
         <div id='host'><span id='valid'>x</span><span id='invalid'>x</span>\
         <span id='parsebad'>x</span><span id='initial'>x</span><span id='inherit'>x</span>\
         <span id='unset'>x</span><span id='all'>x</span><span id='pair'>x</span><span id='shorthand'>x</span></div>"
    ).unwrap();
    let result = resolve(&document);
    use TextWrapMode::{Nowrap, Wrap};
    use WhiteSpaceCollapse::{BreakSpaces, Collapse, Preserve, PreserveBreaks};
    for (name, collapse, wrap) in [
        ("valid", PreserveBreaks, Wrap),
        ("invalid", Preserve, Nowrap),
        ("parsebad", BreakSpaces, Wrap),
        ("initial", Collapse, Wrap),
        ("inherit", Preserve, Nowrap),
        ("unset", Preserve, Nowrap),
        ("all", Collapse, Wrap),
        ("pair", PreserveBreaks, Wrap),
        ("shorthand", Collapse, Wrap),
    ] {
        let style = result.styles.get(id(&document, name)).unwrap();
        assert_eq!(
            (style.white_space_collapse, style.text_wrap_mode),
            (collapse, wrap),
            "{name}"
        );
        for &child in document.node(id(&document, name)).unwrap().children() {
            let text = result.styles.get(child).unwrap();
            assert_eq!(
                (text.white_space_collapse, text.text_wrap_mode),
                (collapse, wrap)
            );
        }
    }
    assert_eq!(
        result.styles.get(id(&document, "valid")).unwrap().color,
        Some(0xff)
    );
    let pseudo = result
        .styles
        .pseudo(id(&document, "host"), PseudoElement::Before)
        .unwrap();
    assert_eq!(
        (pseudo.white_space_collapse, pseudo.text_wrap_mode),
        (Preserve, Wrap)
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("white-space"))
    );
    let dump = crate::devtools::format_style(&document, &result.styles);
    assert!(dump.contains("white-space-collapse: PreserveBreaks"));
    assert!(dump.contains("text-wrap-mode: Nowrap"));
}

#[test]
fn whitespace_rollbacks_origins_important_font_independence_and_pre_ua() {
    let document = parser::parse(
        "<style>@layer low,high;@layer low{#layer{white-space:pre-line}}\
         @layer high{#layer{white-space:nowrap;white-space:revert-layer}}\
         #origin{white-space:normal;white-space:revert}\
         #important{white-space:pre!important}#font{white-space:pre;font:20px/30px serif}\
         #pre-revert{white-space:normal;white-space:revert}</style>\
         <div id='layer'>x</div><div id='origin'>x</div>\
         <div id='important' style='white-space:normal'>x</div><div id='font'>x</div>\
         <pre id='pre-ua'> x\n y </pre><pre id='pre-revert'> x\n y </pre>",
    )
    .unwrap();
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &["#origin{white-space:break-spaces}"],
    );
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    use TextWrapMode::{Nowrap, Wrap};
    use WhiteSpaceCollapse::{BreakSpaces, Preserve, PreserveBreaks};
    for (name, collapse, wrap) in [
        ("layer", PreserveBreaks, Wrap),
        ("origin", BreakSpaces, Wrap),
        ("important", Preserve, Nowrap),
        ("font", Preserve, Nowrap),
        ("pre-ua", Preserve, Nowrap),
        ("pre-revert", Preserve, Nowrap),
    ] {
        let style = result.styles.get(id(&document, name)).unwrap();
        assert_eq!(
            (style.white_space_collapse, style.text_wrap_mode),
            (collapse, wrap),
            "{name}"
        );
    }
}
