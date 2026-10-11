use super::{
    ComputedStyle,
    declarations::{self, Property},
    generated_properties,
    metadata::{self, Grammar, Support},
    resolve,
};
use crate::{css, dom::Document, html::parser};
use std::collections::BTreeSet;

fn find(document: &Document, name: &str) -> crate::dom::NodeId {
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

fn style(source: &str) -> ComputedStyle {
    let document = parser::parse(&format!(
        "<span id='target' style='all:initial; {source}'>text</span>"
    ))
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{source}: {:?}",
        result.stylesheets.diagnostics
    );
    result
        .styles
        .get(find(&document, "target"))
        .unwrap()
        .clone()
}

fn projection(style: &ComputedStyle, property: Property) -> String {
    use Property as P;
    match property {
        P::Content => format!("{:?}", style.content),
        P::Color => format!("{:?}", style.text_color),
        P::TextFillColor => format!("{:?}", style.text_fill_color),
        P::BackgroundColor => format!("{:?}", style.background),
        P::BackgroundImage => format!("{:?}", style.background_image),
        P::Display => format!("{:?}", style.display),
        P::BoxSizing => format!("{:?}", style.box_sizing),
        P::FontSize => format!("{:?}", style.font_size),
        P::FontFamily => format!("{:?}", style.font_family),
        P::FontWeight => format!("{:?}", style.font_weight),
        P::FontStyle => format!("{:?}", style.font_style),
        P::LineHeight => format!("{:?}", style.line_height),
        P::Width => format!("{:?}", style.width),
        P::Height => format!("{:?}", style.height),
        P::ContainerType => format!("{:?}", style.container_type),
        P::ContainerName => format!("{:?}", style.container_names),
        P::Margin(side) => format!("{:?}", style.margin.get(side)),
        P::Padding(side) => format!("{:?}", style.padding.get(side)),
        P::BorderWidth(side) => format!("{:?}", style.border_width.get(side)),
        P::BorderStyle(side) => format!("{:?}", style.border_style.get(side)),
        P::BorderColor(side) => format!("{:?}", style.borders.get(side)),
    }
}

#[test]
fn every_generated_property_field_matches_the_frozen_public_snapshot() {
    let snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/css-spec/webref-css.json")).unwrap();
    let entries = snapshot["properties"].as_array().unwrap();
    assert_eq!(metadata::PROPERTIES.len(), entries.len());
    assert_eq!(entries.len(), 821);
    for entry in entries {
        let name = entry["name"].as_str().unwrap();
        let meta = metadata::property(name).unwrap();
        assert_eq!(meta.href, entry["href"].as_str().unwrap(), "{name}");
        for (actual, field) in [
            (meta.syntax, "syntax"),
            (meta.initial, "initial"),
            (meta.inheritance, "inherited"),
            (meta.computed_value, "computedValue"),
            (meta.animation_type, "animationType"),
            (meta.alias_of, "legacyAliasOf"),
        ] {
            assert_eq!(actual, entry[field].as_str(), "{name}: {field}");
        }
        assert_eq!(
            meta.inherited,
            match entry["inherited"].as_str() {
                Some("yes") => Some(true),
                Some("no") => Some(false),
                _ => None,
            },
            "{name}"
        );
        for (actual, field) in [
            (meta.longhands, "longhands"),
            (meta.reset_longhands, "resetLonghands"),
        ] {
            let expected: Vec<_> = entry[field]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| value.as_str().unwrap())
                .collect();
            assert_eq!(actual, expected, "{name}: {field}");
        }
    }
    for (generated, category) in [
        (metadata::TYPES, "types"),
        (metadata::FUNCTIONS, "functions"),
    ] {
        let source = snapshot[category].as_array().unwrap();
        assert_eq!(generated.len(), source.len());
        for entry in source {
            let scopes: Vec<_> = entry["for"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| value.as_str().unwrap())
                .collect();
            let meta = generated
                .iter()
                .find(|meta| Some(meta.name) == entry["name"].as_str() && meta.scopes == scopes)
                .unwrap();
            assert_eq!(Some(meta.href), entry["href"].as_str());
            assert_eq!(meta.syntax, entry["syntax"].as_str());
        }
    }
    assert_eq!(metadata::TYPES.len(), 529);
    assert_eq!(metadata::FUNCTIONS.len(), 161);
}

