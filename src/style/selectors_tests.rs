use super::*;
use crate::{
    css::values::{Edges, Length},
    dom::{Document, NodeId},
    html::parser,
};

fn find(document: &Document, name: &str) -> NodeId {
    document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

#[test]
fn functional_specificity_cascades_with_layers_and_inactive_media() {
    let document = parser::parse(
        "<style>
        @layer low, high;
        @layer low {#target {color:red}}
        @layer high {
          :is(#unmatched,.item) {color:blue}
          .item {color:green}
          :where(#target) {color:purple}
        }
        :nth-child(1 of .item,#missing) {padding:7px}
        :has(> .child) {margin:3px}
        @media (max-width:300px) {#target {color:yellow}}
        </style><div id='target' class='item'><span class='child'></span></div>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.color, Some(0x0000ff));
    assert_eq!(style.padding, Edges::all(Length::Px(7.0)));
    assert_eq!(style.margin, Edges::all(Length::Px(3.0).into()));
}

#[test]
fn pseudo_cascade_is_isolated_inherits_originating_element_and_rolls_back_layers() {
    let document = parser::parse(
        "<style>
        @layer base, override;
        @layer base {#target::before {content:var(--prefix) attr(data-label); color:green; width:45px}}
        @layer override {#target::before {color:revert-layer; --prefix:'CSS '}}
        #target {font-size:24px; color:red}
        #target::after {content:var(--suffix); color:inherit; font-size:2em}
        @media (max-width:500px) {#target::after {display:none}}
        </style><p id='target' data-label='label' style=\"--prefix:'inline '; --suffix:'after'; color:blue; background:yellow\">origin</p>",
    ).unwrap();
    let id = find(&document, "target");
    let sheets = collect_stylesheets(&document);
    let result = resolve_with_style_environment(
        &document,
        sheets.clone(),
        &["#target::before {color:purple !important}"],
        crate::css::media::MediaEnvironment::new(800.0, 600.0),
        &InteractionState::default(),
    );
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let origin = result.styles.get(id).unwrap();
    let before = result.styles.pseudo(id, PseudoElement::Before).unwrap();
    let after = result.styles.pseudo(id, PseudoElement::After).unwrap();
    assert_eq!(origin.color, Some(0x0000ff));
    assert_eq!(origin.width, None);
    assert_eq!(origin.content, Content::Normal);
    assert_eq!(before.color, Some(0x800080));
    assert_eq!(before.font_size, 24.0);
    assert_eq!(before.background_color, None);
    assert_eq!(before.width, Some(Length::Px(45.0)));
    assert_eq!(
        before
            .content
            .text(document.node(id).unwrap().as_element().unwrap())
            .as_deref(),
        Some("CSS label")
    );
    assert_eq!(after.color, Some(0x0000ff));
    assert_eq!(after.font_size, 48.0);
    let normal = resolve_with_stylesheets(&document, sheets.clone());
    assert_eq!(
        normal
            .styles
            .pseudo(id, PseudoElement::Before)
            .unwrap()
            .color,
        Some(0x008000)
    );
    let narrow = resolve_with_stylesheets_and_environment(
        &document,
        sheets,
        crate::css::media::MediaEnvironment::new(400.0, 600.0),
    );
    assert_eq!(
        narrow
            .styles
            .pseudo(id, PseudoElement::After)
            .unwrap()
            .display,
        Display::None
    );
}

#[test]
fn namespace_scope_nesting_and_pseudo_only_parent_do_not_pollute_elements() {
    let document = parser::parse(
        "<style>
        @namespace h 'http://www.w3.org/1999/xhtml';
        @scope (h|section:has(> h|p)) to (.stop) {
          :scope {--prefix:'scope '}
          h|p {
            color:red;
            &:nth-child(1 of h|p)::before {content:var(--prefix) 'before'; color:blue}
          }
          :scope:has(> h|p) h|p::after {content:'after'; color:green}
        }
        #target::before {
          h|span {color:purple}
        }
        </style><section><p id='target'><span id='child'>child</span></p>
        <div class='stop'><p id='stopped'>stopped</p></div></section>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let id = find(&document, "target");
    assert_eq!(result.styles.get(id).unwrap().color, Some(0xff0000));
    assert_eq!(
        result.styles.get(find(&document, "child")).unwrap().color,
        Some(0xff0000)
    );
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::Before)
            .unwrap()
            .color,
        Some(0x0000ff)
    );
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::Before)
            .unwrap()
            .content
            .text(document.node(id).unwrap().as_element().unwrap())
            .as_deref(),
        Some("scope before")
    );
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::After)
            .unwrap()
            .color,
        Some(0x008000)
    );
    assert!(
        result
            .styles
            .pseudo(find(&document, "stopped"), PseudoElement::Before)
            .is_none()
    );
}

#[test]
fn layout_sensitive_pseudo_foundations_are_computed_and_explicitly_diagnosed() {
    let document = parser::parse(
        "<style>
        #target {color:red}
        #target::first-line {color:blue}
        #target::first-letter {font-size:64px}
        #target::selection {background:yellow}
        #target::backdrop {background:green}
        input::placeholder {color:purple}
        </style><p id='target'>unchanged text</p><input id='input' placeholder='hint'>",
    )
    .unwrap();
    let result = resolve(&document);
    let id = find(&document, "target");
    assert_eq!(result.styles.get(id).unwrap().color, Some(0xff0000));
    assert_eq!(result.styles.get(id).unwrap().font_size, 16.0);
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::FirstLine)
            .unwrap()
            .color,
        Some(0x0000ff)
    );
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::FirstLetter)
            .unwrap()
            .font_size,
        64.0
    );
    assert!(
        result
            .styles
            .pseudo(id, PseudoElement::Selection)
            .unwrap()
            .background_color
            .is_some()
    );
    assert!(
        result
            .styles
            .pseudo(id, PseudoElement::Backdrop)
            .unwrap()
            .background_color
            .is_some()
    );
    assert_eq!(
        result
            .styles
            .pseudo(find(&document, "input"), PseudoElement::Placeholder)
            .unwrap()
            .color,
        Some(0x800080)
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 5);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("layout/paint effect is not implemented"))
    );
}

