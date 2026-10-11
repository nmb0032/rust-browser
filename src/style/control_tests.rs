use super::*;
use crate::{
    css::{media::MediaEnvironment, values::Length},
    html::parser,
};

fn find(document: &crate::dom::Document, name: &str) -> crate::dom::NodeId {
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

fn page(css: &str, body: &str) -> crate::dom::Document {
    parser::parse(&format!("<!doctype html><style>{css}</style>{body}")).unwrap()
}

#[test]
fn layer_registry_is_global_nested_anonymous_ordered_and_reversed_for_important() {
    let document = page(
        "@layer base, theme; @layer base { @layer reset, widgets; \
        @layer reset {p {color:red !important}} @layer widgets {p {color:blue !important}} \
        p {color:green !important; width:10px} } \
        @layer theme {p {color:purple !important; width:20px}} \
        @layer {p {width:30px}} p {width:40px}",
        "<style>@layer base {p {width:50px}} @layer theme {p {width:60px}}</style>\
        <p id='target'>text</p><p id='inline' style='color:blue !important; width:70px'>text</p>",
    );
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0xff0000)
    );
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().width,
        Some(Length::Px(40.0))
    );
    let inline = result.styles.get(find(&document, "inline")).unwrap();
    assert_eq!(inline.color, Some(0x0000ff));
    assert_eq!(inline.width, Some(Length::Px(70.0)));
}

#[test]
fn conditional_layer_creation_changes_order_but_empty_active_blocks_reserve_layers() {
    let document = page(
        "@media (min-width:600px) {@layer a {}} @layer b {p {color:blue}} @layer a {p {color:red}}",
        "<p id='target'>text</p>",
    );
    for (width, color) in [(400.0, 0xff0000), (600.0, 0x0000ff)] {
        let result = resolve_with_environment(&document, MediaEnvironment::new(width, 600.0));
        assert_eq!(
            result.styles.get(find(&document, "target")).unwrap().color,
            Some(color)
        );
    }
}

#[test]
fn rollback_uses_origin_and_layer_candidates_with_custom_values_and_pending_shorthands() {
    let document = page(
        "@layer first, second; @layer first {p {color:red; margin:2px; --x:blue}} \
        @layer second {p {color:green; color:revert-layer; margin:8px; margin:var(--rollback,revert-layer); --x:green; --x:revert-layer}} \
        p {background-color:var(--x)}",
        "<p id='target' style='color:revert-layer'>text</p><p id='origin' style='color:revert; --x:revert'>text</p>",
    );
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &["p {color:purple; --x:yellow}"],
    );
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let target = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(target.color, Some(0xff0000));
    assert_eq!(target.margin.top, Length::Px(2.0).into());
    assert_eq!(target.background_color, Some(0x0000ff));
    let origin = result.styles.get(find(&document, "origin")).unwrap();
    assert_eq!(origin.color, Some(0x800080));
    assert_eq!(origin.background_color, Some(0xffff00));

    let document = page(
        "@layer a, b; @layer a {p {margin:2px}} @layer b {p {margin:8px; margin:var(--missing,revert-layer)}}",
        "<p id='target'>text</p>",
    );
    let result = resolve(&document);
    assert_eq!(
        result
            .styles
            .get(find(&document, "target"))
            .unwrap()
            .margin
            .left,
        Length::Px(2.0).into()
    );
}

#[test]
fn important_rollback_and_shorthand_invalidity_do_not_revive_same_layer_declarations() {
    let document = page(
        "@layer a, b; @layer b {p {color:blue !important; --x:blue !important}} \
        @layer a {p {color:red !important; color:revert-layer !important; --x:red !important; --x:revert-layer !important}} \
        p {background:var(--x); margin:3px; margin:var(--missing)}",
        "<p id='target'>text</p>",
    );
    let result = resolve(&document);
    let target = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(target.color, Some(0));
    assert_eq!(target.background_color, None);
    assert_eq!(target.margin.top, Length::Px(0.0).into());
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &["p {color:revert !important}"],
    );
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x000000)
    );
}