#[test]
fn registry_and_report_totals_are_exact_and_only_explicit_aliases_gain_acceptance() {
    let mut counts = [0usize; 4];
    let mut longhands = BTreeSet::new();
    for meta in metadata::PROPERTIES {
        counts[match meta.support {
            Support::Implemented => 0,
            Support::Partial => 1,
            Support::Unimplemented => 2,
            Support::AliasUnimplemented => 3,
        }] += 1;
        let properties = declarations::properties(meta.name);
        assert_eq!(
            properties.is_some(),
            meta.support == Support::Partial,
            "{}",
            meta.name
        );
        assert_eq!(meta.grammar != Grammar::Unsupported, properties.is_some());
        if let Some(properties) = properties
            && properties.len() == 1
            && meta.longhands.is_empty()
            && meta.alias_of.is_none()
        {
            assert_eq!(properties[0].name(), meta.name);
            assert_eq!(Some(properties[0].inherited()), meta.inherited);
            assert_eq!(
                generated_properties::initial_css(properties[0]),
                if meta.name == "font-family" {
                    "sans-serif"
                } else {
                    meta.initial.unwrap()
                },
                "{}: initial tokens",
                meta.name
            );
            assert_eq!(
                properties[0].computes_absolute_length(),
                meta.computed_value == Some("an absolute length")
            );
            longhands.insert(properties[0]);
        }
    }
    assert_eq!(counts, [0, 51, 707, 63]);
    assert_eq!(longhands.len(), 36);
    let all = declarations::properties("all").unwrap();
    assert_eq!(all.len(), 36);
    assert_eq!(all.into_iter().collect::<BTreeSet<_>>(), longhands);
    let report = metadata::css_coverage();
    assert!(report.contains("**821**; canonical properties: **757**; legacy aliases: **64**"));
    assert!(report.contains("**0 implemented**, **50 partial**, **707 unimplemented**"));
    assert!(report.contains("**1 accepted (partial targets)**, **63 unimplemented**"));
    assert!(report.contains("**36 longhands**, **14 shorthands**"));
    assert!(report.contains("Missing reset targets: border-image-source"));
    assert!(report.contains("Missing expansion targets: font-variant"));
    assert!(report.contains("height percentage/containment"));
    assert!(report.contains("border image-1D alternative"));
    let rows: Vec<_> = report
        .split("## All snapshot properties")
        .nth(1)
        .unwrap()
        .lines()
        .filter(|line| {
            line.starts_with("| ") && !line.starts_with("| Property") && !line.starts_with("| ---")
        })
        .collect();
    assert_eq!(rows.len(), 821);
    for meta in metadata::PROPERTIES {
        assert_eq!(
            rows.iter()
                .filter(|row| row.starts_with(&format!("| {} |", meta.name)))
                .count(),
            1
        );
    }
}

#[test]
fn representative_spec_rules_remain_distinct_from_engine_policy_and_support() {
    let font = metadata::property("FONT-SIZE").unwrap();
    assert_eq!(font.initial, Some("medium"));
    assert_eq!(font.inherited, Some(true));
    assert_eq!(font.computed_value, Some("an absolute length"));
    assert_eq!(font.animation_type, Some("by computed value type"));
    assert!(font.syntax.unwrap().contains("<length-percentage [0,∞]>"));
    let margin = metadata::property("margin").unwrap();
    assert_eq!(margin.syntax, Some("<'margin-top'>{1,4}"));
    assert_eq!(margin.initial, Some("0"));
    assert_eq!(margin.inherited, Some(false));
    assert_eq!(
        margin.longhands,
        &["margin-top", "margin-right", "margin-bottom", "margin-left"]
    );
    let color = metadata::property("color").unwrap();
    assert_eq!(color.initial, Some("CanvasText"));
    assert_eq!(color.grammar, Grammar::ColorSubset);
    assert_eq!(color.support, Support::Partial);
    assert_eq!(
        metadata::property("background-color").unwrap().initial,
        Some("transparent")
    );
    assert_eq!(
        metadata::property("border-top-color").unwrap().initial,
        Some("currentcolor")
    );
    assert!(
        metadata::property("border-top-color")
            .unwrap()
            .syntax
            .unwrap()
            .contains("<image-1D>")
    );
    assert_eq!(
        metadata::property("font-family").unwrap().initial,
        Some("depends on user agent")
    );
    assert_eq!(
        generated_properties::initial_css(Property::FontFamily),
        "sans-serif"
    );
    let alias = metadata::property("font-stretch").unwrap();
    assert_eq!(alias.alias_of, Some("font-width"));
    assert_eq!(alias.initial, None);
    assert_eq!(alias.inherited, None);
    assert_eq!(alias.support, Support::AliasUnimplemented);
    assert_eq!(alias.grammar, Grammar::Unsupported);
}