#[test]
fn unsupported_marker_box_effects_remain_diagnosed_and_do_not_style_the_element() {
    let document = parser::parse("<style>li::marker {width:300px; background:red; content:'mark'}</style><ul><li id='target'>text</li></ul>").unwrap();
    let result = resolve(&document);
    let id = find(&document, "target");
    assert_eq!(result.styles.get(id).unwrap().width, None);
    assert_eq!(result.styles.get(id).unwrap().background_color, None);
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::Marker)
            .unwrap()
            .width,
        Some(Length::Px(300.0))
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 1);
    assert!(
        result.stylesheets.diagnostics[0]
            .message
            .contains("other effects are not implemented")
    );
}

#[test]
fn content_grammar_invalid_computed_values_and_element_only_inline_styles() {
    let document = parser::parse(
        "<style>
        #target::before {content:'good'; content:counter(item); content:url('x'); content:attr(data-x type(<string>))}
        #target::after {content:'earlier'; content:var(--bad)}
        #empty::before {content:''}
        #none::before {content:none}
        #normal::before {content:normal}
        #target {--bad:42}
        </style><p id='target' style=\"content:'inline does not generate'; --valid:'yes'\">text</p>
        <p id='empty'></p><p id='none'></p><p id='normal'></p>",
    ).unwrap();
    let result = resolve(&document);
    let id = find(&document, "target");
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::Before)
            .unwrap()
            .content,
        Content::Items(vec![ContentItem::Text("good".into())])
    );
    assert_eq!(
        result
            .styles
            .pseudo(id, PseudoElement::After)
            .unwrap()
            .content,
        Content::Normal
    );
    assert_eq!(
        result
            .styles
            .pseudo(find(&document, "empty"), PseudoElement::Before)
            .unwrap()
            .content,
        Content::Items(vec![ContentItem::Text(String::new())])
    );
    assert_eq!(
        result
            .styles
            .pseudo(find(&document, "none"), PseudoElement::Before)
            .unwrap()
            .content,
        Content::None
    );
    assert_eq!(
        result
            .styles
            .pseudo(find(&document, "normal"), PseudoElement::Before)
            .unwrap()
            .content,
        Content::Normal
    );
    assert!(result.stylesheets.diagnostics.len() >= 4);
}

#[test]
fn pseudo_rollback_and_registered_variables_reuse_computation_machinery() {
    let document = parser::parse(
        "<style>
        @property --size {syntax:'<length>'; inherits:false; initial-value:12px}
        @layer base, top;
        @layer base {p::before {content:'base'; color:red; font-size:var(--size)}}
        @layer top {p::before {content:revert-layer; color:revert-layer}}
        p {--size:30px; color:blue}
        p::after {content:inherit}
        </style><p id='target'>text</p>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let before = result
        .styles
        .pseudo(find(&document, "target"), PseudoElement::Before)
        .unwrap();
    assert_eq!(before.font_size, 12.0);
    assert_eq!(before.color, Some(0xff0000));
    assert_eq!(
        before.content,
        Content::Items(vec![ContentItem::Text("base".into())])
    );
}