#[test]
fn important_layer_rollback_discards_intervening_layers_but_inline_exception_keeps_them() {
    let document = page(
        "@layer earlier, rollback, intervening; @layer earlier {p {color:purple; --x:purple}} \
        @layer rollback {p {color:revert-layer !important; --x:revert-layer !important}} \
        @layer intervening {p {color:red !important; --x:red !important}} \
        p {background:var(--x)}",
        "<p id='target'>text</p><p id='inline' style='color:revert-layer !important'>inline</p>",
    );
    let result = resolve(&document);
    for name in ["target", "inline"] {
        let target = result.styles.get(find(&document, name)).unwrap();
        assert_eq!(target.color, Some(0x800080));
        assert_eq!(target.background_color, Some(0x800080));
    }
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &["@layer a {p {color:green}} @layer b {p {color:revert-layer !important}}"],
    );
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x008000)
    );

    let document = page(
        "@layer a {p {color:blue !important}}",
        "<p id='target' style='color:revert-layer !important'>text</p>",
    );
    assert_eq!(
        resolve(&document)
            .styles
            .get(find(&document, "target"))
            .unwrap()
            .color,
        Some(0x0000ff)
    );
}

#[test]
fn nesting_preserves_parent_list_max_specificity_combinators_and_declaration_order() {
    let document = page(
        ".parent, #absent {color:red; & > p {color:blue} \
        @supports (display:block) {@media screen {& > p {width:12px}}} \
        & + p {color:purple} } \
        #target {color:green} .parent {color:blue; & {color:red} color:green} \
        .parent { > p {padding:3px} }",
        "<div class='parent' id='parent'><p id='target'>inside</p></div><p id='sibling'>sibling</p>",
    );
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let target = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(target.color, Some(0x0000ff));
    assert_eq!(target.width, Some(Length::Px(12.0)));
    assert_eq!(target.padding.top, Length::Px(3.0));
    assert_eq!(
        result.styles.get(find(&document, "sibling")).unwrap().color,
        Some(0x800080)
    );
    assert_eq!(
        result.styles.get(find(&document, "parent")).unwrap().color,
        Some(0x008000)
    );
}

#[test]
fn supports_reuses_real_grammars_and_boolean_unknown_false_conditions() {
    let document = page(
        "p {color:red} @supports (display:grid) {p {color:blue}} \
        @supports (display:block) and (color:oklch(50% .1 30)) {p {width:12px}} \
        @supports not future(foo) {p {padding:2px}} \
        @supports selector(p > .child) or (display:grid) {p {color:green}} \
        @supports (display:block;) {p {color:blue}} \
        @supports (display:block !important) {p {color:blue}} \
        @supports (display:block) or (color:red) and (width:1px) {p {color:blue}}",
        "<p id='target'>text</p>",
    );
    let result = resolve(&document);
    let target = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(target.color, Some(0x008000));
    assert_eq!(target.width, Some(Length::Px(12.0)));
    assert_eq!(target.padding.top, Length::Px(2.0));
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unknown @supports function"))
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("invalid @supports"))
    );
}

#[test]
fn scopes_include_roots_exclude_limit_subtrees_and_compare_proximity_after_specificity() {
    let document = page(
        "p {color:black} @scope (.outer) to (.stop) {:scope {background:red} p {color:red}} \
        @scope (.inner) {p {color:blue}} \
        @scope (.outer) {p {color:green} #specific {color:purple}}",
        "<div class='outer' id='outer'><div class='inner'><p id='target'>text</p><p id='specific'>specific</p></div>\
        <div class='stop'><p id='limited'>limit</p></div></div><p id='outside'>outside</p>",
    );
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    assert_eq!(
        result
            .styles
            .get(find(&document, "outer"))
            .unwrap()
            .background_color,
        Some(0xff0000)
    );
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        result
            .styles
            .get(find(&document, "specific"))
            .unwrap()
            .color,
        Some(0x800080)
    );
    assert_eq!(
        result.styles.get(find(&document, "limited")).unwrap().color,
        Some(0x008000)
    );
    assert_eq!(
        result.styles.get(find(&document, "outside")).unwrap().color,
        Some(0)
    );
}