#[test]
fn all_generated_initials_match_existing_defaults_and_every_longhand_wide_rule() {
    assert_eq!(style(""), ComputedStyle::default());
    let inherited: Vec<_> = declarations::properties("all")
        .unwrap()
        .into_iter()
        .filter(|property| property.inherited())
        .map(Property::name)
        .collect();
    assert_eq!(
        inherited,
        &[
            "color",
            "-webkit-text-fill-color",
            "font-size",
            "font-family",
            "font-weight",
            "font-style",
            "line-height"
        ]
    );
    for &(name, sample) in generated_properties::SAMPLES {
        let properties = declarations::properties(name).unwrap();
        if properties.len() != 1 || !metadata::property(name).unwrap().longhands.is_empty() {
            continue;
        }
        let property = properties[0];
        let source = format!(
            "<div id='parent' style='all:initial; border-style:solid; {name}:{sample}'>
            <span id='initial' style='all:initial; border-style:solid; {name}:initial'>text</span>
            <span id='inherit' style='all:initial; border-style:solid; {name}:inherit'>text</span>
            <span id='unset' style='all:initial; border-style:solid; {name}:unset'>text</span></div>"
        );
        let document = parser::parse(&source).unwrap();
        let result = resolve(&document);
        assert!(
            result.stylesheets.diagnostics.is_empty(),
            "{name}: {:?}",
            result.stylesheets.diagnostics
        );
        let value = |id| projection(result.styles.get(find(&document, id)).unwrap(), property);
        let default = projection(
            &style(&format!("border-style:solid; {name}:initial")),
            property,
        );
        assert_eq!(value("initial"), default, "{name}: initial");
        assert_eq!(value("inherit"), value("parent"), "{name}: inherit");
        assert_eq!(
            value("unset"),
            if property.inherited() {
                value("parent")
            } else {
                default
            },
            "{name}: unset"
        );
        let initial = declarations::initial_value(property);
        assert!(!matches!(initial, declarations::Value::Wide(_)), "{name}");
    }
}

#[test]
fn every_generated_sample_and_keyword_has_finite_atomic_dispatch() {
    for &(name, sample) in generated_properties::SAMPLES {
        let properties = declarations::properties(name).unwrap();
        let declaration = css::parse_declarations(&format!("{name}:{sample}")).unwrap();
        let values = declarations::parse(&declaration[0], 0).unwrap();
        assert_eq!(values.len(), properties.len(), "{name}");
        for (value, property) in values.iter().zip(&properties) {
            assert!(
                matches!(value.property, declarations::DeclarationProperty::Longhand(p) if p == *property)
            );
        }
        for keyword in ["inherit", "initial", "unset", "revert", "revert-layer"] {
            let declaration = css::parse_declarations(&format!("{name}:{keyword}")).unwrap();
            assert_eq!(
                declarations::parse(&declaration[0], 0).unwrap().len(),
                properties.len()
            );
        }
        let declaration = css::parse_declarations(&format!("{name}:var(--value)")).unwrap();
        assert_eq!(
            declarations::parse(&declaration[0], 0).unwrap().len(),
            properties.len()
        );
    }
    for meta in metadata::PROPERTIES {
        if let Grammar::Keywords(keywords) = meta.grammar {
            for keyword in keywords {
                let declaration =
                    css::parse_declarations(&format!("{}:{keyword}", meta.name)).unwrap();
                assert!(
                    declarations::parse(&declaration[0], 0).is_ok(),
                    "{}:{keyword}",
                    meta.name
                );
            }
        }
    }
}

