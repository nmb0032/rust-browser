use super::*;
use crate::{css, css::values::Length, html::parser};

fn computed(source: &str) -> (ComputedStyle, Vec<Diagnostic>) {
    let doc = parser::parse(&format!(
        "<style>{source}</style><main id='host'><span id='item'></span></main>"
    ))
    .unwrap();
    let resolved = resolve(&doc);
    let id = doc
        .preorder()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some("item"))
        })
        .unwrap();
    (
        resolved.styles.get(id).unwrap().clone(),
        resolved.stylesheets.diagnostics,
    )
}

#[test]
fn grid_tracks_units_fonts_variables_shorthands_and_blockification_are_typed() {
    let (style, diagnostics) = computed(
        ":root{font-size:20px}#host{display:grid;--cols:[begin] repeat(2, minmax(0px,1fr)) fit-content(2rem) [end];--area:hero}#item{display:inline-grid;font-size:10px;grid-template-columns:var(--cols);grid-template-rows:2em 10%;grid-area:var(--area);grid-auto-flow:dense column;place-items:safe center stretch}",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(style.display, Display::Grid);
    assert_eq!(
        style.grid_auto_flow,
        GridAutoFlow {
            column: true,
            dense: true
        }
    );
    assert_eq!(
        style.grid_template_rows,
        vec![
            GridTrack::Size(TrackSize::Breadth(TrackBreadth::Length(Length::Px(20.0)))),
            GridTrack::Size(TrackSize::Breadth(TrackBreadth::Length(Length::Percent(
                10.0
            ))))
        ]
    );
    assert!(
        style
            .grid_template_columns
            .contains(&GridTrack::Size(TrackSize::FitContent(Length::Px(40.0))))
    );
    let name = GridLine::Line(1, Some("hero".into()));
    assert_eq!(style.grid_row_start, name);
    assert_eq!(style.grid_row_end, name);
    assert_eq!(style.grid_column_start, name);
    assert_eq!(style.grid_column_end, name);
    assert_eq!(
        style.align_items,
        Alignment {
            keyword: AlignKeyword::Center,
            safe: true
        }
    );
    assert_eq!(style.justify_items.keyword, AlignKeyword::Stretch);
}

#[test]
fn placement_omissions_are_names_only_and_invalid_shorthands_are_atomic() {
    for (value, end) in [
        ("hero", GridLine::Line(1, Some("hero".into()))),
        ("hero 1", GridLine::Auto),
        ("2", GridLine::Auto),
    ] {
        let (style, diagnostics) = computed(&format!("#item{{grid-column:{value}}}"));
        assert!(diagnostics.is_empty());
        assert_eq!(style.grid_column_end, end, "{value}");
    }
    let (style, diagnostics) = computed(
        "#item{grid-area:1/2/3/4;grid-area:2/0;grid-column:2 / span 3;grid-column:1 / span -2;place-self:end center;place-self:safe stretch;grid-template-columns:40px;grid-template-columns:minmax(1fr,2fr)}",
    );
    assert_eq!(diagnostics.len(), 4, "{diagnostics:?}");
    assert_eq!(style.grid_row_start, GridLine::Line(1, None));
    assert_eq!(style.grid_row_end, GridLine::Line(3, None));
    assert_eq!(style.grid_column_start, GridLine::Line(2, None));
    assert_eq!(style.grid_column_end, GridLine::Span(3, None));
    assert_eq!(style.align_self.keyword, AlignKeyword::End);
}

#[test]
fn invalid_grid_grammar_is_rejected_and_next_declaration_recovers() {
    for value in [
        "grid-template-columns:repeat(0,1fr)",
        "grid-template-columns:repeat(2,repeat(2,1fr))",
        "grid-template-columns:repeat(auto-fit,1fr)",
        "grid-template-columns:repeat(auto-fill,40px) repeat(auto-fit,40px)",
        "grid-template-columns:repeat(auto-fill,40px) auto",
        "grid-template-columns:-1fr",
        "grid-template-columns:minmax(1fr,auto)",
        "grid-template-columns:fit-content(-10px)",
        "grid-template-columns:[span] 10px",
        "grid-template-columns:subgrid",
        "grid-template-areas:'a a' 'a b'",
        "grid-template-areas:'a b' 'a'",
        "grid-column:0",
        "grid-row:span",
        "grid-row:span 1.0",
        "grid-area:1/2/3/4/5",
        "grid-auto-flow:row column",
        "grid-auto-flow:dense dense",
        "grid-auto-rows:repeat(2,10px)",
        "grid-auto-columns:none",
        "justify-items:space-around",
        "justify-self:legacy center",
    ] {
        let declarations = css::parse_declarations(value).unwrap();
        assert!(
            super::declarations::parse(&declarations[0], 0).is_err(),
            "{value}"
        );
    }
    let (style, diagnostics) =
        computed("#item{grid-template-columns:subgrid;width:42px;display:grid}");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(style.width, Some(Length::Px(42.0)));
    assert_eq!(style.display, Display::Grid);
}

#[test]
fn grid_wide_var_invalid_computed_and_layer_rollback_reset_all_targets() {
    let (style, diagnostics) = computed(
        "@layer low,high;@layer low{#item{grid-column:2/4;grid-template-columns:40px}}@layer high{#item{grid-column:5/8;grid-column:revert-layer;grid-template-columns:revert-layer}}#item{grid-row:2/3;grid-row:var(--missing)}",
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(style.grid_column_start, GridLine::Line(2, None));
    assert_eq!(style.grid_column_end, GridLine::Line(4, None));
    assert_eq!(style.grid_row_start, GridLine::Auto);
    assert_eq!(style.grid_row_end, GridLine::Auto);
    let (style, diagnostics) = computed(
        "#host{grid-column:2/4;grid-template-columns:30px;justify-items:end}#item{grid-column:inherit;grid-template-columns:inherit;justify-items:inherit}",
    );
    assert!(diagnostics.is_empty());
    assert_eq!(style.grid_column_end, GridLine::Line(4, None));
    assert_eq!(style.justify_items.keyword, AlignKeyword::End);
    let (style, _) =
        computed("#item{grid-template-columns:30px;grid-column:2/4;justify-items:end;all:initial}");
    assert!(style.grid_template_columns.is_empty());
    assert_eq!(style.grid_column_start, GridLine::Auto);
    assert_eq!(style.justify_items.keyword, AlignKeyword::Normal);
}

#[test]
fn grid_areas_dot_cells_and_supports_use_real_grammar() {
    let (style, diagnostics) = computed(
        "#item{grid-template-areas:'a..b' 'c..d'}@supports (display:grid) and (grid-template-columns:repeat(auto-fit,minmax(40px,1fr))){#item{width:17px}}@supports(grid-template-columns:subgrid){#item{width:99px}}",
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        style.grid_template_areas[0],
        vec![Some("a".into()), None, Some("b".into())]
    );
    assert_eq!(style.width, Some(Length::Px(17.0)));
}
