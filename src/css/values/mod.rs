mod color;
pub(crate) mod components;
mod keywords;
pub(crate) mod math;

pub use color::{Color, ColorSpace, HueMethod, ResolvedColor, parse_color};
pub use keywords::{
    BackgroundImage, BorderStyle, FamilyName, FontFamily, FontSizeKeyword, FontStyle, FontWeight,
    LineHeight,
};
pub use math::{AbsoluteUnit, Angle, Dimension, MathExpression, RelativeUnit, Resolution, Time};

#[derive(Clone, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Em(f32),
    Percent(f32),
    Absolute(f32, AbsoluteUnit),
    Relative(f32, RelativeUnit),
    Math(std::sync::Arc<MathExpression>),
}

impl Default for Length {
    fn default() -> Self {
        Self::Px(0.0)
    }
}

/// A box value whose `auto` keyword must survive until layout.
#[derive(Clone, Debug, PartialEq)]
pub enum LengthOrAuto {
    Length(Length),
    Auto,
}

impl Default for LengthOrAuto {
    fn default() -> Self {
        Self::Length(Length::default())
    }
}

impl From<Length> for LengthOrAuto {
    fn from(length: Length) -> Self {
        Self::Length(length)
    }
}

impl Length {
    pub fn parse(source: &str) -> Option<Self> {
        Self::from_tokens(&math::tokenize(source)?)
    }

    pub(crate) fn from_tokens(tokens: &[crate::css::tokenizer::TokenKind]) -> Option<Self> {
        use crate::css::tokenizer::TokenKind;
        if tokens.len() > math::MAX_MATH_TOKENS {
            return None;
        }
        let compact = tokens
            .iter()
            .filter(|token| **token != TokenKind::Whitespace)
            .collect::<Vec<_>>();
        if let [token] = compact.as_slice() {
            let number = match token {
                TokenKind::Number(number)
                | TokenKind::Percentage(number)
                | TokenKind::Dimension { number, .. } => number.value().ok()? as f32,
                _ => return None,
            };
            if !number.is_finite() {
                return None;
            }
            return match token {
                TokenKind::Number(_) if number == 0.0 => Some(Self::Px(0.0)),
                TokenKind::Percentage(_) => Some(Self::Percent(number)),
                TokenKind::Dimension { unit, .. } => {
                    let (dimension, unit_kind) = math::unit(unit)?;
                    if dimension != Dimension::LENGTH {
                        return None;
                    }
                    match unit_kind {
                        math::Unit::Absolute(scale) => {
                            let value = number * scale as f32;
                            if !value.is_finite() {
                                return None;
                            }
                            Some(if let Some(unit) = AbsoluteUnit::parse(unit) {
                                Self::Absolute(number, unit)
                            } else {
                                Self::Px(number)
                            })
                        }
                        math::Unit::Relative(RelativeUnit::Em) => Some(Self::Em(number)),
                        math::Unit::Relative(unit) => Some(Self::Relative(number, unit)),
                        _ => None,
                    }
                }
                _ => None,
            };
        }
        if !matches!(compact.first(), Some(TokenKind::Function(_))) {
            return None;
        }
        let expression = MathExpression::from_tokens(tokens, Some(Dimension::LENGTH))?;
        (expression.dimension == Dimension::LENGTH).then_some(Self::Math(expression))
    }

    pub fn resolve(&self, font_size: f32, reference: f32) -> f32 {
        self.resolve_with_diagnostic(font_size, reference, |message| eprintln!("CSS: {message}"))
    }

    pub(crate) fn resolve_with_diagnostic(
        &self,
        font_size: f32,
        reference: f32,
        report: impl FnOnce(String),
    ) -> f32 {
        match self.used(
            &LengthContext {
                font_size,
                ..LengthContext::default()
            },
            reference,
        ) {
            Some(value) => value.0,
            None => {
                report(format!(
                    "invalid at used-value time: non-finite CSS length result; \
                     value={self:?}, font-size={font_size}px, reference={reference}px; \
                     compatibility fallback is 0px"
                ));
                0.0
            }
        }
    }

    pub fn used(&self, context: &LengthContext, reference: f32) -> Option<UsedLength> {
        let value = match self {
            Self::Px(value) => *value,
            Self::Em(value) => value * context.font_size,
            Self::Percent(value) => value * reference / 100.0,
            Self::Absolute(value, unit) => *value * unit.scale() as f32,
            Self::Relative(value, unit) => *value * unit.scale(context) as f32,
            Self::Math(expression) => expression.evaluate(context, reference)?,
        };
        value.is_finite().then_some(UsedLength(value))
    }

    pub fn computed(&self, context: &LengthContext) -> Option<ComputedLength> {
        match self {
            Self::Percent(_) => Some(ComputedLength(self.clone())),
            Self::Math(expression) if expression.has_percentage() => {
                Some(ComputedLength(Self::Math(expression.computed(context)?)))
            }
            _ => self
                .used(context, 0.0)
                .map(|value| ComputedLength(Self::Px(value.0))),
        }
    }

    pub fn has_percentage(&self) -> bool {
        matches!(self, Self::Percent(_))
            || matches!(self, Self::Math(expression) if expression.has_percentage())
    }