#[test]
fn scopes_anchor_relative_selectors_and_direct_declarations_without_leaking_external_ancestors() {
    let document = page(
        "@scope (.component) to (> .stop) {background:yellow; \
        > p {color:blue} body p {color:red} body :scope p {padding:2px} \
        & {border:1px solid green} \
        @scope (> .inner) {> p {color:purple}} }",
        "<main class='component' id='root'><p id='direct'>direct</p>\
        <div class='inner'><p id='inner'>inner</p></div><div class='stop'><p id='stopped'>stopped</p></div></main>",
    );
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let root = result.styles.get(find(&document, "root")).unwrap();
    assert_eq!(root.background_color, Some(0xffff00));
    assert_eq!(root.border_width.top, 1.0);
    assert_eq!(
        result.styles.get(find(&document, "direct")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        result.styles.get(find(&document, "inner")).unwrap().color,
        Some(0x800080)
    );
    assert_eq!(
        result
            .styles
            .get(find(&document, "direct"))
            .unwrap()
            .padding
            .top,
        Length::Px(2.0)
    );
    assert_eq!(
        result.styles.get(find(&document, "stopped")).unwrap().color,
        Some(0)
    );
}

#[test]
fn implicit_scope_uses_stylesheet_owner_parent_and_root_specificity_is_not_added() {
    let document = parser::parse("<!doctype html><style>p {color:black} #target {color:purple}</style>\
        <section id='section'><style>@scope {p {color:blue} & {background:yellow}}</style>\
        <p id='target'>specificity</p><p id='inside'>inside</p></section><p id='outside'>outside</p>").unwrap();
    let result = resolve(&document);
    assert_eq!(
        result
            .styles
            .get(find(&document, "section"))
            .unwrap()
            .background_color,
        Some(0xffff00)
    );
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x800080)
    );
    assert_eq!(
        result.styles.get(find(&document, "inside")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        result.styles.get(find(&document, "outside")).unwrap().color,
        Some(0)
    );
}

#[test]
fn all_rolls_back_supported_longhands_without_resetting_custom_properties_or_presentational_hints()
{
    let document = page(
        "@layer a {p {color:blue; width:9px; margin:2px; --x:purple}} \
        @layer b {p {all:var(--absent,revert-layer)}} \
        @layer images {img {width:12px}}",
        "<p id='target'>text</p><img id='image' width='50'>",
    );
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let target = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(target.width, Some(Length::Px(9.0)));
    assert_eq!(target.margin.top, Length::Px(2.0).into());
    assert_eq!(target.color, Some(0x0000ff));
    assert!(target.custom_properties.contains_key("--x"));
    assert_eq!(
        result.styles.get(find(&document, "image")).unwrap().width,
        Some(Length::Px(12.0))
    );

    let document = page("img {width:revert-layer}", "<img id='image' width='50'>");
    assert_eq!(
        resolve(&document)
            .styles
            .get(find(&document, "image"))
            .unwrap()
            .width,
        Some(Length::Px(50.0))
    );
    let document = page("img {width:revert}", "<img id='image' width='50'>");
    assert_eq!(
        resolve(&document)
            .styles
            .get(find(&document, "image"))
            .unwrap()
            .width,
        None
    );
}

#[test]
fn namespace_attribute_selectors_compare_uri_not_prefix_and_xml_local_names_are_sensitive() {
    use crate::dom::{Attribute, Document, ElementData, Namespace};
    let mut document = Document::new();
    let root = document.root_id();
    let style_node = document
        .append_element(root, ElementData::new("style".into(), vec![]))
        .unwrap();
    document.append_text(style_node, "@namespace x 'urn:attribute'; @namespace e 'urn:element'; \
        e|Box[x|flag='yes'] {color:red} e|box {color:blue} [|flag] {width:7px} [*|flag] {padding:2px}".into()).unwrap();
    let target = document
        .append_element(
            root,
            ElementData::xml(
                "Box".into(),
                vec![Attribute::namespaced(
                    "flag".into(),
                    "yes".into(),
                    "urn:attribute".into(),
                    Some("different".into()),
                )],
                Namespace::Other("urn:element".into()),
                Some("different".into()),
            ),
        )
        .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let target = result.styles.get(target).unwrap();
    assert_eq!(target.color, Some(0xff0000));
    assert_eq!(target.width, None);
    assert_eq!(target.padding.top, Length::Px(2.0));
}

