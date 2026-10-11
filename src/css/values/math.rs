//! Bounded, token-based CSS math. Percentage hints are supplied by the caller's
//! grammar, so length percentages retain their containing-block dependency.
use std::sync::Arc;

use crate::css::tokenizer::{TokenKind, Tokenizer};

use super::LengthContext;

pub const MAX_MATH_TOKENS: usize = 4096;
pub const MAX_MATH_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dimension(pub [i8; 4]);

impl Dimension {
    pub const NUMBER: Self = Self([0; 4]);
    pub const LENGTH: Self = Self([1, 0, 0, 0]);
    pub const ANGLE: Self = Self([0, 1, 0, 0]);
    pub const TIME: Self = Self([0, 0, 1, 0]);
    pub const RESOLUTION: Self = Self([0, 0, 0, 1]);

    fn product(self, other: Self, divide: bool) -> Option<Self> {
        let mut result = [0; 4];
        for (i, value) in result.iter_mut().enumerate() {
            *value = if divide {
                self.0[i].checked_sub(other.0[i])?
            } else {
                self.0[i].checked_add(other.0[i])?
            };
        }
        Some(Self(result))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MathExpression {
    pub dimension: Dimension,
    node: Node,
    height: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Value(f64, Unit),
    Binary(char, Arc<MathExpression>, Arc<MathExpression>),
    Function(Function, Vec<Arc<MathExpression>>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Unit {
    Number,
    Percent,
    Absolute(f64),
    Relative(RelativeUnit),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbsoluteUnit {
    In,
    Cm,
    Mm,
    Q,
    Pt,
    Pc,
}

impl AbsoluteUnit {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "in" => Self::In,
            "cm" => Self::Cm,
            "mm" => Self::Mm,
            "q" => Self::Q,
            "pt" => Self::Pt,
            "pc" => Self::Pc,
            _ => return None,
        })
    }

    pub(crate) fn scale(self) -> f64 {
        match self {
            Self::In => 96.0,
            Self::Cm => 96.0 / 2.54,
            Self::Mm => 96.0 / 25.4,
            Self::Q => 96.0 / 101.6,
            Self::Pt => 96.0 / 72.0,
            Self::Pc => 16.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeUnit {
    Em,
    Rem,
    Ch,
    Ex,
    Cap,
    Ic,
    Rch,
    Rex,
    Rcap,
    Ric,
    Lh,
    Rlh,
    Vw,
    Vh,
    Vi,
    Vb,
    Vmin,
    Vmax,
    Svw,
    Svh,
    Svi,
    Svb,
    Svmin,
    Svmax,
    Lvw,
    Lvh,
    Lvi,
    Lvb,
    Lvmin,
    Lvmax,
    Dvw,
    Dvh,
    Dvi,
    Dvb,
    Dvmin,
    Dvmax,
    Cqw,
    Cqh,
    Cqi,
    Cqb,
    Cqmin,
    Cqmax,
}

impl RelativeUnit {
    pub(crate) fn scale(self, context: &LengthContext) -> f64 {
        let (w, h) = match self {
            Self::Svw | Self::Svh | Self::Svi | Self::Svb | Self::Svmin | Self::Svmax => {
                context.small_viewport
            }
            Self::Dvw | Self::Dvh | Self::Dvi | Self::Dvb | Self::Dvmin | Self::Dvmax => {
                context.dynamic_viewport
            }
            _ => context.large_viewport,
        };
        f64::from(match self {
            Self::Em => context.font_size,
            Self::Rem => context.root_font_size,
            Self::Ch => context.zero_advance,
            Self::Ex => context.x_height,
            Self::Cap => context.cap_height,
            Self::Ic => context.ideographic_advance,
            Self::Rch => context.root_zero_advance,
            Self::Rex => context.root_x_height,
            Self::Rcap => context.root_cap_height,
            Self::Ric => context.root_ideographic_advance,
            Self::Lh => context.line_height,
            Self::Rlh => context.root_line_height,
            Self::Vw | Self::Svw | Self::Lvw | Self::Dvw => w / 100.0,
            Self::Vh | Self::Svh | Self::Lvh | Self::Dvh => h / 100.0,
            Self::Vi | Self::Svi | Self::Lvi | Self::Dvi => {
                (if context.viewport_vertical { h } else { w }) / 100.0
            }
            Self::Vb | Self::Svb | Self::Lvb | Self::Dvb => {
                (if context.viewport_vertical { w } else { h }) / 100.0
            }
            Self::Vmin | Self::Svmin | Self::Lvmin | Self::Dvmin => w.min(h) / 100.0,
            Self::Vmax | Self::Svmax | Self::Lvmax | Self::Dvmax => w.max(h) / 100.0,
            Self::Cqw => context.container_width.unwrap_or(context.small_viewport.0) / 100.0,
            Self::Cqh => context.container_height.unwrap_or(context.small_viewport.1) / 100.0,
            Self::Cqi | Self::Cqb => {
                (if self == Self::Cqi {
                    context
                        .container_inline
                        .unwrap_or(if context.viewport_vertical {
                            context.small_viewport.1
                        } else {
                            context.small_viewport.0
                        })
                } else {
                    context
                        .container_block
                        .unwrap_or(if context.viewport_vertical {
                            context.small_viewport.0
                        } else {
                            context.small_viewport.1
                        })
                }) / 100.0
            }
            Self::Cqmin | Self::Cqmax => {
                let w = context
                    .container_inline
                    .unwrap_or(if context.viewport_vertical {
                        context.small_viewport.1
                    } else {
                        context.small_viewport.0
                    });
                let h = context
                    .container_block
                    .unwrap_or(if context.viewport_vertical {
                        context.small_viewport.0
                    } else {
                        context.small_viewport.1
                    });
                if self == Self::Cqmin {
                    w.min(h) / 100.0
                } else {
                    w.max(h) / 100.0
                }
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Function {
    Min,
    Max,
    Clamp,
    Hypot,
    Abs,
    Sign,
    Mod,
    Rem,
    Round(Rounding),
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Pow,
    Sqrt,
    Log,
    Exp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rounding {
    Nearest,
    Up,
    Down,
    ToZero,
}

pub(crate) fn unit(name: &str) -> Option<(Dimension, Unit)> {
    use RelativeUnit as R;
    if let Some(unit) = AbsoluteUnit::parse(name) {
        return Some((Dimension::LENGTH, Unit::Absolute(unit.scale())));
    }
    let relative = match name.to_ascii_lowercase().as_str() {
        "px" => return Some((Dimension::LENGTH, Unit::Absolute(1.0))),
        "deg" => return Some((Dimension::ANGLE, Unit::Absolute(1.0))),
        "grad" => return Some((Dimension::ANGLE, Unit::Absolute(0.9))),
        "rad" => {
            return Some((
                Dimension::ANGLE,
                Unit::Absolute(180.0 / std::f64::consts::PI),
            ));
        }
        "turn" => return Some((Dimension::ANGLE, Unit::Absolute(360.0))),
        "s" => return Some((Dimension::TIME, Unit::Absolute(1.0))),
        "ms" => return Some((Dimension::TIME, Unit::Absolute(0.001))),
        "dppx" | "x" => return Some((Dimension::RESOLUTION, Unit::Absolute(1.0))),
        "dpi" => return Some((Dimension::RESOLUTION, Unit::Absolute(1.0 / 96.0))),
        "dpcm" => return Some((Dimension::RESOLUTION, Unit::Absolute(2.54 / 96.0))),
        "em" => R::Em,
        "rem" => R::Rem,
        "ch" => R::Ch,
        "ex" => R::Ex,
        "cap" => R::Cap,
        "ic" => R::Ic,
        "rch" => R::Rch,
        "rex" => R::Rex,
        "rcap" => R::Rcap,
        "ric" => R::Ric,
        "lh" => R::Lh,
        "rlh" => R::Rlh,
        "vw" => R::Vw,
        "vh" => R::Vh,
        "vi" => R::Vi,
        "vb" => R::Vb,
        "vmin" => R::Vmin,
        "vmax" => R::Vmax,
        "svw" => R::Svw,
        "svh" => R::Svh,
        "svi" => R::Svi,
        "svb" => R::Svb,
        "svmin" => R::Svmin,
        "svmax" => R::Svmax,
        "lvw" => R::Lvw,
        "lvh" => R::Lvh,
        "lvi" => R::Lvi,
        "lvb" => R::Lvb,
        "lvmin" => R::Lvmin,
        "lvmax" => R::Lvmax,
        "dvw" => R::Dvw,
        "dvh" => R::Dvh,
        "dvi" => R::Dvi,
        "dvb" => R::Dvb,
        "dvmin" => R::Dvmin,
        "dvmax" => R::Dvmax,
        "cqw" => R::Cqw,
        "cqh" => R::Cqh,
        "cqi" => R::Cqi,
        "cqb" => R::Cqb,
        "cqmin" => R::Cqmin,
        "cqmax" => R::Cqmax,
        _ => return None,
    };
    Some((Dimension::LENGTH, Unit::Relative(relative)))
}

pub(crate) fn tokenize(source: &str) -> Option<Vec<TokenKind>> {
    if source.len() > 65_536 {
        return None;
    }
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let mut tokens = Vec::new();
    while let Some(token) = tokenizer.next_token() {
        if token.kind == TokenKind::Eof {
            break;
        }
        if tokens.len() == MAX_MATH_TOKENS {
            return None;
        }
        tokens.push(token.kind);
    }
    tokenizer.errors().is_empty().then_some(tokens)
}

impl MathExpression {
    pub fn parse(source: &str, percentage_hint: Option<Dimension>) -> Option<Arc<Self>> {
        Self::from_tokens(&tokenize(source)?, percentage_hint)
    }

    pub(crate) fn from_tokens(tokens: &[TokenKind], hint: Option<Dimension>) -> Option<Arc<Self>> {
        if tokens.len() > MAX_MATH_TOKENS {
            return None;
        }
        let mut parser = Parser {
            tokens,
            index: 0,
            hint,
        };
        parser.space();
        if !matches!(
            parser.tokens.get(parser.index),
            Some(
                TokenKind::Number(_)
                    | TokenKind::Percentage(_)
                    | TokenKind::Dimension { .. }
                    | TokenKind::Function(_)
            )
        ) {
            return None;
        }
        // Outside a math function, only a single numeric token is allowed.
        let expression = parser.atom(0)?;
        parser.space();
        (parser.index == tokens.len()).then_some(expression)
    }

    pub fn has_percentage(&self) -> bool {
        match &self.node {
            Node::Value(_, unit) => *unit == Unit::Percent,
            Node::Binary(_, a, b) => a.has_percentage() || b.has_percentage(),
            Node::Function(_, args) => args.iter().any(|arg| arg.has_percentage()),
        }
    }

    pub(crate) fn computed(&self, context: &LengthContext) -> Option<Arc<Self>> {
        let node = match &self.node {
            Node::Value(value, Unit::Relative(unit)) => {
                Node::Value(value * unit.scale(context), Unit::Number)
            }
            Node::Value(value, Unit::Absolute(scale)) => Node::Value(value * scale, Unit::Number),
            Node::Value(value, unit) => Node::Value(*value, *unit),
            Node::Binary(op, a, b) => Node::Binary(*op, a.computed(context)?, b.computed(context)?),
            Node::Function(function, args) => Node::Function(
                *function,
                args.iter()
                    .map(|arg| arg.computed(context))
                    .collect::<Option<Vec<_>>>()?,
            ),
        };
        let mut result = Self {
            dimension: self.dimension,
            node,
            height: self.height,
        };
        if !result.has_percentage() {
            result.node = Node::Value(result.evaluate64(context, 0.0)?, Unit::Number);
            result.height = 1;
        }
        Some(Arc::new(result))
    }

    pub fn evaluate(&self, context: &LengthContext, reference: f32) -> Option<f32> {
        let value = self.evaluate64(context, f64::from(reference))?;
        let value = value as f32;
        value.is_finite().then_some(value)
    }

    pub(crate) fn evaluate64(&self, context: &LengthContext, reference: f64) -> Option<f64> {
        let value = match &self.node {
            Node::Value(value, unit) => {
                value
                    * match unit {
                        Unit::Number => 1.0,
                        Unit::Percent => reference / 100.0,
                        Unit::Absolute(scale) => *scale,
                        Unit::Relative(unit) => unit.scale(context),
                    }
            }
            Node::Binary(op, a, b) => {
                let a = a.evaluate64(context, reference)?;
                let b = b.evaluate64(context, reference)?;
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => unreachable!(),
                }
            }
            Node::Function(function, args) => {
                let values = args
                    .iter()
                    .map(|arg| arg.evaluate64(context, reference))
                    .collect::<Option<Vec<_>>>()?;
                let a = values[0];
                let b = values.get(1).copied().unwrap_or(1.0);
                match function {
                    Function::Min => values.into_iter().fold(f64::INFINITY, f64::min),
                    Function::Max => values.into_iter().fold(f64::NEG_INFINITY, f64::max),
                    Function::Clamp => a.max(b.min(values[2])),
                    Function::Hypot => values.into_iter().fold(0.0, f64::hypot),
                    Function::Abs => a.abs(),
                    Function::Sign => {
                        if a == 0.0 {
                            a
                        } else {
                            a.signum()
                        }
                    }
                    Function::Mod => a - b * (a / b).floor(),
                    Function::Rem => a % b,
                    Function::Round(rounding) => {
                        if b <= 0.0 {
                            return None;
                        }
                        let quotient = a / b;
                        let rounded = match rounding {
                            Rounding::Nearest => (quotient + 0.5).floor(),
                            Rounding::Up => quotient.ceil(),
                            Rounding::Down => quotient.floor(),
                            Rounding::ToZero => quotient.trunc(),
                        };
                        rounded * b
                    }
                    Function::Sin | Function::Cos | Function::Tan => {
                        let angle = if args[0].dimension == Dimension::ANGLE {
                            a.to_radians()
                        } else {
                            a
                        };
                        match function {
                            Function::Sin => angle.sin(),
                            Function::Cos => angle.cos(),
                            _ => angle.tan(),
                        }
                    }
                    Function::Asin => a.asin().to_degrees(),
                    Function::Acos => a.acos().to_degrees(),
                    Function::Atan => a.atan().to_degrees(),
                    Function::Atan2 => a.atan2(b).to_degrees(),
                    Function::Pow => a.powf(b),
                    Function::Sqrt => a.sqrt(),
                    Function::Log => {
                        if values.len() == 1 {
                            a.ln()
                        } else {
                            a.log(b)
                        }
                    }
                    Function::Exp => a.exp(),
                }
            }
        };
        value.is_finite().then_some(value)
    }
}

struct Parser<'a> {
    tokens: &'a [TokenKind],
    index: usize,
    hint: Option<Dimension>,
}

impl Parser<'_> {
    fn space(&mut self) -> bool {
        let start = self.index;
        while self.tokens.get(self.index) == Some(&TokenKind::Whitespace) {
            self.index += 1;
        }
        self.index != start
    }

    fn atom(&mut self, depth: usize) -> Option<Arc<MathExpression>> {
        if depth >= MAX_MATH_DEPTH {
            return None;
        }
        self.space();
        let token = self.tokens.get(self.index)?.clone();
        self.index += 1;
        let (dimension, value, unit) = match token {
            TokenKind::Number(number) => (Dimension::NUMBER, number.value().ok()?, Unit::Number),
            TokenKind::Percentage(number) => (self.hint?, number.value().ok()?, Unit::Percent),
            TokenKind::Dimension { number, unit: name } => {
                let (dimension, unit) = unit(&name)?;
                (dimension, number.value().ok()?, unit)
            }
            TokenKind::Ident(name) if name.eq_ignore_ascii_case("pi") => {
                (Dimension::NUMBER, std::f64::consts::PI, Unit::Number)
            }
            TokenKind::Ident(name) if name.eq_ignore_ascii_case("e") => {
                (Dimension::NUMBER, std::f64::consts::E, Unit::Number)
            }
            TokenKind::OpenParen => {
                let result = self.sum(depth + 1)?;
                self.close()?;
                return Some(result);
            }
            TokenKind::Function(name) => {
                return self.function(&name.to_ascii_lowercase(), depth + 1);
            }
            _ => return None,
        };
        if !value.is_finite() || !(value as f32).is_finite() {
            return None;
        }
        Some(Arc::new(MathExpression {
            dimension,
            node: Node::Value(value, unit),
            height: 1,
        }))
    }

    fn close(&mut self) -> Option<()> {
        self.space();
        if self.tokens.get(self.index)? != &TokenKind::CloseParen {
            return None;
        }
        self.index += 1;
        Some(())
    }

    fn sum(&mut self, depth: usize) -> Option<Arc<MathExpression>> {
        let mut result = self.product(depth)?;
        loop {
            let before = self.index;
            let spaced = self.space();
            let Some(TokenKind::Delim(op @ ('+' | '-'))) = self.tokens.get(self.index) else {
                break;
            };
            let op = *op;
            // CSS requires whitespace on both sides of a binary + or -.
            if !spaced || self.tokens.get(self.index + 1) != Some(&TokenKind::Whitespace) {
                return None;
            }
            self.index += 1;
            let rhs = self.product(depth)?;
            if result.dimension != rhs.dimension {
                return None;
            }
            let height = result.height.max(rhs.height) + 1;
            if height > MAX_MATH_DEPTH {
                return None;
            }
            result = Arc::new(MathExpression {
                dimension: result.dimension,
                node: Node::Binary(op, result, rhs),
                height,
            });
            if self.index <= before {
                return None;
            }
        }
        Some(result)
    }

    fn product(&mut self, depth: usize) -> Option<Arc<MathExpression>> {
        let mut result = self.atom(depth)?;
        loop {
            let before = self.index;
            self.space();
            let Some(TokenKind::Delim(op @ ('*' | '/'))) = self.tokens.get(self.index) else {
                // Let sum observe the whitespace preceding its operator.
                self.index = before;
                break;
            };
            let op = *op;
            self.index += 1;
            let rhs = self.atom(depth)?;
            let dimension = result.dimension.product(rhs.dimension, op == '/')?;
            let height = result.height.max(rhs.height) + 1;
            if height > MAX_MATH_DEPTH {
                return None;
            }
            result = Arc::new(MathExpression {
                dimension,
                node: Node::Binary(op, result, rhs),
                height,
            });
        }
        Some(result)
    }

    fn function(&mut self, name: &str, depth: usize) -> Option<Arc<MathExpression>> {
        if name == "calc" {
            let result = self.sum(depth)?;
            self.close()?;
            return Some(result);
        }
        let function = match name {
            "min" => Function::Min,
            "max" => Function::Max,
            "clamp" => Function::Clamp,
            "hypot" => Function::Hypot,
            "abs" => Function::Abs,
            "sign" => Function::Sign,
            "mod" => Function::Mod,
            "rem" => Function::Rem,
            "round" => {
                self.space();
                let rounding = if let Some(TokenKind::Ident(strategy)) = self.tokens.get(self.index)
                {
                    let rounding = match strategy.to_ascii_lowercase().as_str() {
                        "nearest" => Rounding::Nearest,
                        "up" => Rounding::Up,
                        "down" => Rounding::Down,
                        "to-zero" => Rounding::ToZero,
                        _ => return None,
                    };
                    self.index += 1;
                    self.space();
                    if self.tokens.get(self.index)? != &TokenKind::Comma {
                        return None;
                    }
                    self.index += 1;
                    rounding
                } else {
                    Rounding::Nearest
                };
                Function::Round(rounding)
            }
            "sin" => Function::Sin,
            "cos" => Function::Cos,
            "tan" => Function::Tan,
            "asin" => Function::Asin,
            "acos" => Function::Acos,
            "atan" => Function::Atan,
            "atan2" => Function::Atan2,
            "pow" => Function::Pow,
            "sqrt" => Function::Sqrt,
            "log" => Function::Log,
            "exp" => Function::Exp,
            _ => return None,
        };
        let mut args = Vec::new();
        loop {
            args.push(self.sum(depth)?);
            self.space();
            match self.tokens.get(self.index)? {
                TokenKind::CloseParen => {
                    self.index += 1;
                    break;
                }
                TokenKind::Comma => self.index += 1,
                _ => return None,
            }
        }
        let first = args[0].dimension;
        let same = args.iter().all(|arg| arg.dimension == first);
        let numbers = args.iter().all(|arg| arg.dimension == Dimension::NUMBER);
        let dimension = match function {
            Function::Min | Function::Max | Function::Hypot if same => first,
            Function::Clamp if args.len() == 3 && same => first,
            Function::Abs if args.len() == 1 => first,
            Function::Sign if args.len() == 1 => Dimension::NUMBER,
            Function::Mod | Function::Rem if args.len() == 2 && same => first,
            Function::Round(_) if args.len() == 2 && same || args.len() == 1 && numbers => first,
            Function::Sin | Function::Cos | Function::Tan
                if args.len() == 1 && (numbers || first == Dimension::ANGLE) =>
            {
                Dimension::NUMBER
            }
            Function::Asin | Function::Acos | Function::Atan if args.len() == 1 && numbers => {
                Dimension::ANGLE
            }
            Function::Atan2 if args.len() == 2 && same => Dimension::ANGLE,
            Function::Pow if args.len() == 2 && numbers => Dimension::NUMBER,
            Function::Sqrt | Function::Exp if args.len() == 1 && numbers => Dimension::NUMBER,
            Function::Log if (1..=2).contains(&args.len()) && numbers => Dimension::NUMBER,
            _ => return None,
        };
        let height = args.iter().map(|arg| arg.height).max()? + 1;
        if height > MAX_MATH_DEPTH {
            return None;
        }
        Some(Arc::new(MathExpression {
            dimension,
            node: Node::Function(function, args),
            height,
        }))
    }
}

macro_rules! scalar_type {
    ($name:ident, $dimension:ident, $field:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            pub $field: f32,
        }

        impl $name {
            pub fn parse(source: &str) -> Option<Self> {
                Self::parse_with_context(source, &LengthContext::default())
            }

            pub fn parse_with_context(source: &str, context: &LengthContext) -> Option<Self> {
                let expression = MathExpression::parse(source, None)?;
                if expression.dimension != Dimension::$dimension {
                    return None;
                }
                let value = expression.evaluate(context, 0.0)?;
                if expression.dimension == Dimension::RESOLUTION && value < 0.0 {
                    return None;
                }
                Some(Self { $field: value })
            }
        }
    };
}

scalar_type!(Angle, ANGLE, degrees);
scalar_type!(Time, TIME, seconds);
scalar_type!(Resolution, RESOLUTION, dppx);
