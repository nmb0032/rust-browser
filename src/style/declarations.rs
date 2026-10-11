use std::sync::Arc;

use crate::css::{
    Declaration,
    tokenizer::TokenKind,
    values::{
        BackgroundImage, BorderStyle, Color, FamilyName, FontFamily, FontSizeKeyword, FontStyle,
        FontWeight, Length, LineHeight, MathExpression, Side,
        components::{self, Component},
        expand_four,
    },
};

use super::{
    AlignKeyword, Alignment, BaselineShift, BaselineSource, BorderCollapse, BoxSizing, CaptionSide,
    Clear, Content, ContentItem, Direction, Display, EmptyCells, FlexDirection, FlexWrap, Float,
    Overflow, Position, TableLayout, TextAlign, TextAlignLast, TextWrapMode, VerticalAlign,
    WhiteSpaceCollapse, ZIndex,
    compat::DeclarationContext,
    generated_properties::{self, Parser},
    variables::TokenValue,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Property {
    Content,
    Color,
    TextFillColor,
    BackgroundColor,
    BackgroundImage,
    Display,
    Float,
    Clear,
    TableLayout,
    BorderCollapse,
    BorderSpacing,
    CaptionSide,
    EmptyCells,
    Position,
    Inset(Side),
    ZIndex,
    OverflowX,
    OverflowY,
    Direction,
    TextAlignAll,
    TextAlignLast,
    WhiteSpaceCollapse,
    TextWrapMode,
    AlignmentBaseline,
    BaselineShift,
    BaselineSource,
    BoxSizing,
    FontSize,
    FontFamily,
    FontWeight,
    FontStyle,
    LineHeight,
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    GridTemplateColumns,
    GridTemplateRows,
    GridTemplateAreas,
    GridAutoColumns,
    GridAutoRows,
    GridAutoFlow,
    GridColumnStart,
    GridColumnEnd,
    GridRowStart,
    GridRowEnd,
    Order,
    JustifyContent,
    JustifyItems,
    JustifySelf,
    AlignItems,
    AlignSelf,
    AlignContent,
    RowGap,
    ColumnGap,
    ContainerType,
    ContainerName,
    Margin(Side),
    Padding(Side),
    BorderWidth(Side),
    BorderColor(Side),
    BorderStyle(Side),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum WideKeyword {
    Inherit,
    Initial,
    Unset,
    Revert,
    RevertLayer,
}

#[derive(Clone, Debug)]
pub(super) enum Value {
    Content(Content),
    ContainerType(crate::css::container::ContainerType),
    ContainerNames(Vec<String>),
    Wide(WideKeyword),
    Color(Color),
    BackgroundImage(BackgroundImage),
    BorderStyle(BorderStyle),
    Display(Display),
    Float(Float),
    Clear(Clear),
    TableLayout(TableLayout),
    BorderCollapse(BorderCollapse),
    BorderSpacing(Length, Length),
    CaptionSide(CaptionSide),
    EmptyCells(EmptyCells),
    Position(Position),
    ZIndex(ZIndex),
    Overflow(Overflow),
    Direction(Direction),
    TextAlign(TextAlign),
    TextAlignLast(TextAlignLast),
    WhiteSpaceCollapse(WhiteSpaceCollapse),
    TextWrapMode(TextWrapMode),
    VerticalAlign(VerticalAlign),
    BaselineShift(BaselineShift),
    BaselineSource(BaselineSource),
    BoxSizing(BoxSizing),
    Length(Length),
    NumberMath(Arc<MathExpression>),
    Auto,
    None,
    ContentBasis,
    Number(f32),
    Integer(i32),
    FlexDirection(FlexDirection),
    FlexWrap(FlexWrap),
    GridTracks(Vec<super::GridTrack>),
    GridAreas(Vec<Vec<Option<String>>>),
    GridLine(super::GridLine),
    GridAutoFlow(super::GridAutoFlow),
    Alignment(Alignment),
    Family(FontFamily),
    Weight(FontWeight),
    RelativeWeight(bool),
    FontStyle(FontStyle),
    FontSizeKeyword(FontSizeKeyword),
    LineHeight(LineHeight),
    Custom(Arc<TokenValue>),
    Pending(Arc<PendingDeclaration>),
}

#[derive(Clone, Debug)]
pub(super) struct PendingDeclaration {
    pub name: String,
    pub value: Arc<TokenValue>,
    pub context: DeclarationContext,
}

#[derive(Clone, Debug)]
pub(super) enum DeclarationProperty {
    Longhand(Property),
    Custom(String),
}

#[derive(Clone, Debug)]
pub(super) struct StyledDeclaration {
    pub property: DeclarationProperty,
    pub value: Value,
    pub important: bool,
    pub order: usize,
}

#[cfg(test)]
pub(super) fn parse(
    declaration: &Declaration,
    order: usize,
) -> Result<Vec<StyledDeclaration>, String> {
    parse_with_context(declaration, order, DeclarationContext::default())
}

pub(super) fn parse_with_context(
    declaration: &Declaration,
    order: usize,
    context: DeclarationContext,
) -> Result<Vec<StyledDeclaration>, String> {
    if declaration.property().starts_with("--") && declaration.property() != "--" {
        let tokens =
            TokenValue::new(declaration.value_tokens()).map_err(|_| invalid(declaration))?;
        let value = custom_wide(declaration.value_tokens())
            .map(Value::Wide)
            .unwrap_or_else(|| Value::Custom(Arc::new(tokens)));
        return Ok(vec![StyledDeclaration {
            property: DeclarationProperty::Custom(declaration.property().to_string()),
            value,
            important: declaration.is_important(),
            order,
        }]);
    }
    let name = declaration.property().to_ascii_lowercase();
    let properties = properties(&name)
        .ok_or_else(|| format!("unsupported CSS property: {}", declaration.property()))?;
    let values = if super::variables::contains_var(declaration.value_tokens()) {
        let value =
            TokenValue::new(declaration.value_tokens()).map_err(|_| invalid(declaration))?;
        vec![
            Value::Pending(Arc::new(PendingDeclaration {
                name: name.clone(),
                value: Arc::new(value),
                context,
            }));
            properties.len()
        ]
    } else {
        parse_tokens_with_context(&name, declaration.value_tokens(), context).ok_or_else(|| {
            let mut message = invalid(declaration);
            if name == "background" || name == "background-image" {
                message.push_str(" (supported background subset: one color and/or none; images, layers, and positioning are deferred)");
            }
            message
        })?
    };
    Ok(properties
        .into_iter()
        .zip(values)
        .map(|(property, value)| StyledDeclaration {
            property: DeclarationProperty::Longhand(property),
            value,
            important: declaration.is_important(),
            order,
        })
        .collect())
}

pub(super) fn parse_tokens(name: &str, tokens: &[TokenKind]) -> Option<Vec<Value>> {
    parse_tokens_with_context(name, tokens, DeclarationContext::default())
}

pub(super) fn parse_tokens_with_context(
    name: &str,
    tokens: &[TokenKind],
    context: DeclarationContext,
) -> Option<Vec<Value>> {
    let properties = properties(name)?;
    let parser = generated_properties::parser(name)?;
    let tokens = context.tokens(name, tokens);
    let tokens = components::from_tokens(&tokens)?;
    let wide = match tokens.as_slice() {
        [token] => token.ident().and_then(wide_keyword),
        _ => None,
    };
    if let Some(wide) = wide {
        return Some(vec![Value::Wide(wide); properties.len()]);
    }
    match parser {
        Parser::GridPlacement => super::grid::placement(&tokens, properties.len())
            .map(|values| values.into_iter().map(Value::GridLine).collect()),
        Parser::Place => {
            // Each alignment can have a two-token safety/baseline modifier.
            for split in (1..=tokens.len()).rev() {
                if let Some(first) = value(properties[0], &tokens[..split]) {
                    let second = if split == tokens.len() {
                        value(properties[1], &tokens[..split])
                    } else {
                        value(properties[1], &tokens[split..])
                    };
                    if let Some(second) = second {
                        return Some(vec![first, second]);
                    }
                }
            }
            None
        }
        Parser::Flex => flex(&tokens),
        Parser::FlexFlow => {
            if tokens.is_empty() || tokens.len() > 2 {
                return None;
            }
            let mut direction = None;
            let mut wrap = None;
            for token in &tokens {
                let one = std::slice::from_ref(token);
                if let Some(v) = value(Property::FlexDirection, one) {
                    if direction.replace(v).is_some() {
                        return None;
                    }
                } else {
                    if wrap.replace(value(Property::FlexWrap, one)?).is_some() {
                        return None;
                    }
                }
            }
            Some(vec![
                direction.unwrap_or(Value::FlexDirection(FlexDirection::Row)),
                wrap.unwrap_or(Value::FlexWrap(FlexWrap::Nowrap)),
            ])
        }
        Parser::Gap => {
            let (row, column) = match tokens.as_slice() {
                [a] => (a, a),
                [a, b] => (a, b),
                _ => return None,
            };
            Some(vec![
                value(Property::RowGap, std::slice::from_ref(row))?,
                value(Property::ColumnGap, std::slice::from_ref(column))?,
            ])
        }
        Parser::Overflow => {
            let (x, y) = match tokens.as_slice() {
                [x] => (x, x),
                [x, y] => (x, y),
                _ => return None,
            };
            Some(vec![
                value(Property::OverflowX, std::slice::from_ref(x))?,
                value(Property::OverflowY, std::slice::from_ref(y))?,
            ])
        }
        Parser::Longhand => value(properties[0], &tokens).map(|value| vec![value]),
        Parser::TextAlign => Some(vec![
            value(Property::TextAlignAll, &tokens)?,
            initial_value(Property::TextAlignLast),
        ]),
        Parser::WhiteSpace => white_space(&tokens),
        Parser::VerticalAlign => {
            let alignment = match tokens.as_slice() {
                [token] if token.is_ident("top") => Value::VerticalAlign(VerticalAlign::Top),
                [token] if token.is_ident("bottom") => Value::VerticalAlign(VerticalAlign::Bottom),
                _ => value(Property::AlignmentBaseline, &tokens)?,
            };
            Some(vec![
                alignment,
                initial_value(Property::BaselineShift),
                initial_value(Property::BaselineSource),
            ])
        }
        Parser::Container => {
            let parts = tokens
                .split(|t| matches!(t, Component::Token(TokenKind::Delim('/'))))
                .collect::<Vec<_>>();
            if parts.len() > 2 {
                return None;
            }
            let names = container_names(parts[0])?;
            let kind = if parts.len() == 2 {
                let [token] = parts[1] else { return None };
                Value::ContainerType(crate::css::container::ContainerType::parse(token.ident()?)?)
            } else {
                initial_value(Property::ContainerType)
            };
            Some(vec![Value::ContainerNames(names), kind])
        }
        Parser::Wide => None,
        Parser::Border => border(&tokens).map(|values| {
            properties
                .iter()
                .enumerate()
                .map(|(index, _)| values[index % 3].clone())
                .collect()
        }),
        Parser::Font => font(&tokens),
        Parser::Background => background(&tokens),
        Parser::Four => tokens
            .iter()
            .map(|token| value(properties[0], std::slice::from_ref(token)))
            .collect::<Option<Vec<_>>>()
            .and_then(|values| expand_four(&values))
            .map(Vec::from),
    }
}

/// Spec initial tokens use the same finite executable parser as declarations.
/// The generated UA-dependent family choice is an engine policy, not spec prose.
pub(super) fn initial_value(property: Property) -> Value {
    let source = generated_properties::initial_css(property);
    let tokens = crate::css::values::math::tokenize(source).expect("generated initial tokens");
    parse_tokens(property.name(), &tokens)
        .and_then(|mut values| (values.len() == 1).then(|| values.remove(0)))
        .expect("generated initial must match its executable property grammar")
}

pub(super) fn custom_wide(tokens: &[TokenKind]) -> Option<WideKeyword> {
    let mut tokens = tokens
        .iter()
        .filter(|token| !matches!(token, TokenKind::Whitespace));
    match (tokens.next(), tokens.next()) {
        (Some(TokenKind::Ident(name)), None) => wide_keyword(name),
        _ => None,
    }
}

fn wide_keyword(name: &str) -> Option<WideKeyword> {
    match name.to_ascii_lowercase().as_str() {
        "inherit" => Some(WideKeyword::Inherit),
        "initial" => Some(WideKeyword::Initial),
        "unset" => Some(WideKeyword::Unset),
        "revert" => Some(WideKeyword::Revert),
        "revert-layer" => Some(WideKeyword::RevertLayer),
        _ => None,
    }
}

fn reserved(name: &str) -> bool {
    wide_keyword(name).is_some()
        || name.eq_ignore_ascii_case("revert")
        || name.eq_ignore_ascii_case("revert-layer")
        || name.eq_ignore_ascii_case("default")
}

fn invalid(declaration: &Declaration) -> String {
    format!(
        "invalid or unsupported CSS value: {}: {}",
        declaration.property(),
        declaration.value()
    )
}

pub(super) fn properties(name: &str) -> Option<Vec<Property>> {
    generated_properties::properties(name)
}

fn white_space(tokens: &[Component]) -> Option<Vec<Value>> {
    use TextWrapMode::{Nowrap, Wrap};
    use WhiteSpaceCollapse::{BreakSpaces, Collapse, Preserve, PreserveBreaks};
    let modes = match tokens {
        [token] => match token.ident()?.to_ascii_lowercase().as_str() {
            "normal" => Some((Collapse, Wrap)),
            "nowrap" => Some((Collapse, Nowrap)),
            "pre" => Some((Preserve, Nowrap)),
            "pre-wrap" => Some((Preserve, Wrap)),
            "pre-line" => Some((PreserveBreaks, Wrap)),
            "break-spaces" => Some((BreakSpaces, Wrap)),
            _ => None,
        },
        _ => None,
    };
    let (collapse, wrap) = if let Some(modes) = modes {
        modes
    } else {
        if tokens.is_empty() || tokens.len() > 2 {
            return None;
        }
        let mut collapse = None;
        let mut wrap = None;
        for token in tokens {
            let one = std::slice::from_ref(token);
            if let Some(Value::WhiteSpaceCollapse(mode)) = value(Property::WhiteSpaceCollapse, one)
            {
                if collapse.replace(mode).is_some() {
                    return None;
                }
            } else if let Some(Value::TextWrapMode(mode)) = value(Property::TextWrapMode, one) {
                if wrap.replace(mode).is_some() {
                    return None;
                }
            } else {
                return None;
            }
        }
        (collapse.unwrap_or(Collapse), wrap.unwrap_or(Wrap))
    };
    Some(vec![
        Value::WhiteSpaceCollapse(collapse),
        Value::TextWrapMode(wrap),
    ])
}

fn border_properties(side: Side) -> [Property; 3] {
    [
        Property::BorderWidth(side),
        Property::BorderStyle(side),
        Property::BorderColor(side),
    ]
}

fn value(property: Property, tokens: &[Component]) -> Option<Value> {
    use Property as P;
    if generated_properties::simple(property) {
        return generated_properties::simple_value(property, tokens);
    }
    if matches!(
        property,
        P::JustifyContent
            | P::JustifyItems
            | P::JustifySelf
            | P::AlignItems
            | P::AlignSelf
            | P::AlignContent
    ) {
        return alignment(property, tokens).map(Value::Alignment);
    }
    match property {
        P::GridTemplateColumns | P::GridTemplateRows | P::GridAutoColumns | P::GridAutoRows => {
            return super::grid::tracks(
                tokens,
                matches!(property, P::GridAutoColumns | P::GridAutoRows),
            )
            .map(Value::GridTracks);
        }
        P::GridTemplateAreas => return super::grid::areas(tokens).map(Value::GridAreas),
        P::GridAutoFlow => return super::grid::flow(tokens).map(Value::GridAutoFlow),
        P::GridColumnStart | P::GridColumnEnd | P::GridRowStart | P::GridRowEnd => {
            return super::grid::line(tokens).map(Value::GridLine);
        }
        _ => {}
    }
    if property == P::Content {
        return content(tokens).map(Value::Content);
    }
    if property == P::FontFamily {
        return family(tokens).map(Value::Family);
    }
    if property == P::ContainerName {
        return container_names(tokens).map(Value::ContainerNames);
    }
    if property == P::BorderSpacing {
        let (x, y) = match tokens {
            [x] => (x, x),
            [x, y] => (x, y),
            _ => return None,
        };
        let (x, y) = (x.length()?, y.length()?);
        return (x.is_nonnegative()
            && y.is_nonnegative()
            && !x.has_percentage()
            && !y.has_percentage())
        .then_some(Value::BorderSpacing(x, y));
    }
    let [token] = tokens else {
        return None;
    };
    let lower = token.ident().unwrap_or("").to_ascii_lowercase();
    match property {
        P::Content
        | P::Color
        | P::TextFillColor
        | P::BackgroundColor
        | P::BorderColor(_)
        | P::BackgroundImage
        | P::BorderStyle(_)
        | P::Display
        | P::Float
        | P::Clear
        | P::TableLayout
        | P::BorderCollapse
        | P::CaptionSide
        | P::EmptyCells
        | P::Position
        | P::OverflowX
        | P::OverflowY
        | P::Direction
        | P::TextAlignAll
        | P::TextAlignLast
        | P::WhiteSpaceCollapse
        | P::TextWrapMode
        | P::AlignmentBaseline
        | P::BaselineSource
        | P::BoxSizing
        | P::FlexDirection
        | P::FlexWrap
        | P::ContainerType => unreachable!("generated simple dispatch or content"),
        P::JustifyContent
        | P::JustifyItems
        | P::JustifySelf
        | P::AlignItems
        | P::AlignSelf
        | P::AlignContent
        | P::GridTemplateColumns
        | P::GridTemplateRows
        | P::GridAutoColumns
        | P::GridAutoRows
        | P::GridTemplateAreas
        | P::GridAutoFlow
        | P::GridColumnStart
        | P::GridColumnEnd
        | P::GridRowStart
        | P::GridRowEnd => unreachable!(),
        P::ContainerName => unreachable!(),
        P::BorderSpacing => unreachable!(),
        P::BaselineShift => {
            (token.number()? == 0.0).then_some(Value::BaselineShift(BaselineShift::Zero))
        }

        P::ZIndex => {
            if lower == "auto" {
                Some(Value::ZIndex(ZIndex::Auto))
            } else {
                let Component::Token(TokenKind::Number(number)) = token else {
                    return None;
                };
                // CSS <integer> is lexical: 1.0 and 1e0 are not integers.
                let integer = number.representation.parse::<i32>().ok()?;
                Some(Value::ZIndex(ZIndex::Integer(integer)))
            }
        }
        P::Width
        | P::Height
        | P::MinWidth
        | P::MinHeight
        | P::FlexBasis
        | P::Margin(_)
        | P::Inset(_)
            if lower == "auto" =>
        {
            Some(Value::Auto)
        }
        P::MaxWidth | P::MaxHeight if lower == "none" => Some(Value::None),
        P::FlexBasis if lower == "content" => Some(Value::ContentBasis),
        P::RowGap | P::ColumnGap if lower == "normal" => Some(Value::None),
        P::FlexGrow | P::FlexShrink => {
            let n = token.number()?;
            (n.is_finite() && n >= 0.0).then_some(Value::Number(n))
        }
        P::Order => {
            let Component::Token(TokenKind::Number(n)) = token else {
                return None;
            };
            Some(Value::Integer(n.representation.parse().ok()?))
        }
        P::FontSize => font_size(token),
        P::Width
        | P::Height
        | P::MinWidth
        | P::MinHeight
        | P::MaxWidth
        | P::MaxHeight
        | P::FlexBasis
        | P::RowGap
        | P::ColumnGap
        | P::Margin(_)
        | P::Inset(_)
        | P::Padding(_)
        | P::BorderWidth(_) => {
            let length = if matches!(property, P::BorderWidth(_)) {
                match lower.as_str() {
                    "thin" => Length::Px(1.0),
                    "medium" => Length::Px(3.0),
                    "thick" => Length::Px(5.0),
                    _ => token.length()?,
                }
            } else {
                token.length()?
            };
            if !matches!(property, P::Margin(_) | P::Inset(_)) && !length.is_nonnegative() {
                return None;
            }
            if matches!(property, P::BorderWidth(_)) && length.has_percentage() {
                return None;
            }
            Some(Value::Length(length))
        }
        P::FontFamily => unreachable!(),
        P::FontWeight => match lower.as_str() {
            "bold" => Some(Value::Weight(FontWeight(700.0))),
            "normal" => Some(Value::Weight(FontWeight::default())),
            "bolder" => Some(Value::RelativeWeight(true)),
            "lighter" => Some(Value::RelativeWeight(false)),
            _ => {
                if let Some(weight) = token.number() {
                    (1.0..=1000.0)
                        .contains(&weight)
                        .then_some(Value::Weight(FontWeight(weight)))
                } else {
                    token.number_math().map(Value::NumberMath)
                }
            }
        },
        P::FontStyle => unreachable!("generated simple dispatch"),
        P::LineHeight => {
            let height = if lower == "normal" {
                LineHeight::Normal
            } else if let Some(number) = token.number() {
                if number < 0.0 {
                    return None;
                }
                LineHeight::Number(number)
            } else if let Some(expression) = token.number_math() {
                return Some(Value::NumberMath(expression));
            } else {
                let length = token.length()?;
                if !length.is_nonnegative() {
                    return None;
                }
                LineHeight::Length(length)
            };
            Some(Value::LineHeight(height))
        }
    }
}

fn alignment(property: Property, tokens: &[Component]) -> Option<Alignment> {
    use AlignKeyword as A;
    let (safe, token) = match tokens {
        [token] => (false, token),
        [modifier, token] if modifier.is_ident("safe") || modifier.is_ident("unsafe") => {
            (modifier.is_ident("safe"), token)
        }
        [first, baseline]
            if first.is_ident("first")
                && baseline.is_ident("baseline")
                && property != Property::JustifyContent =>
        {
            return Some(Alignment::new(A::Baseline));
        }
        _ => return None,
    };
    let keyword = match token.ident()?.to_ascii_lowercase().as_str() {
        "legacy" if property == Property::JustifyItems && tokens.len() == 1 => A::Normal,
        "auto" if matches!(property, Property::AlignSelf | Property::JustifySelf) => A::Auto,
        "normal" => A::Normal,
        "stretch" => A::Stretch,
        "flex-start" => A::FlexStart,
        "flex-end" => A::FlexEnd,
        "start" => A::Start,
        "end" => A::End,
        "self-start"
            if matches!(
                property,
                Property::AlignItems
                    | Property::AlignSelf
                    | Property::JustifyItems
                    | Property::JustifySelf
            ) =>
        {
            A::SelfStart
        }
        "self-end"
            if matches!(
                property,
                Property::AlignItems
                    | Property::AlignSelf
                    | Property::JustifyItems
                    | Property::JustifySelf
            ) =>
        {
            A::SelfEnd
        }
        "center" => A::Center,
        "baseline" if property != Property::JustifyContent => A::Baseline,
        "space-between"
            if matches!(property, Property::JustifyContent | Property::AlignContent) =>
        {
            A::SpaceBetween
        }
        "space-around" if matches!(property, Property::JustifyContent | Property::AlignContent) => {
            A::SpaceAround
        }
        "space-evenly" if matches!(property, Property::JustifyContent | Property::AlignContent) => {
            A::SpaceEvenly
        }
        _ => return None,
    };
    if tokens.len() == 2
        && !matches!(
            keyword,
            A::FlexStart | A::FlexEnd | A::Start | A::End | A::SelfStart | A::SelfEnd | A::Center
        )
    {
        return None;
    }
    Some(Alignment { keyword, safe })
}

fn flex(tokens: &[Component]) -> Option<Vec<Value>> {
    if let [token] = tokens {
        if token.is_ident("none") {
            return Some(vec![Value::Number(0.0), Value::Number(0.0), Value::Auto]);
        }
        if token.is_ident("auto") {
            return Some(vec![Value::Number(1.0), Value::Number(1.0), Value::Auto]);
        }
    }
    if tokens.is_empty() || tokens.len() > 3 {
        return None;
    }
    let mut numbers = Vec::new();
    let mut basis = None;
    let mut basis_between_factors = false;
    for token in tokens {
        if let Some(n) = token.number() {
            if n < 0.0 || !n.is_finite() {
                return None;
            }
            if numbers.len() < 2 {
                if basis_between_factors {
                    return None;
                }
                numbers.push(n);
            } else if n == 0.0 && basis.is_none() {
                basis = Some(Value::Length(Length::Px(0.0)));
            } else {
                return None;
            }
        } else {
            basis_between_factors = numbers.len() == 1;
            if basis
                .replace(value(Property::FlexBasis, std::slice::from_ref(token))?)
                .is_some()
            {
                return None;
            }
        }
    }
    Some(vec![
        Value::Number(numbers.first().copied().unwrap_or(1.0)),
        Value::Number(numbers.get(1).copied().unwrap_or(1.0)),
        basis.unwrap_or(Value::Length(Length::Percent(0.0))),
    ])
}

fn container_names(tokens: &[Component]) -> Option<Vec<String>> {
    let tokens = tokens
        .iter()
        .map(|t| match t {
            Component::Token(token) => Some(token.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    crate::css::container::names(&tokens)
}

fn content(tokens: &[Component]) -> Option<Content> {
    if let [token] = tokens {
        if token.is_ident("normal") {
            return Some(Content::Normal);
        }
        if token.is_ident("none") {
            return Some(Content::None);
        }
    }
    if tokens.is_empty() || tokens.len() > 256 {
        return None;
    }
    let items = tokens
        .iter()
        .map(|token| match token {
            Component::Token(TokenKind::String(text)) if text.len() <= 65_536 => {
                Some(ContentItem::Text(text.clone()))
            }
            Component::Function(name, args) if name.eq_ignore_ascii_case("attr") => {
                let significant: Vec<_> = args
                    .iter()
                    .filter(|t| **t != TokenKind::Whitespace)
                    .collect();
                match significant.as_slice() {
                    [TokenKind::Ident(name)] => Some(ContentItem::Attribute(name.clone())),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Content::Items(items))
}

fn font_size(token: &Component) -> Option<Value> {
    use FontSizeKeyword as K;
    let keyword = match token.ident().unwrap_or("").to_ascii_lowercase().as_str() {
        "xx-small" => Some(K::XxSmall),
        "x-small" => Some(K::XSmall),
        "small" => Some(K::Small),
        "medium" => Some(K::Medium),
        "large" => Some(K::Large),
        "x-large" => Some(K::XLarge),
        "xx-large" => Some(K::XxLarge),
        "xxx-large" => Some(K::XxxLarge),
        _ => None,
    };
    if let Some(keyword) = keyword {
        return Some(Value::FontSizeKeyword(keyword));
    }
    let length = match token.ident().unwrap_or("").to_ascii_lowercase().as_str() {
        "smaller" => Length::Em(1.0 / 1.2),
        "larger" => Length::Em(1.2),
        _ => token.length()?,
    };
    length.is_nonnegative().then_some(Value::Length(length))
}

fn border(tokens: &[Component]) -> Option<[Value; 3]> {
    if tokens.is_empty() {
        return None;
    }
    let mut values: [Option<Value>; 3] = [None, None, None];
    for token in tokens {
        let mut matched = false;
        for (index, property) in border_properties(Side::Top).into_iter().enumerate() {
            if let Some(value) = value(property, std::slice::from_ref(token)) {
                if values[index].is_some() {
                    return None;
                }
                values[index] = Some(value);
                matched = true;
                break;
            }
        }
        if !matched {
            return None;
        }
    }
    let [width, style, color] = values;
    Some([
        width.unwrap_or_else(|| initial_value(Property::BorderWidth(Side::Top))),
        style.unwrap_or_else(|| initial_value(Property::BorderStyle(Side::Top))),
        color.unwrap_or_else(|| initial_value(Property::BorderColor(Side::Top))),
    ])
}

fn family(tokens: &[Component]) -> Option<FontFamily> {
    let mut names = Vec::new();
    for tokens in tokens.split(|token| matches!(token, Component::Token(TokenKind::Comma))) {
        let name = match tokens {
            [Component::Token(TokenKind::String(name))] => FamilyName::Named(name.clone()),
            _ => {
                if tokens.is_empty() {
                    return None;
                }
                let words = tokens
                    .iter()
                    .map(|token| token.ident())
                    .collect::<Option<Vec<_>>>()?;
                if words.iter().any(|word| reserved(word)) {
                    return None;
                }
                let generic = |word: &str| {
                    matches!(
                        word.to_ascii_lowercase().as_str(),
                        "serif"
                            | "sans-serif"
                            | "monospace"
                            | "cursive"
                            | "fantasy"
                            | "system-ui"
                            | "ui-serif"
                            | "ui-sans-serif"
                            | "ui-monospace"
                            | "ui-rounded"
                            | "emoji"
                            | "math"
                            | "fangsong"
                    )
                };
                if words.iter().any(|word| generic(word)) {
                    if words.len() != 1 {
                        return None;
                    }
                    FamilyName::Generic(words[0].to_ascii_lowercase())
                } else {
                    FamilyName::Named(words.join(" "))
                }
            }
        };
        names.push(name);
    }
    Some(FontFamily(names))
}

fn font(tokens: &[Component]) -> Option<Vec<Value>> {
    let mut style = None;
    let mut weight = None;
    let mut normals = 0;
    let mut index = 0;
    let size = loop {
        let token = tokens.get(index)?;
        if let Some(size) = font_size(token) {
            break size;
        }
        if token.is_ident("normal") {
            normals += 1;
        } else if let Some(value) = value(Property::FontStyle, std::slice::from_ref(token)) {
            if style.replace(value).is_some() {
                return None;
            }
        } else {
            let value = value(Property::FontWeight, std::slice::from_ref(token))?;
            if weight.replace(value).is_some() {
                return None;
            }
        }
        index += 1;
    };
    if normals + usize::from(style.is_some()) + usize::from(weight.is_some()) > 2 {
        return None;
    }
    index += 1;
    let mut line_height = initial_value(Property::LineHeight);
    if matches!(
        tokens.get(index),
        Some(Component::Token(TokenKind::Delim('/')))
    ) {
        index += 1;
        line_height = value(
            Property::LineHeight,
            std::slice::from_ref(tokens.get(index)?),
        )?;
        index += 1;
    }
    Some(vec![
        style.unwrap_or_else(|| initial_value(Property::FontStyle)),
        weight.unwrap_or_else(|| initial_value(Property::FontWeight)),
        size,
        line_height,
        Value::Family(family(&tokens[index..])?),
    ])
}

fn background(tokens: &[Component]) -> Option<Vec<Value>> {
    if tokens.is_empty() {
        return None;
    }
    let mut color = None;
    let mut image = false;
    for token in tokens {
        if token.is_ident("none") {
            if image {
                return None;
            }
            image = true;
        } else {
            if color.is_some() {
                return None;
            }
            color = Some(token.color()?);
        }
    }
    Some(vec![
        color
            .map(Value::Color)
            .unwrap_or_else(|| initial_value(Property::BackgroundColor)),
        initial_value(Property::BackgroundImage),
    ])
}
