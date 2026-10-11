use super::*;
use crate::{
    css::{
        self,
        media::MediaEnvironment,
        tokenizer::{MAX_PARSE_ERRORS, TokenKind},
        values::{BorderStyle, Edges, FamilyName, Length, LineHeight},
    },
    dom::{Document, NodeId},
    engine::Frame,
    html::parser,
};
use std::fmt::Write;

fn find(document: &Document, name: &str) -> NodeId {
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

fn resolve_inline(source: &str) -> (Document, StyleResolution) {
    let document = parser::parse(&format!("<p id='target' style='{source}'>theme</p>")).unwrap();
    let result = resolve(&document);
    (document, result)
}

fn clean_style(source: &str) -> ComputedStyle {
    let (document, result) = resolve_inline(source);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    result
        .styles
        .get(find(&document, "target"))
        .unwrap()
        .clone()
}

fn without_custom(mut style: ComputedStyle) -> ComputedStyle {
    style.custom_properties.clear();
    style
}

#[test]
fn case_sensitive_custom_names_unicode_and_escaped_names_use_decoded_tokens() {
    let style = clean_style(
        r"--tone:red; --Tone:blue; --\66 oo:green; --é:purple;
        COLOR:VaR(--tone); background:var(--Tone); border:1px solid var(--foo);
        border-left-color:var(--\e9); --空:yellow; border-right-color:var(--空)",
    );
    assert_eq!(style.color, Some(0xff0000));
    assert_eq!(style.background_color, Some(0x0000ff));
    assert_eq!(style.border_color.top, Some(0x008000));
    assert_eq!(style.border_color.left, Some(0x800080));
    assert_eq!(style.border_color.right, Some(0xffff00));
    assert!(style.custom_properties.contains_key("--foo"));
    assert!(!style.custom_properties.contains_key(r"--\66 oo"));
    assert_eq!(
        clean_style("--Tone:red; color:var(--tone, blue)").color,
        Some(0x0000ff)
    );
}

#[test]
fn declaration_ast_preserves_whitespace_and_comment_boundaries_before_importance() {
    let declaration = css::parse_declarations(r"--\78: 1/**/px  ! IMPORTANT")
        .unwrap()
        .remove(0);
    assert_eq!(declaration.property(), "--x");
    assert!(declaration.is_important());
    assert_eq!(declaration.value(), "1/**/px");
    assert!(matches!(declaration.value_tokens(), [
        TokenKind::Whitespace, TokenKind::Number(_), TokenKind::Ident(unit), TokenKind::Whitespace
    ] if unit == "px"));
}

#[test]
fn inheritance_uses_already_computed_parent_tokens_not_child_dependencies() {
    let document = parser::parse(
        "<main style='--base:red; --alias:var(--base);
        --bad:var(--missing); --initial:blue; --inherited:purple; --unset:yellow; color:green'>
        <p id='target' style='--base:blue; --missing:green; --fresh:var(--base);
        --initial:INITIAL; --inherited:INHERIT; --unset:UnSeT;
        color:var(--alias); background:var(--fresh);
        border:solid var(--bad, navy); border-left-color:var(--initial, black)'>theme</p>
        <p id='grandchild' style='color:var(--bad, blue)'>other</p></main>",
    )
    .unwrap();
    let result = resolve(&document);
    let target = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(target.color, Some(0xff0000));
    assert_eq!(target.background_color, Some(0x0000ff));
    assert_eq!(target.border_color.top, Some(0x000080));
    assert_eq!(target.border_color.left, Some(0));
    assert_eq!(target.custom_properties.get("--initial"), Some(&None));
    assert_eq!(target.custom_properties.get("--bad"), Some(&None));
    assert_eq!(
        result
            .styles
            .get(find(&document, "grandchild"))
            .unwrap()
            .color,
        Some(0x0000ff)
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 1);
    let initial =
        clean_style("--a:inherit; --b:unset; color:var(--a, red); background:var(--b, blue)");
    assert_eq!(initial.custom_properties.get("--a"), Some(&None));
    assert_eq!(initial.custom_properties.get("--b"), Some(&None));
}

#[test]
fn missing_empty_and_comma_rich_nested_fallbacks_are_distinct() {
    let style = clean_style(
        "--empty:; --space: ; --channels:1, 2, 3;
        --nested:var(--absent, var(--also-absent, rgb(var(--channels))));
        color:var(--nested); background:var(--missing, var(--other, blue));
        font-family:var(--missing, Noto Sans, serif, monospace);
        border:var(--empty, bad) solid var(--space, bad) red;
        padding:var(--missing,) 2px; margin:var(--missing, var(--empty)) 3px",
    );
    assert_eq!(style.color, Some(0x010203));
    assert_eq!(style.background_color, Some(0x0000ff));
    assert_eq!(style.border_style, Edges::all(BorderStyle::Solid));
    assert_eq!(style.border_width, Edges::all(3.0));
    assert_eq!(style.padding, Edges::all(Length::Px(2.0)));
    assert_eq!(style.margin, Edges::all(Length::Px(3.0).into()));
    assert_eq!(
        style.font_family.0,
        [
            FamilyName::Named("Noto Sans".into()),
            FamilyName::Generic("serif".into()),
            FamilyName::Generic("monospace".into())
        ]
    );
    assert_eq!(
        style.custom_properties["--empty"].as_ref().unwrap().len(),
        0
    );
    assert!(matches!(
        style.custom_properties["--space"].as_deref(),
        Some([TokenKind::Whitespace])
    ));
}

#[test]
fn fallback_is_not_a_property_grammar_retry_and_invalid_winners_become_unset() {
    let document = parser::parse("<main style='color:green; font:italic bold 24px/2 serif'>
        <p id='target' style='--bad:not-a-color; --empty:; --len:bad;
        color:red; color:var(--bad, blue); width:99px; width:var(--missing);
        background:red; background:var(--empty, blue); font-size:50px; font-size:var(--len, 40px);
        border:8px solid red; border:var(--bad); margin:9px; margin:var(--missing)'>text</p></main>").unwrap();
    let result = resolve(&document);
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.color, Some(0x008000));
    assert_eq!(style.font_size, 24.0);
    assert_eq!(style.width, None);
    assert_eq!(style.background_color, None);
    assert_eq!(style.margin, Edges::all(Length::Px(0.0).into()));
    assert_eq!(style.border_style, Edges::all(BorderStyle::None));
    assert_eq!(style.border_width, Edges::all(0.0));
    assert_eq!(result.stylesheets.diagnostics.len(), 6);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("computed-value time"))
    );
}

