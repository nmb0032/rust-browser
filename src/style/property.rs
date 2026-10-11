//! A deliberately finite registration grammar. Registered values are computed
//! to canonical tokens before dependent var() values are substituted.

use std::{collections::BTreeMap, sync::Arc};

use crate::css::{
    self, AtRule, BlockItem,
    tokenizer::TokenKind,
    values::{
        Color, ColorSpace, Dimension, Length, LengthContext, MathExpression, ResolvedColor, math,
    },
};

use super::{
    control, declarations,
    variables::{self, Environment},
};

pub(super) type Registrations = BTreeMap<String, Registration>;

#[derive(Clone, Debug)]
pub(super) struct Registration {
    syntax: Syntax,
    pub inherits: bool,
    pub initial: Option<Arc<[TokenKind]>>,
}

#[derive(Clone, Copy, Debug)]
enum Syntax {
    Universal,
    Length,
    Number,
    Integer,
    Percentage,
    Color,
}

fn independent(tokens: &[TokenKind]) -> bool {
    !variables::contains_var(tokens)
        && !tokens.iter().any(|token| match token {
            TokenKind::Dimension { unit, .. } => {
                matches!(math::unit(unit), Some((_, math::Unit::Relative(_))))
            }
            TokenKind::Ident(name) => name.eq_ignore_ascii_case("currentcolor"),
            TokenKind::Function(name) => {
                name.eq_ignore_ascii_case("env") || name.eq_ignore_ascii_case("attr")
            }
            _ => false,
        })
}

impl Registration {
    pub fn parse(rule: &AtRule) -> Result<(String, Self), String> {
        let name = css::control::components(rule.prelude())
            .and_then(|values| match values.as_slice() {
                [css::control::Component::Token(TokenKind::Ident(name))]
                    if name.starts_with("--") && name != "--" =>
                {
                    Some(name.clone())
                }
                _ => None,
            })
            .ok_or("invalid @property name")?;
        let mut syntax = None;
        let mut inherits = None;
        let mut initial = None;
        for item in rule.block().ok_or("@property requires descriptors")? {
            let BlockItem::Declaration(declaration) = item else {
                return Err("nested rule in @property".into());
            };
            if declaration.is_important() {
                return Err("!important is not allowed in @property descriptors".into());
            }
            match declaration.property().to_ascii_lowercase().as_str() {
                "syntax" => {
                    let tokens: Vec<_> = declaration
                        .value_tokens()
                        .iter()
                        .filter(|token| !matches!(token, TokenKind::Whitespace))
                        .collect();
                    syntax = Some(match tokens.as_slice() {
                        [TokenKind::String(value)] => match value.as_str() {
                            "*" => Syntax::Universal,
                            "<length>" => Syntax::Length,
                            "<number>" => Syntax::Number,
                            "<integer>" => Syntax::Integer,
                            "<percentage>" => Syntax::Percentage,
                            "<color>" => Syntax::Color,
                            _ => return Err("unsupported @property syntax (supported: *, <length>, <number>, <integer>, <percentage>, <color>; no lists or alternatives)".into()),
                        },
                        _ => return Err("@property syntax must be one string".into()),
                    });
                }
                "inherits" => {
                    inherits = Some(
                        if control::is_ident_token(declaration.value_tokens(), "true") {
                            true
                        } else if control::is_ident_token(declaration.value_tokens(), "false") {
                            false
                        } else {
                            return Err("@property inherits must be true or false".into());
                        },
                    )
                }
                "initial-value" => {
                    initial = Some(Arc::<[TokenKind]>::from(declaration.value_tokens()))
                }
                _ => {
                    return Err(format!(
                        "unsupported @property descriptor: {}",
                        declaration.property()
                    ));
                }
            }
        }
        let mut registration = Self {
            syntax: syntax.ok_or("@property requires syntax")?,
            inherits: inherits.ok_or("@property requires inherits")?,
            initial: None,
        };
        if let Some(initial) = initial {
            if !independent(&initial) || declarations::custom_wide(&initial).is_some() {
                return Err(
                    "@property initial-value must be computationally independent and not CSS-wide"
                        .into(),
                );
            }
            registration.initial = Some(
                registration
                    .compute(&initial)
                    .ok_or("@property initial-value does not match the supported typed grammar")?,
            );
        } else if !matches!(registration.syntax, Syntax::Universal) {
            return Err("typed @property requires initial-value".into());
        }
        Ok((name, registration))
    }

    pub fn compute(&self, tokens: &[TokenKind]) -> Option<Arc<[TokenKind]>> {
        if matches!(self.syntax, Syntax::Universal) {
            return Some(tokens.into());
        }
        // Relative registered lengths/currentcolor need the CSS dependency
        // model for font-size and color cycles, not a parent-metric shortcut.
        if !independent(tokens) {
            return None;
        }
        let compact: Vec<_> = tokens
            .iter()
            .filter(|token| !matches!(token, TokenKind::Whitespace))
            .collect();
        let source = match self.syntax {
            Syntax::Universal => unreachable!(),
            Syntax::Length => {
                let length = Length::from_tokens(tokens)?;
                if length.has_percentage() {
                    return None;
                }
                format!("{}px", length.used(&LengthContext::default(), 0.0)?.0)
            }
            Syntax::Percentage => match compact.as_slice() {
                [TokenKind::Percentage(number)] if number.value().ok()?.is_finite() => {
                    format!("{}%", number.value().ok()?)
                }
                _ => return None,
            },
            Syntax::Number | Syntax::Integer => {
                let number = match compact.as_slice() {
                    [TokenKind::Number(number)] => number.value().ok()?,
                    _ => {
                        let expression = MathExpression::from_tokens(tokens, None)?;
                        if expression.dimension != Dimension::NUMBER {
                            return None;
                        }
                        expression.evaluate64(&LengthContext::default(), 0.0)?
                    }
                };
                if !number.is_finite()
                    || matches!(self.syntax, Syntax::Integer) && number.fract() != 0.0
                {
                    return None;
                }
                number.to_string()
            }
            Syntax::Color => {
                let color = Color::from_tokens(tokens)?
                    .resolve(ResolvedColor::BLACK)?
                    .convert(ColorSpace::Srgb);
                let component = |index: usize| {
                    if color.missing & (1 << index) != 0 {
                        "none".to_owned()
                    } else if index == 3 {
                        color.alpha.to_string()
                    } else {
                        color.channels[index].to_string()
                    }
                };
                format!(
                    "color(srgb {} {} {} / {})",
                    component(0),
                    component(1),
                    component(2),
                    component(3)
                )
            }
        };
        math::tokenize(&source).map(Arc::from)
    }

    pub fn default_value(&self, parent: &Environment, name: &str) -> Option<Arc<[TokenKind]>> {
        if self.inherits {
            parent
                .get(name)
                .cloned()
                .flatten()
                .or_else(|| self.initial.clone())
        } else {
            self.initial.clone()
        }
    }
}