#[test]
fn namespaces_match_foreign_dom_uri_and_unknown_prefix_and_placement_are_invalid() {
    let document = page(
        "@namespace svg 'http://www.w3.org/2000/svg'; \
        @namespace 'http://www.w3.org/1999/xhtml'; \
        svg|a {color:red} a {color:blue} *|a {width:7px} \
        missing|a {color:purple} @namespace late 'wrong'; late|a {color:green}",
        "<a id='html'>html</a><svg><a id='svg'>svg</a></svg>",
    );
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "html")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        result.styles.get(find(&document, "svg")).unwrap().color,
        Some(0xff0000)
    );
    for name in ["html", "svg"] {
        assert_eq!(
            result.styles.get(find(&document, name)).unwrap().width,
            Some(Length::Px(7.0))
        );
    }
    assert_eq!(result.stylesheets.diagnostics.len(), 3);
}

#[test]
fn registered_values_compute_before_var_inherit_or_initial_and_invalid_values_default() {
    let document = page(
        "@property --size {syntax:'<length>'; inherits:false; initial-value:2in} \
        @property --tone {syntax:'<color>'; inherits:true; initial-value:red} \
        @property --n {syntax:'<number>'; inherits:false; initial-value:1} \
        main {--size:20px; --tone:blue} \
        p {width:var(--size); color:var(--tone); line-height:var(--n)} \
        #invalid {--size:red; --tone:nonsense} #valid {--size:calc(1in + 4px); --n:calc(2 + 1)}",
        "<main><p id='default'>default</p><p id='invalid'>invalid</p><p id='valid'>valid</p></main>",
    );
    let result = resolve(&document);
    for name in ["default", "invalid"] {
        let target = result.styles.get(find(&document, name)).unwrap();
        assert_eq!(target.width, Some(Length::Px(192.0)));
        assert_eq!(target.color, Some(0x0000ff));
    }
    assert_eq!(
        result.styles.get(find(&document, "valid")).unwrap().width,
        Some(Length::Px(100.0))
    );
    assert_eq!(
        result
            .styles
            .get(find(&document, "valid"))
            .unwrap()
            .line_height,
        crate::css::values::LineHeight::Number(3.0)
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
}

#[test]
fn unsupported_registration_and_dependent_initial_values_are_explicitly_rejected() {
    let document = page(
        "@property --a {syntax:'<length>'; inherits:false; initial-value:1em} \
        @property --b {syntax:'<length> | <color>'; inherits:false; initial-value:red} \
        @property --c {syntax:'<color>'; inherits:false; initial-value:currentcolor} \
        @property --d {syntax:'<number>'; initial-value:1} \
        @property --e {syntax:'*'; inherits:false} \
        p {width:var(--e,7px); --a:10px}",
        "<p id='target'>text</p>",
    );
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().width,
        Some(Length::Px(7.0))
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 4);
}

#[test]
fn registered_cycles_css_wide_inheritance_and_universal_tokens_have_correct_defaults() {
    let document = page(
        "@property --n {syntax:'<number>'; inherits:false; initial-value:2} \
        @property --len {syntax:'<length>'; inherits:true; initial-value:10px} \
        @property --tokens {syntax:'*'; inherits:false} \
        main {--n:3; --len:20px; --tokens:blue} \
        p {width:var(--len); line-height:var(--n); color:var(--tokens,red)} \
        #inherited {--n:inherit; --len:initial; --tokens:inherit} \
        #cycle {--n:var(--n); --len:var(--len)}",
        "<main><p id='default'>default</p><p id='inherited'>inherit</p><p id='cycle'>cycle</p></main>",
    );
    let result = resolve(&document);
    let default = result.styles.get(find(&document, "default")).unwrap();
    assert_eq!(default.width, Some(Length::Px(20.0)));
    assert_eq!(
        default.line_height,
        crate::css::values::LineHeight::Number(2.0)
    );
    assert_eq!(default.color, Some(0xff0000));
    let inherited = result.styles.get(find(&document, "inherited")).unwrap();
    assert_eq!(inherited.width, Some(Length::Px(10.0)));
    assert_eq!(
        inherited.line_height,
        crate::css::values::LineHeight::Number(3.0)
    );
    assert_eq!(inherited.color, Some(0x0000ff));
    let cycle = result.styles.get(find(&document, "cycle")).unwrap();
    assert_eq!(cycle.width, Some(Length::Px(20.0)));
    assert_eq!(
        cycle.line_height,
        crate::css::values::LineHeight::Number(2.0)
    );
}