#[test]
fn cycles_include_unused_fallback_edges_and_other_properties_can_recover() {
    let (document, result) = resolve_inline(
        "--good:red;
        --a:var(--good, var(--b)); --b:var(--a);
        --self:var(--good, var(--self)); --simple:var(--simple);
        --outer:var(--a, var(--good)); --recover:var(--self, blue);
        color:var(--outer); background:var(--recover); border:solid var(--b, green)",
    );
    let style = result.styles.get(find(&document, "target")).unwrap();
    for name in ["--a", "--b", "--self", "--simple"] {
        assert_eq!(style.custom_properties.get(name), Some(&None), "{name}");
    }
    assert_eq!(style.color, Some(0xff0000));
    assert_eq!(style.background_color, Some(0x0000ff));
    assert_eq!(style.border_color, Edges::all(Some(0x008000)));
    assert_eq!(result.stylesheets.diagnostics.len(), 1);
    assert!(result.stylesheets.diagnostics[0].message.contains("cycle"));
}

#[test]
fn cycle_edges_use_only_winning_declarations_and_case_sensitive_names() {
    let style = clean_style(
        "--a:var(--b); --a:red; --b:var(--a); --A:var(--b);
        color:var(--A); --case:var(--CASE, blue); background:var(--case)",
    );
    assert_eq!(style.color, Some(0xff0000));
    assert_eq!(style.background_color, Some(0x0000ff));
    let style = clean_style("--a:red !important; --a:var(--a); color:var(--a)");
    assert_eq!(style.color, Some(0xff0000));
}