#[test]
fn generated_shorthand_targets_keep_semantic_order_and_resets() {
    for (shorthand, expanded) in [
        (
            "margin:1px 2px 3px 4px",
            "margin-top:1px;margin-right:2px;margin-bottom:3px;margin-left:4px",
        ),
        (
            "padding:1px 2px 3px 4px",
            "padding-top:1px;padding-right:2px;padding-bottom:3px;padding-left:4px",
        ),
        (
            "background:red",
            "background-color:red;background-image:none",
        ),
        (
            "font:italic bold 20px/2 serif",
            "font-style:italic;font-weight:bold;font-size:20px;line-height:2;font-family:serif",
        ),
        (
            "container:card / size",
            "container-name:card;container-type:size",
        ),
        (
            "border:4px solid red",
            "border-width:4px;border-style:solid;border-color:red",
        ),
        (
            "border-top:4px solid red",
            "border-top-width:4px;border-top-style:solid;border-top-color:red",
        ),
        (
            "border-right:4px solid red",
            "border-right-width:4px;border-right-style:solid;border-right-color:red",
        ),
        (
            "border-bottom:4px solid red",
            "border-bottom-width:4px;border-bottom-style:solid;border-bottom-color:red",
        ),
        (
            "border-left:4px solid red",
            "border-left-width:4px;border-left-style:solid;border-left-color:red",
        ),
        (
            "border-width:1px 2px 3px 4px",
            "border-top-width:1px;border-right-width:2px;border-bottom-width:3px;border-left-width:4px",
        ),
        (
            "border-style:solid dashed dotted double",
            "border-top-style:solid;border-right-style:dashed;border-bottom-style:dotted;border-left-style:double",
        ),
        (
            "border-color:red blue green purple",
            "border-top-color:red;border-right-color:blue;border-bottom-color:green;border-left-color:purple",
        ),
    ] {
        let direct = style(shorthand);
        let expanded = style(expanded);
        // Compare every actual typed field, not specified-source debug maps.
        for property in declarations::properties("all").unwrap() {
            assert_eq!(
                projection(&direct, property),
                projection(&expanded, property),
                "{shorthand}: {property:?}"
            );
        }
    }
    assert_eq!(
        style("font:italic bold 20px/2 serif; font:24px sans-serif").font_style,
        crate::css::values::FontStyle::Normal
    );
    assert_eq!(
        style("border:8px dashed blue; border:solid")
            .border_width
            .top,
        3.0
    );
    assert_eq!(
        style("container:card / size; container:other").container_type,
        css::container::ContainerType::Normal
    );
}

fn spec_targets(name: &str) -> Vec<&'static str> {
    let meta = metadata::property(name).unwrap();
    if let Some(alias) = meta.alias_of {
        return spec_targets(alias);
    }
    if meta.longhands.is_empty() {
        return vec![meta.name];
    }
    meta.longhands
        .iter()
        .flat_map(|name| spec_targets(name))
        .collect()
}