#[test]
fn invalid_layer_paths_are_rejected_and_default_is_an_ordinary_layer_name() {
    let document = page(
        "@layer default {p {color:blue}} @layer invalid . path {p {color:red}} \
        @layer invalid. path {p {color:red}} @layer initial {p {color:red}}",
        "<p id='target'>text</p>",
    );
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 3);
}

#[test]
fn registration_name_collisions_follow_origin_layer_order_and_conditional_activation() {
    let document = page(
        "@layer a,b; @layer b {@property --size {syntax:'<length>'; inherits:false; initial-value:30px}} \
        @layer a {@property --size {syntax:'<length>'; inherits:false; initial-value:10px}} \
        @media (min-width:600px) {@property --size {syntax:'<length>'; inherits:false; initial-value:50px}} \
        p {width:var(--size)}",
        "<p id='target'>text</p>",
    );
    for (width, size) in [(400.0, 30.0), (600.0, 50.0)] {
        let result = resolve_with_user_stylesheets_and_environment(
            &document,
            collect_stylesheets(&document),
            &["@property --size {syntax:'<length>'; inherits:false; initial-value:90px}"],
            MediaEnvironment::new(width, 600.0),
        );
        assert_eq!(
            result.styles.get(find(&document, "target")).unwrap().width,
            Some(Length::Px(size))
        );
    }
}

#[test]
fn cascade_controls_reach_actual_raster_pixels() {
    let document = page(
        "@layer a,b; @layer a {p {color:red}} \
        @layer b {@scope (main) {@supports (display:block) {p {color:blue; background:yellow; width:80px; margin:0}}}} body {margin:0}",
        "<main><p id='target'>text</p></main>",
    );
    let result = resolve(&document);
    let font = crate::paint::load_bundled_font().unwrap();
    let mut tree = crate::layout::build_layout_tree_with_styles(&document, &result.styles);
    tree.layout(200.0, &font);
    let display = crate::paint::build_display_list(&tree, 200.0, 100.0, false);
    let mut pixels = vec![0xffffff; 200 * 100];
    crate::paint::rasterize_display_list(&display, &font, &mut pixels, 200, 100, 1.0, 0.0);
    assert!(pixels.contains(&0xffff00));
    assert!(
        pixels
            .iter()
            .any(|&pixel| pixel & 0xff > (pixel >> 16) & 0xff)
    );
}

#[cfg(feature = "style-bench-reference")]
#[test]
fn cascade_control_is_identical_in_all_matching_modes() {
    let document = page(
        "@layer a,b; @layer a {p {color:red}} @layer b {.parent,#absent {& > p {color:blue} @scope (.inner) {p {width:8px}}}} @supports (display:block) {p {margin:var(--none,revert-layer)}}",
        "<div class='parent'><div class='inner'><p id='target'>text</p></div><p>child</p></div>",
    );
    let reference = resolve_for_benchmark(&document, BenchmarkMatching::Unindexed);
    for mode in [
        BenchmarkMatching::Indexed,
        BenchmarkMatching::IndexedWithBloom,
    ] {
        let result = resolve_for_benchmark(&document, mode);
        for node in document.preorder() {
            assert_eq!(result.styles.get(node), reference.styles.get(node));
        }
        assert_eq!(
            result.stylesheets.diagnostics,
            reference.stylesheets.diagnostics
        );
    }
}