#[test]
fn substitution_never_creates_identifiers_or_dimensions_across_token_boundaries() {
    for value in [
        "--number:1; width:var(--number)px",
        "--number:1; --unit:px; width:var(--number)var(--unit)",
        "--dim:1/**/px; width:var(--dim)",
        "--dim:1 px; width:var(--dim)",
    ] {
        let (document, result) = resolve_inline(&format!("width:88px; {value}"));
        assert_eq!(
            result.styles.get(find(&document, "target")).unwrap().width,
            None,
            "{value}"
        );
        assert_eq!(result.stylesheets.diagnostics.len(), 1, "{value}");
    }
    for value in [
        "--s:s; border:var(--s)olid red",
        "--s:s; --tail:olid; border:var(--s)var(--tail) red",
        "--s:s/**/olid; border:var(--s) red",
    ] {
        let (document, result) = resolve_inline(&format!("border:9px solid blue; {value}"));
        assert_eq!(
            result
                .styles
                .get(find(&document, "target"))
                .unwrap()
                .border_width,
            Edges::all(0.0),
            "{value}"
        );
        assert_eq!(result.stylesheets.diagnostics.len(), 1, "{value}");
    }
    assert_eq!(
        clean_style("--x:1px/**/2px; margin:var(--x)").margin.right,
        Length::Px(2.0).into()
    );
    assert_eq!(
        clean_style("--channels:1 2 3; color:rgb(var(--channels))").color,
        Some(0x010203)
    );
    let (document, result) =
        resolve_inline("--a:1; --b:2; --c:3; color:red; color:rgb(var(--a)var(--b)var(--c))");
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0)
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 1);
}

const SHORTHANDS: &[(&str, &str)] = &[
    ("margin", "1px 2px 3px 4px"),
    ("padding", "1px 2px 3px 4px"),
    ("border-width", "1px 2px 3px 4px"),
    ("border-color", "red green blue black"),
    ("border-style", "solid dashed dotted double"),
    ("border", "2px solid red"),
    ("border-top", "2px solid red"),
    ("border-right", "2px solid red"),
    ("border-bottom", "2px solid red"),
    ("border-left", "2px solid red"),
    ("font", "italic 650 20px/1.5 Noto Sans, serif"),
    ("background", "none red"),
];

#[test]
fn every_shorthand_expands_after_substitution_and_invalid_expansion_is_atomic() {
    for (name, value) in SHORTHANDS {
        let variable = clean_style(&format!("--x:{value}; {name}:var(--x)"));
        let direct = clean_style(&format!("{name}:{value}"));
        assert_eq!(without_custom(variable), without_custom(direct), "{name}");
        let (document, result) = resolve_inline(&format!(
            "{name}:{value}; --bad:invalid-value; {name}:var(--bad)"
        ));
        assert_eq!(result.stylesheets.diagnostics.len(), 1, "{name}");
        let computed = result
            .styles
            .get(find(&document, "target"))
            .unwrap()
            .clone();
        assert_eq!(
            without_custom(computed),
            without_custom(clean_style(&format!("{name}:unset"))),
            "{name}"
        );
    }
}

#[test]
fn shorthand_longhand_winners_priority_and_omitted_resets_survive_substitution() {
    let style = clean_style(
        "--border:solid; --font:20px serif; --m:2px 3px; --bg:none;
        border:9px dashed red; border:var(--border); border-top-color:blue;
        border-right-width:8px !important; border-right:var(--border);
        border-left:4px double purple !important; border-left-color:var(--missing);
        margin-top:9px; margin:var(--m); margin-left:7px !important;
        padding:var(--m) !important; padding-left:12px;
        font:italic bold 40px/2 monospace; font:var(--font); font-weight:650 !important;
        background:red; background:var(--bg)",
    );
    assert_eq!(
        style.border_width,
        Edges {
            top: 3.0,
            right: 8.0,
            bottom: 3.0,
            left: 4.0
        }
    );
    assert_eq!(
        style.border_color,
        Edges {
            top: Some(0x0000ff),
            right: Some(0),
            bottom: Some(0),
            left: Some(0x800080)
        }
    );
    assert_eq!(style.border_style.left, BorderStyle::Double);
    assert_eq!(
        style.margin,
        Edges {
            top: Length::Px(2.0),
            right: Length::Px(3.0),
            bottom: Length::Px(2.0),
            left: Length::Px(7.0)
        }
        .map(Into::into)
    );
    assert_eq!(style.padding.left, Length::Px(3.0));
    assert_eq!(style.font_size, 20.0);
    assert!(!style.italic);
    assert_eq!(style.font_weight.0, 650.0);
    assert_eq!(style.line_height, LineHeight::Normal);
    assert_eq!(style.background_color, None);
}

