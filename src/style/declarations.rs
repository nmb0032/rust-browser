use crate::css::{
    Declaration,
    values::{Color, Length, Side, expand_four, parse_color},
    without_comments,
};

use super::Display;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Property {
    Color,
    BackgroundColor,
    Display,
    FontSize,
    FontFamily,
    FontWeight,
    FontStyle,
    Width,
    Margin(Side),
    Padding(Side),
    BorderWidth(Side),
    BorderColor(Side),
}

impl Property {
    pub(super) fn inherited(self) -> bool {
        matches!(
            self,
            Self::Color | Self::FontSize | Self::FontFamily | Self::FontWeight | Self::FontStyle
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum WideKeyword {
    Inherit,
    Initial,
    Unset,
}

#[derive(Clone, Debug)]
pub(super) enum Value {
    Wide(WideKeyword),
    Color(Color),
    Display(Display),
    Length(Length),
    Auto,
    Family(String),
    Bold(bool),
    Italic(bool),
}

#[derive(Clone, Debug)]
pub(super) struct StyledDeclaration {
    pub property: Property,
    pub value: Value,
    pub important: bool,
    pub order: usize,
}

pub(super) fn parse(
    declaration: &Declaration,
    order: usize,
) -> Result<Vec<StyledDeclaration>, String> {
    let properties = properties(&declaration.property().to_ascii_lowercase())
        .ok_or_else(|| format!("unsupported CSS property: {}", declaration.property()))?;
    let source = without_comments(declaration.value()).ok_or_else(|| invalid(declaration))?;
    let source = source.trim();
    let wide = match source.to_ascii_lowercase().as_str() {
        "inherit" => Some(WideKeyword::Inherit),
        "initial" => Some(WideKeyword::Initial),
        "unset" => Some(WideKeyword::Unset),
        _ => None,
    };
    let values = if let Some(wide) = wide {
        vec![Value::Wide(wide); properties.len()]
    } else if properties.len() == 4 {
        let values = source
            .split_ascii_whitespace()
            .map(|source| value(properties[0], source))
            .collect::<Option<Vec<_>>>();
        values
            .and_then(|values| expand_four(&values))
            .map(Vec::from)
            .ok_or_else(|| invalid(declaration))?
    } else {
        vec![value(properties[0], source).ok_or_else(|| invalid(declaration))?]
    };
    Ok(properties
        .into_iter()
        .zip(values)
        .map(|(property, value)| StyledDeclaration {
            property,
            value,
            important: declaration.is_important(),
            order,
        })
        .collect())
}

fn invalid(declaration: &Declaration) -> String {
    format!(
        "invalid or unsupported CSS value: {}: {}",
        declaration.property(),
        declaration.value()
    )
}

fn properties(name: &str) -> Option<Vec<Property>> {
    use Property as P;
    let sides = [Side::Top, Side::Right, Side::Bottom, Side::Left];
    let single = match name {
        "color" => P::Color,
        "background-color" => P::BackgroundColor,
        "display" => P::Display,
        "font-size" => P::FontSize,
        "font-family" => P::FontFamily,
        "font-weight" => P::FontWeight,
        "font-style" => P::FontStyle,
        "width" => P::Width,
        "margin" => return Some(sides.map(P::Margin).into()),
        "padding" => return Some(sides.map(P::Padding).into()),
        "border-width" => return Some(sides.map(P::BorderWidth).into()),
        "border-color" => return Some(sides.map(P::BorderColor).into()),
        _ => {
            for (suffix, side) in [
                ("top", Side::Top),
                ("right", Side::Right),
                ("bottom", Side::Bottom),
                ("left", Side::Left),
            ] {
                if name.strip_prefix("margin-") == Some(suffix) {
                    return Some(vec![P::Margin(side)]);
                }
                if name.strip_prefix("padding-") == Some(suffix) {
                    return Some(vec![P::Padding(side)]);
                }
                if name
                    .strip_prefix("border-")
                    .and_then(|name| name.strip_suffix("-width"))
                    == Some(suffix)
                {
                    return Some(vec![P::BorderWidth(side)]);
                }
                if name
                    .strip_prefix("border-")
                    .and_then(|name| name.strip_suffix("-color"))
                    == Some(suffix)
                {
                    return Some(vec![P::BorderColor(side)]);
                }
            }
            return None;
        }
    };
    Some(vec![single])
}

fn value(property: Property, source: &str) -> Option<Value> {
    use Property as P;
    let lower = source.to_ascii_lowercase();
    match property {
        P::Color | P::BackgroundColor | P::BorderColor(_) => parse_color(source).map(Value::Color),
        P::Display => Some(Value::Display(match lower.as_str() {
            "block" => Display::Block,
            "inline" => Display::Inline,
            "list-item" => Display::ListItem,
            "none" => Display::None,
            _ => return None,
        })),
        P::Width if lower == "auto" => Some(Value::Auto),
        P::FontSize | P::Width | P::Margin(_) | P::Padding(_) | P::BorderWidth(_) => {
            let length = Length::parse(source)?;
            if !matches!(property, P::Margin(_)) && !length.is_nonnegative() {
                return None;
            }
            if matches!(property, P::BorderWidth(_)) && matches!(length, Length::Percent(_)) {
                return None;
            }
            Some(Value::Length(length))
        }
        P::FontFamily if !source.is_empty() => Some(Value::Family(source.to_string())),
        P::FontFamily => None,
        P::FontWeight => match lower.as_str() {
            "bold" => Some(Value::Bold(true)),
            "normal" => Some(Value::Bold(false)),
            _ => {
                let weight: u16 = source.parse().ok()?;
                (100..=900)
                    .contains(&weight)
                    .then_some(Value::Bold(weight >= 600))
            }
        },
        P::FontStyle => match lower.as_str() {
            "normal" => Some(Value::Italic(false)),
            "italic" | "oblique" => Some(Value::Italic(true)),
            _ => None,
        },
    }
}