    pub(crate) fn is_nonnegative(&self) -> bool {
        match self {
            Self::Px(value)
            | Self::Em(value)
            | Self::Percent(value)
            | Self::Relative(value, _)
            | Self::Absolute(value, _) => *value >= 0.0,
            // Range restrictions on math results apply after evaluation.
            Self::Math(_) => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LengthContext {
    pub font_size: f32,
    pub root_font_size: f32,
    pub zero_advance: f32,
    pub x_height: f32,
    pub line_height: f32,
    pub root_line_height: f32,
    pub small_viewport: (f32, f32),
    pub large_viewport: (f32, f32),
    pub dynamic_viewport: (f32, f32),
    /// Per-axis nearest eligible ancestor content sizes; absent axes use the
    /// small viewport, as required for container-relative units.
    pub container_width: Option<f32>,
    pub container_height: Option<f32>,
}

impl Default for LengthContext {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            root_font_size: 16.0,
            zero_advance: 8.0,
            x_height: 8.0,
            line_height: 20.0,
            root_line_height: 20.0,
            small_viewport: (800.0, 600.0),
            large_viewport: (800.0, 600.0),
            dynamic_viewport: (800.0, 600.0),
            container_width: None,
            container_height: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpecifiedLength(pub Length);
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedLength(pub Length);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsedLength(pub f32);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActualLength {
    pub device_pixels: f32,
}

impl SpecifiedLength {
    pub fn parse(source: &str) -> Option<Self> {
        Length::parse(source).map(Self)
    }
    pub fn compute(&self, context: &LengthContext) -> Option<ComputedLength> {
        self.0.computed(context)
    }
}
impl ComputedLength {
    pub fn use_value(&self, reference: f32) -> Option<UsedLength> {
        self.0.used(&LengthContext::default(), reference)
    }
}
impl UsedLength {
    /// Quantize the two edges, as rectangle rasterization does, not just width.
    pub fn actual(self, css_origin: f32, device_scale: f32) -> Option<ActualLength> {
        if !css_origin.is_finite() || !device_scale.is_finite() || device_scale <= 0.0 {
            return None;
        }
        let pixels =
            ((css_origin + self.0) * device_scale).ceil() - (css_origin * device_scale).floor();
        pixels.is_finite().then_some(ActualLength {
            device_pixels: pixels,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl<T> Edges<T> {
    pub(crate) fn get(&self, side: Side) -> &T {
        match side {
            Side::Top => &self.top,
            Side::Right => &self.right,
            Side::Bottom => &self.bottom,
            Side::Left => &self.left,
        }
    }

    pub(crate) fn get_mut(&mut self, side: Side) -> &mut T {
        match side {
            Side::Top => &mut self.top,
            Side::Right => &mut self.right,
            Side::Bottom => &mut self.bottom,
            Side::Left => &mut self.left,
        }
    }
}

impl<T: Clone> Edges<T> {
    pub fn all(value: T) -> Self {
        Self {
            top: value.clone(),
            right: value.clone(),
            bottom: value.clone(),
            left: value,
        }
    }

    pub fn map<U>(&self, mut map: impl FnMut(T) -> U) -> Edges<U> {
        Edges {
            top: map(self.top.clone()),
            right: map(self.right.clone()),
            bottom: map(self.bottom.clone()),
            left: map(self.left.clone()),
        }
    }
}

pub(crate) fn expand_four<T: Clone>(values: &[T]) -> Option<[T; 4]> {
    let [first, rest @ ..] = values else {
        return None;
    };
    match rest {
        [] => Some([first.clone(), first.clone(), first.clone(), first.clone()]),
        [second] => Some([first.clone(), second.clone(), first.clone(), second.clone()]),
        [second, third] => Some([first.clone(), second.clone(), third.clone(), second.clone()]),
        [second, third, fourth] => {
            Some([first.clone(), second.clone(), third.clone(), fourth.clone()])
        }
        _ => None,
    }
}

#[cfg(test)]
mod math_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_resolves_supported_units() {
        assert_eq!(Length::parse("2EM").unwrap().resolve(16.0, 200.0), 32.0);
        assert_eq!(Length::parse("25%").unwrap().resolve(16.0, 200.0), 50.0);
        assert_eq!(Length::parse("-3px"), Some(Length::Px(-3.0)));
        assert_eq!(Length::parse("0"), Some(Length::Px(0.0)));
        for source in ["3", "NaNpx", "infem", "1e99px", "2 px", "auto"] {
            assert!(Length::parse(source).is_none(), "{source}");
        }
    }

    #[test]
    fn expands_shorthand_edges_in_clockwise_order() {
        assert_eq!(expand_four(&[1]), Some([1, 1, 1, 1]));
        assert_eq!(expand_four(&[1, 2]), Some([1, 2, 1, 2]));
        assert_eq!(expand_four(&[1, 2, 3]), Some([1, 2, 3, 2]));
        assert_eq!(expand_four(&[1, 2, 3, 4]), Some([1, 2, 3, 4]));
        assert_eq!(expand_four::<i32>(&[]), None);
        assert_eq!(expand_four(&[1, 2, 3, 4, 5]), None);
    }
}