#[test]
fn all_supported_longhands_parse_substituted_tokens_like_direct_values() {
    for (name, value) in [
        ("color", "rgb(1, 2, 3)"),
        ("background-color", "blue"),
        ("background-image", "none"),
        ("display", "block"),
        ("font-size", "120%"),
        ("font-family", "Noto Sans, serif"),
        ("font-weight", "650.5"),
        ("font-style", "oblique"),
        ("line-height", "1.25"),
        ("width", "3em"),
        ("margin-top", "-1em"),
        ("margin-right", "2px"),
        ("margin-bottom", "30%"),
        ("margin-left", "4px"),
        ("padding-top", "1em"),
        ("padding-right", "2px"),
        ("padding-bottom", "30%"),
        ("padding-left", "4px"),
        ("border-top-width", "2px"),
        ("border-right-width", "medium"),
        ("border-bottom-width", "3em"),
        ("border-left-width", "thick"),
        ("border-top-color", "red"),
        ("border-right-color", "currentcolor"),
        ("border-bottom-color", "transparent"),
        ("border-left-color", "blue"),
        ("border-top-style", "solid"),
        ("border-right-style", "dashed"),
        ("border-bottom-style", "dotted"),
        ("border-left-style", "double"),
    ] {
        assert_eq!(
            without_custom(clean_style(&format!("--value:{value};{name}:var(--value)"))),
            without_custom(clean_style(&format!("{name}:{value}"))),
            "{name}"
        );
    }
}

#[test]
fn custom_component_blocks_strings_and_escaped_importance_are_preserved() {
    let style = clean_style(
        r#"--color:red; --string:"var(--missing)"; --block:{value:var(--color); bang:!};
        --square:[var(--color), (a; b)]; --n:2em; --chosen:blue !im\70 ortant; --chosen:green;
        color:var(--chosen); font-size:10px; width:var(--n);
        font-family:var(--string)"#,
    );
    assert_eq!(style.color, Some(0x0000ff));
    assert_eq!(style.width, Some(Length::Px(20.0)));
    assert_eq!(
        style.font_family.0,
        [FamilyName::Named("var(--missing)".into())]
    );
    let block = style.custom_properties["--block"].as_ref().unwrap();
    assert!(
        block
            .iter()
            .any(|t| matches!(t,TokenKind::Ident(name) if name == "red"))
    );
    assert!(
        !block
            .iter()
            .any(|t| matches!(t,TokenKind::Function(name) if name == "var"))
    );
    let style = clean_style(
        "color:var(--missing, initial); --a:blue; --a:initial; background:var(--a, red)",
    );
    assert_eq!(style.color, Some(0));
    assert_eq!(style.background_color, Some(0xff0000));
}

#[test]
fn invalid_variable_shorthand_does_not_touch_other_winners_or_recover_older_values() {
    let (document, result) = resolve_inline(
        "margin:9px; margin:var(--missing) !important;
        margin-left:7px !important; margin-top:8px;
        border:9px solid red; border:var(--missing); border-left:4px double blue",
    );
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(
        style.margin,
        Edges {
            top: Length::Px(0.0),
            right: Length::Px(0.0),
            bottom: Length::Px(0.0),
            left: Length::Px(7.0)
        }
        .map(Into::into)
    );
    assert_eq!(
        style.border_width,
        Edges {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 4.0
        }
    );
    assert_eq!(style.border_color.left, Some(0x0000ff));
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
}

#[test]
fn syntax_errors_are_ignored_but_unknown_properties_are_still_diagnosed() {
    for invalid in [
        "color:var()",
        "color:var(red)",
        "color:var(--)",
        "color:var(--x extra)",
        "color:var(--x,,var(bad))",
        "color:var(--x, red ! nope)",
        "--x:red ! nope",
        "--x:var(--)",
    ] {
        let (document, result) = resolve_inline(&format!("--x:blue; color:red; {invalid}"));
        assert_eq!(
            result.styles.get(find(&document, "target")).unwrap().color,
            Some(0xff0000),
            "{invalid}"
        );
        assert_eq!(
            result.stylesheets.diagnostics.len(),
            1,
            "{invalid}: {:?}",
            result.stylesheets.diagnostics
        );
    }
    let (_, result) = resolve_inline("unknown:var(--x); --:red");
    assert_eq!(result.stylesheets.diagnostics.len(), 2);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unsupported CSS property"))
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("InvalidPropertyName"))
    );
}