#[test]
fn every_shorthand_wide_rule_matches_its_actual_supported_spec_longhands() {
    let all = declarations::properties("all").unwrap();
    let parent = "all:initial; content:\"parent\"; color:red; background:blue;
                display:block; font:italic bold 24px/2 serif; width:42px;height:36px;
                container:card / size; margin:1px 2px 3px 4px; padding:5px 6px 7px 8px;
                border:7px dashed purple";
    for &(name, _) in generated_properties::SAMPLES {
        if name != "all" && metadata::property(name).unwrap().longhands.is_empty() {
            continue;
        }
        let targets = if name == "all" {
            all.iter()
                .map(|property| property.name())
                .collect::<Vec<_>>()
        } else {
            spec_targets(name)
                .into_iter()
                .filter(|name| {
                    declarations::properties(name).is_some_and(|properties| {
                        properties.len() == 1 && properties[0].name() == *name
                    })
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            declarations::properties(name)
                .unwrap()
                .iter()
                .map(|property| property.name())
                .collect::<BTreeSet<_>>(),
            targets.iter().copied().collect::<BTreeSet<_>>(),
            "{name}: supported spec expansion"
        );
        for keyword in ["initial", "inherit", "unset"] {
            let expanded = targets
                .iter()
                .map(|name| format!("{name}:{keyword};"))
                .collect::<String>();
            let document = parser::parse(&format!(
                        "<div style='{parent}'>
                        <span id='direct' style='all:initial;border-style:solid;{name}:{keyword}'>text</span>
                        <span id='expanded' style='all:initial;border-style:solid;{expanded}'>text</span></div>"
                    )).unwrap();
            let result = resolve(&document);
            assert!(
                result.stylesheets.diagnostics.is_empty(),
                "{name}:{keyword}: {:?}",
                result.stylesheets.diagnostics
            );
            let direct = result.styles.get(find(&document, "direct")).unwrap();
            let expanded = result.styles.get(find(&document, "expanded")).unwrap();
            for &property in &all {
                assert_eq!(
                    projection(direct, property),
                    projection(expanded, property),
                    "{name}:{keyword}: {property:?}"
                );
            }
        }
    }
    let initial_border = style("color:red; border-style:solid; border-color:initial");
    assert_eq!(initial_border.borders.top, initial_border.text_color);
    assert_eq!(
        style("color:red;background-color:initial").background,
        css::values::ResolvedColor::TRANSPARENT
    );
}

#[test]
fn missing_unknown_and_unsupported_metadata_never_becomes_a_catch_all_parser() {
    for name in ["", "made-up-property", "--custom", "--CUSTOM"] {
        assert!(metadata::property(name).is_none(), "{name}");
        assert!(declarations::properties(name).is_none(), "{name}");
    }
    for source in [
        "opacity:0.5",
        "font-stretch:normal",
        "-webkit-text-stroke-color:red",
        "display:flex",
        "display:grid",
        "width:min-content",
        "height:fit-content",
        "margin:auto",
        "container-type:scroll-state",
        "border-style:inset",
        "border-top-color:stripes(red, blue)",
        "background-image:url(x)",
        "all:revert-rule",
    ] {
        let declarations = css::parse_declarations(source).unwrap();
        assert!(
            declarations::parse(&declarations[0], 0).is_err(),
            "{source}"
        );
    }
    let document = parser::parse("<span id='target' style='width:42px; opacity:0.5; made-up:yes; font-stretch:normal'>text</span>").unwrap();
    let result = resolve(&document);
    assert_eq!(
        result.styles.get(find(&document, "target")).unwrap().width,
        Some(css::values::Length::Px(42.0))
    );
    assert_eq!(result.stylesheets.diagnostics.len(), 3);
    assert!(
        result
            .stylesheets
            .diagnostics
            .iter()
            .all(|d| d.message.contains("unsupported CSS property"))
    );
}

#[test]
fn custom_registration_spec_metadata_does_not_replace_runtime_required_rules() {
    let descriptors = metadata::PROPERTY_DESCRIPTORS;
    assert_eq!(descriptors.len(), 3);
    let inherits = descriptors
        .iter()
        .find(|entry| entry.name == "inherits")
        .unwrap();
    assert_eq!(inherits.syntax, Some("true | false"));
    assert_eq!(inherits.initial, Some("true"));
    let syntax = descriptors
        .iter()
        .find(|entry| entry.name == "syntax")
        .unwrap();
    assert_eq!(syntax.syntax, Some("<string>"));
    assert_eq!(syntax.initial, Some("\"*\""));
    let initial = descriptors
        .iter()
        .find(|entry| entry.name == "initial-value")
        .unwrap();
    assert_eq!(initial.syntax, Some("<declaration-value>?"));
    assert_eq!(initial.initial, Some("the guaranteed-invalid value"));
    let document = parser::parse(
        "<style>@property --size {syntax:'<length>'; inherits:false; initial-value:2px}
        #parent {--size:7px; --plain:9px}
        #target {width:var(--size); height:var(--plain)}</style>
        <div id='parent'><span id='target'>text</span></div>",
    )
    .unwrap();
    let result = resolve(&document);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let computed = result.styles.get(find(&document, "target")).unwrap();
    assert_eq!(computed.width, Some(css::values::Length::Px(2.0)));
    assert_eq!(computed.height, Some(css::values::Length::Px(9.0)));
    for descriptors in [
        "syntax:'<length>';initial-value:2px",
        "syntax:'<length>';inherits:false",
        "syntax:'<length> | <color>';inherits:false;initial-value:2px",
    ] {
        let document = parser::parse(&format!(
            "<style>@property --size {{{descriptors}}}</style>"
        ))
        .unwrap();
        assert!(
            !resolve(&document).stylesheets.diagnostics.is_empty(),
            "{descriptors}"
        );
    }
}
