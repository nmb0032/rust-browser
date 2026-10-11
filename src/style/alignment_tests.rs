use super::*;
use crate::{
    css,
    dom::{Document, NodeId},
    html::parser,
    layout, paint,
};

fn id(document: &Document, name: &str) -> NodeId {
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

#[test]
fn pinned_shorthands_expand_and_reset_atomically_with_typed_properties() {
    use declarations::{DeclarationProperty, Property, Value};
    fn parse(source: &str) -> Vec<declarations::StyledDeclaration> {
        declarations::parse(&css::parse_declarations(source).unwrap().remove(0), 0).unwrap()
    }
    let text = parse("text-align:center");
    assert_eq!(text.len(), 2);
    assert!(matches!(
        text[0].property,
        DeclarationProperty::Longhand(Property::TextAlignAll)
    ));
    assert!(matches!(text[0].value, Value::TextAlign(TextAlign::Center)));
    assert!(matches!(
        text[1].value,
        Value::TextAlignLast(TextAlignLast::Auto)
    ));
    let baseline = parse("vertical-align:baseline");
    assert_eq!(baseline.len(), 3);
    assert!(matches!(
        baseline[0].value,
        Value::VerticalAlign(VerticalAlign::Baseline)
    ));
    assert!(matches!(
        baseline[1].value,
        Value::BaselineShift(BaselineShift::Zero)
    ));
    assert!(matches!(
        baseline[2].value,
        Value::BaselineSource(BaselineSource::Auto)
    ));
    for name in ["text-align", "vertical-align"] {
        for wide in ["initial", "inherit", "unset", "revert", "revert-layer"] {
            assert!(
                parse(&format!("{name}:{wide}"))
                    .iter()
                    .all(|declaration| matches!(declaration.value, Value::Wide(_)))
            );
        }
    }
    assert!(Property::TextAlignAll.inherited());
    assert!(Property::TextAlignLast.inherited());
    for property in [
        Property::AlignmentBaseline,
        Property::BaselineShift,
        Property::BaselineSource,
    ] {
        assert!(!property.inherited());
    }
}

#[test]
fn invalid_alignment_and_justification_are_rejected_without_claiming_support() {
    let document = parser::parse(
        "<style>#host{text-align:right}#target{text-align:left;text-align:justify;vertical-align:top}\
         @supports (text-align:justify){#target{color:red}}\
         @supports (text-align:center) and (vertical-align:baseline){#target{background:blue}}</style>\
         <div id='host'><span id='target'>H</span></div>"
    ).unwrap();
    let result = resolve(&document);
    let target = result.styles.get(id(&document, "target")).unwrap();
    assert_eq!(target.text_align, TextAlign::Left);
    assert_eq!(target.color, Some(0));
    assert_eq!(target.background_color, Some(0x0000ff));
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("text-align: justify"))
    );
    assert_eq!(target.vertical_align, VerticalAlign::Top);
    assert_eq!(
        metadata::property("text-align").unwrap().support,
        metadata::Support::Partial
    );
    assert_eq!(
        metadata::property("vertical-align").unwrap().support,
        metadata::Support::Partial
    );
    assert!(
        !matches!(metadata::property("text-align-all").unwrap().grammar, metadata::Grammar::Keywords(words) if words.contains(&"justify"))
    );
}

#[test]
fn variables_invalid_shorthands_inherit_atomically_and_wide_rules_use_initials() {
    let document = parser::parse(
        "<style>#host{text-align:right;direction:rtl;--align:center;--bad:justify;--baseline:baseline}\
         #valid{text-align:var(--align);vertical-align:var(--baseline)}\
         #invalid{text-align:left;text-align:var(--bad)}\
         #initial{text-align:initial}#inherit{text-align:inherit}#unset{text-align:unset}\
         #all{all:initial}</style>\
         <div id='host'><span id='valid'>H</span><span id='invalid'>H</span>\
         <span id='initial'>H</span><span id='inherit'>H</span><span id='unset'>H</span><span id='all'>H</span></div>"
    ).unwrap();
    let result = resolve(&document);
    for (name, align) in [
        ("valid", TextAlign::Center),
        ("invalid", TextAlign::Right),
        ("initial", TextAlign::Start),
        ("inherit", TextAlign::Right),
        ("unset", TextAlign::Right),
        ("all", TextAlign::Start),
    ] {
        let style = result.styles.get(id(&document, name)).unwrap();
        assert_eq!(style.text_align, align);
        assert_eq!(style.text_align_last, TextAlignLast::Auto);
        assert_eq!(style.vertical_align, VerticalAlign::Baseline);
        assert_eq!(style.baseline_shift, BaselineShift::Zero);
        assert_eq!(style.baseline_source, BaselineSource::Auto);
        assert_eq!(style.direction, Direction::Rtl); // all excludes direction
    }
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("text-align"))
    );
}

#[test]
fn alignment_respects_layers_origins_important_and_font_does_not_reset_it() {
    let document = parser::parse(
        "<style>@layer low, high;@layer low{#target{text-align:center}}\
         @layer high{#target{text-align:right;text-align:revert-layer}}\
         #important{text-align:left!important}#font{text-align:right;font:24px/30px serif}</style>\
         <div id='target'>H</div><div id='important' style='text-align:center'>H</div><div id='font'>H</div>"
    ).unwrap();
    let result = resolve(&document);
    assert!(result.stylesheets.diagnostics.is_empty());
    assert_eq!(
        result
            .styles
            .get(id(&document, "target"))
            .unwrap()
            .text_align,
        TextAlign::Center
    );
    assert_eq!(
        result
            .styles
            .get(id(&document, "important"))
            .unwrap()
            .text_align,
        TextAlign::Left
    );
    assert_eq!(
        result.styles.get(id(&document, "font")).unwrap().text_align,
        TextAlign::Right
    );
}

#[test]
fn anonymous_wrappers_inherit_alignment_and_dumps_expose_baseline_properties() {
    let document = parser::parse(
        "<div id='host' style='text-align:center;direction:rtl;font:16px/20px serif'>\
         Before<span style='display:block'>Block</span>After</div>",
    )
    .unwrap();
    let result = resolve(&document);
    let mut tree = layout::build_layout_tree_with_styles(&document, &result.styles);
    tree.layout(400.0, &paint::load_bundled_font().unwrap());
    fn host(root: &layout::LayoutBox, id: NodeId) -> Option<&layout::LayoutBox> {
        if root.dom_node_id() == Some(id) {
            Some(root)
        } else {
            root.children().iter().find_map(|child| host(child, id))
        }
    }
    let host = host(tree.root(), id(&document, "host")).unwrap();
    for child in [&host.children()[0], &host.children()[2]] {
        assert!(child.is_anonymous());
        assert_eq!(child.style().text_align, TextAlign::Center);
        assert_eq!(child.style().direction, Direction::Rtl);
        let line = &child.line_boxes()[0];
        assert!(
            (line.content_rect.x
                - (child.content_rect().x
                    + (child.content_rect().width - line.content_rect.width) / 2.0))
                .abs()
                < 0.0001
        );
        assert_eq!(line.rect.x, child.content_rect().x);
        assert_eq!(line.rect.width, child.content_rect().width);
    }
    for dump in [
        crate::devtools::format_style(&document, &result.styles),
        crate::devtools::format_values(&tree, 1.0),
    ] {
        for value in [
            "text-align: Center",
            "vertical-align: Baseline",
            "baseline-shift: Zero",
            "baseline-source: Auto",
        ] {
            assert!(dump.contains(value), "{dump}");
        }
    }
}