#[test]
fn inline_custom_importance_and_user_author_origin_order_share_the_normal_cascade() {
    let document = parser::parse("<style>#target {--c:red !important; --m:3px !important; color:var(--c);
        margin:var(--m)}</style><p id='target' style='--c:purple !important; --m:5px; color:var(--c)'>text</p>").unwrap();
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &[
            "#target {--c:green; --m:7px}",
            "p {--c:blue !important; --m:9px !important}",
        ],
    );
    assert!(result.stylesheets.diagnostics.is_empty());
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.color, Some(0x0000ff));
    assert_eq!(style.margin, Edges::all(Length::Px(9.0).into()));
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x800080)
    );
}

#[test]
fn media_environment_restyles_recompute_inherited_variables_without_accumulating_diagnostics() {
    let document = parser::parse(
        "<style>:root {--theme:red; --size:20px}
        @media (min-width:600px) {:root {--theme:blue; --size:40px}}
        p {color:var(--theme); width:var(--size); padding:var(--missing)}</style>
        <style media='(max-width:500px)'>:root {--theme:green}</style><p id='target'>text</p>",
    )
    .unwrap();
    let mut frame = Frame::new_with_environment(
        url::Url::parse("https://example.test/").unwrap(),
        document,
        MediaEnvironment::new(400.0, 600.0),
    );
    let node = find(&frame.document, "target");
    let ast = frame.stylesheets.stylesheets[0].stylesheet.clone();
    for (width, color, size) in [
        (400.0, 0x008000, 20.0),
        (600.0, 0x0000ff, 40.0),
        (550.0, 0xff0000, 20.0),
        (400.0, 0x008000, 20.0),
    ] {
        frame.set_media_environment(MediaEnvironment::new(width, 600.0));
        let style = frame.styles.get(node).unwrap();
        assert_eq!(style.color, Some(color));
        assert_eq!(style.width, Some(Length::Px(size)));
        assert_eq!(frame.stylesheets.diagnostics.len(), 1);
        assert!(std::sync::Arc::ptr_eq(
            &ast,
            &frame.stylesheets.stylesheets[0].stylesheet
        ));
    }
}

#[test]
fn deterministic_style_dump_includes_computed_empty_and_invalid_token_values() {
    let (document, result) =
        resolve_inline("--z:var(--absent); --a:1/**/px; --empty:; --m:var(--absent, red)");
    let output = crate::devtools::format_style(&document, &result.styles);
    assert_eq!(
        output,
        crate::devtools::format_style(&document, &resolve(&document).styles)
    );
    let target = &output[output.find("id=Some(\"target\")").unwrap()..];
    assert!(target.find("--a:").unwrap() < target.find("--empty:").unwrap());
    assert!(target.find("--empty:").unwrap() < target.find("--m:").unwrap());
    assert!(target.find("--m:").unwrap() < target.find("--z:").unwrap());
    assert!(target.contains("--z: GuaranteedInvalid"));
    assert!(target.contains("--empty: Tokens([])"));
    assert!(target.contains("Number(Numeric"));
    assert!(target.contains("Ident(\"px\")"));
    assert!(!target.contains("var("));
}

#[test]
fn long_dependency_chains_and_deep_nested_fallbacks_do_not_use_the_call_stack() {
    let mut source = "--v0:3px;".to_string();
    for index in 1..3_000 {
        write!(source, "--v{index}:var(--v{});", index - 1).unwrap();
    }
    source.push_str("width:var(--v2999);");
    assert_eq!(clean_style(&source).width, Some(Length::Px(3.0)));
    let depth = 3_000;
    let source = format!(
        "width:{}4px{};",
        "var(--missing,".repeat(depth),
        ")".repeat(depth)
    );
    assert_eq!(clean_style(&source).width, Some(Length::Px(4.0)));
}

