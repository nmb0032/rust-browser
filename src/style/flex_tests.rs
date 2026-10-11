use super::*;
use crate::{css::values::Length, html::parser};

fn computed(css: &str) -> ComputedStyle {
    let (style, diagnostics) = computed_with_diagnostics(css);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    style
}

fn computed_with_diagnostics(css: &str) -> (ComputedStyle, Vec<Diagnostic>) {
    let document = parser::parse(&format!(
        "<style>{css}</style><main id='host'><div id='item'></div></main>"
    ))
    .unwrap();
    let styles = resolve(&document);
    let id = document
        .preorder()
        .find(|&id| {
            document
                .node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some("item"))
        })
        .unwrap();
    (
        styles.styles.get(id).unwrap().clone(),
        styles.stylesheets.diagnostics,
    )
}

#[test]
fn flex_shorthand_keywords_factors_basis_order_and_initial_reset_are_typed() {
    for (value, grow, shrink, basis) in [
        ("none", 0.0, 0.0, FlexBasis::Auto),
        ("auto", 1.0, 1.0, FlexBasis::Auto),
        ("initial", 0.0, 1.0, FlexBasis::Auto),
        ("2", 2.0, 1.0, FlexBasis::Length(Length::Percent(0.0))),
        ("2 3", 2.0, 3.0, FlexBasis::Length(Length::Percent(0.0))),
        ("10px", 1.0, 1.0, FlexBasis::Length(Length::Px(10.0))),
        (
            "20% 2 3",
            2.0,
            3.0,
            FlexBasis::Length(Length::Percent(20.0)),
        ),
        ("2 3 content", 2.0, 3.0, FlexBasis::Content),
        ("2 3 0", 2.0, 3.0, FlexBasis::Length(Length::Px(0.0))),
    ] {
        let style = computed(&format!("#item{{flex:8 9 30px;flex:{value}}}"));
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (grow, shrink, basis),
            "{value}"
        );
    }
}

#[test]
fn flex_lengths_use_selected_font_root_viewport_variables_and_atomic_shorthands() {
    let style = computed(
        ":root{font-size:20px}#host{--f:2 3 2rem;--flow:wrap column-reverse;--g:1rem 2em}#item{font-size:10px;flex:var(--f);flex-flow:var(--flow);gap:var(--g);min-width:0;max-height:50%;order:-2;align-self:safe center}",
    );
    assert_eq!(style.flex_basis, FlexBasis::Length(Length::Px(40.0)));
    assert_eq!(style.row_gap, Some(Length::Px(20.0)));
    assert_eq!(style.column_gap, Some(Length::Px(20.0)));
    assert_eq!(style.flex_direction, FlexDirection::ColumnReverse);
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
    assert_eq!(style.order, -2);
    assert!(style.align_self.safe);
    let inherited = computed(
        "#host{flex:3 4 content;gap:8px;align-items:end}#item{flex:inherit;gap:inherit;align-items:inherit}",
    );
    assert_eq!(
        (
            inherited.flex_grow,
            inherited.flex_shrink,
            inherited.flex_basis
        ),
        (3.0, 4.0, FlexBasis::Content)
    );
    assert_eq!(inherited.row_gap, Some(Length::Px(8.0)));
    let not_inherited = computed("#host{flex:3 4 content;gap:8px;order:10;align-items:end}");
    assert_eq!(not_inherited.flex_grow, 0.0);
    assert_eq!(not_inherited.order, 0);
    assert_eq!(not_inherited.row_gap, None);
}

#[test]
fn flex_invalid_shorthands_and_property_values_are_rejected_atomically() {
    for source in [
        "flex:-1",
        "flex:1 20px 2",
        "flex:1 2 3",
        "flex:none 1",
        "flex-flow:row column",
        "flex-flow:wrap wrap",
        "gap:1px -2px",
        "order:1.0",
        "flex-grow:-1",
        "flex-basis:-1px",
        "min-width:-1px",
        "align-items:space-between",
        "justify-content:baseline",
        "align-self:safe stretch",
        "align-content:last center",
        "display:masonry",
    ] {
        let declarations = crate::css::parse_declarations(source).unwrap();
        assert!(
            super::declarations::parse(&declarations[0], 0).is_err(),
            "{source}"
        );
    }
    let (style, diagnostics) = computed_with_diagnostics(
        "#item{flex:2 3 10px;gap:8px;flex-flow:column wrap;flex:var(--missing);gap:var(--missing);flex-flow:var(--missing)}",
    );
    assert_eq!(diagnostics.len(), 3);
    assert_eq!(
        (style.flex_grow, style.flex_shrink, style.flex_basis),
        (0.0, 1.0, FlexBasis::Auto)
    );
    assert_eq!(style.row_gap, None);
    assert_eq!(style.flex_direction, FlexDirection::Row);
    assert_eq!(style.flex_wrap, FlexWrap::Nowrap);
}