#[test]
fn expansion_limits_invalidate_winners_allow_fallbacks_and_bound_diagnostics() {
    let mut source = "--v0:x;".to_string();
    for index in 1..18 {
        write!(
            source,
            "--v{index}:var(--v{})var(--v{});",
            index - 1,
            index - 1
        )
        .unwrap();
    }
    source.push_str("color:red; color:var(--v17); background:var(--v17, blue);");
    let (document, result) = resolve_inline(&source);
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.custom_properties.get("--v17"), Some(&None));
    assert_eq!(style.color, Some(0));
    assert_eq!(style.background_color, Some(0x0000ff));
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("resource limit"))
    );
    assert!(
        style
            .custom_properties
            .values()
            .flatten()
            .all(|tokens| tokens.len() <= super::variables::MAX_VALUE_TOKENS)
    );
    let mut source = String::new();
    for index in 0..600 {
        write!(source, "--v{index}:var(--missing);").unwrap();
    }
    let (_, result) = resolve_inline(&source);
    assert_eq!(result.stylesheets.diagnostics.len(), MAX_PARSE_ERRORS);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.len() < 256)
    );
    assert!(
        result
            .stylesheets
            .diagnostics
            .last()
            .unwrap()
            .message
            .contains("omitted")
    );
    let (_, result) = resolve_inline(&"--x:var(bad);".repeat(600));
    assert_eq!(result.stylesheets.diagnostics.len(), MAX_PARSE_ERRORS);
    assert!(
        result
            .stylesheets
            .diagnostics
            .last()
            .unwrap()
            .message
            .contains("omitted")
    );
}

#[test]
fn oversized_raw_values_and_environments_fail_closed_without_reviving_earlier_values() {
    let source = format!(
        "--x:red; --x:{}; color:var(--x,blue); width:8px; width:var(--missing,{});",
        "x ".repeat(super::variables::MAX_VALUE_TOKENS),
        "1px ".repeat(super::variables::MAX_VALUE_TOKENS)
    );
    let (document, result) = resolve_inline(&source);
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.custom_properties.get("--x"), Some(&None));
    assert_eq!(style.color, Some(0x0000ff));
    assert_eq!(style.width, None);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("resource limit"))
    );
    let mut source = String::new();
    for index in 0..=super::variables::MAX_CUSTOM_PROPERTIES {
        write!(source, "--v{index}:red;").unwrap();
    }
    source.push_str("color:var(--v0, blue)");
    let (document, result) = resolve_inline(&source);
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.color, Some(0x0000ff));
    assert!(style.custom_properties.is_empty());
    assert_eq!(result.stylesheets.diagnostics.len(), 1);
}

#[test]
fn byte_and_aggregate_token_limits_bound_raw_input_and_expanded_values() {
    let payload = "x".repeat(super::variables::MAX_VALUE_BYTES + 1);
    let (document, result) =
        resolve_inline(&format!("--x:red; --x:{payload}; color:var(--x, blue)"));
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().color,
        Some(0x0000ff)
    );
    assert!(
        result.stylesheets.diagnostics[0]
            .message
            .contains("resource limit")
    );
    let payload = "x".repeat(super::variables::MAX_VALUE_BYTES / 2 + 1);
    let (document, result) = resolve_inline(&format!(
        "--x:{payload}; --double:var(--x)var(--x); color:var(--double, blue)"
    ));
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.custom_properties.get("--double"), Some(&None));
    assert_eq!(style.color, Some(0x0000ff));
    assert!(
        result.stylesheets.diagnostics[0]
            .message
            .contains("resource limit")
    );
    let value = "x ".repeat(7_000);
    let mut source = String::new();
    for index in 0..5 {
        write!(source, "--v{index}:{value};").unwrap();
    }
    source.push_str("color:var(--v0, blue)");
    let (document, result) = resolve_inline(&source);
    let style = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(style.custom_properties.get("--v0"), Some(&None));
    assert_eq!(style.color, Some(0x0000ff));
    assert!(
        result.stylesheets.diagnostics[0]
            .message
            .contains("resource limit")
    );
}

#[test]
fn variable_syntax_diagnostics_are_bounded_across_multiple_stylesheet_origins() {
    let mut html = String::new();
    for _ in 0..5 {
        html.push_str(&format!(
            "<style>p {{{}}}</style>",
            "--x:var(bad);".repeat(100)
        ));
    }
    html.push_str("<p id='target'>text</p>");
    let document = parser::parse(&html).unwrap();
    let user = format!("p {{{}}}", "--x:var(bad);".repeat(100));
    let result = resolve_with_user_stylesheets(
        &document,
        collect_stylesheets(&document),
        &[&user, &user, &user],
    );
    assert_eq!(result.stylesheets.diagnostics.len(), MAX_PARSE_ERRORS);
    assert!(
        result
            .stylesheets
            .diagnostics
            .last()
            .unwrap()
            .message
            .contains("omitted")
    );
}
