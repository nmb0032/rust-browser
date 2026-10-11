use crate::css::tokenizer::{Numeric, TokenKind};

use super::{Color, Dimension, Length, MathExpression};

/// One top-level component value, keeping function arguments together.
#[derive(Clone, Debug)]
pub(crate) enum Component {
    Token(TokenKind),
    Function(String, Vec<TokenKind>),
}

impl Component {
    pub(crate) fn ident(&self) -> Option<&str> {
        match self {
            Self::Token(TokenKind::Ident(name)) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn is_ident(&self, expected: &str) -> bool {
        self.ident()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
    }

    pub(crate) fn number(&self) -> Option<f32> {
        match self {
            Self::Token(TokenKind::Number(number)) => finite(number),
            _ => None,
        }
    }

    pub(crate) fn number_math(&self) -> Option<std::sync::Arc<MathExpression>> {
        match self {
            Self::Function(name, arguments) => {
                if arguments.len() + 2 > super::math::MAX_MATH_TOKENS {
                    return None;
                }
                let mut tokens = Vec::with_capacity(arguments.len() + 2);
                tokens.push(TokenKind::Function(name.clone()));
                tokens.extend_from_slice(arguments);
                tokens.push(TokenKind::CloseParen);
                let expression = MathExpression::from_tokens(&tokens, None)?;
                (expression.dimension == Dimension::NUMBER).then_some(expression)
            }
            _ => None,
        }
    }

    pub(crate) fn length(&self) -> Option<Length> {
        match self {
            Self::Token(token) => Length::from_tokens(std::slice::from_ref(token)),
            Self::Function(name, arguments) => {
                if arguments.len() + 2 > super::math::MAX_MATH_TOKENS {
                    return None;
                }
                let mut tokens = Vec::with_capacity(arguments.len() + 2);
                tokens.push(TokenKind::Function(name.clone()));
                tokens.extend_from_slice(arguments);
                tokens.push(TokenKind::CloseParen);
                Length::from_tokens(&tokens)
            }
        }
    }

    pub(crate) fn color(&self) -> Option<Color> {
        let tokens = match self {
            Self::Token(token) => vec![token.clone()],
            Self::Function(name, arguments) => {
                if arguments.len() + 2 > super::math::MAX_MATH_TOKENS {
                    return None;
                }
                let mut tokens = vec![TokenKind::Function(name.clone())];
                tokens.extend_from_slice(arguments);
                tokens.push(TokenKind::CloseParen);
                tokens
            }
        };
        Color::from_tokens(&tokens)
    }
}

fn finite(number: &Numeric) -> Option<f32> {
    let value = number.value().ok()? as f32;
    value.is_finite().then_some(value)
}

/// Parse already-tokenized values without changing substitution boundaries.
pub(crate) fn from_tokens(tokens: &[TokenKind]) -> Option<Vec<Component>> {
    let mut values = Vec::new();
    let mut tokens = tokens.iter().cloned();
    while let Some(token) = tokens.next() {
        if matches!(token, TokenKind::Whitespace) {
            continue;
        } else if let TokenKind::Function(name) = token {
            let mut arguments = Vec::new();
            let mut depth = 1;
            for token in tokens.by_ref() {
                match token {
                    TokenKind::Function(_) | TokenKind::OpenParen => depth += 1,
                    TokenKind::CloseParen => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
                arguments.push(token);
            }
            if depth != 0 {
                return None;
            }
            values.push(Component::Function(name, arguments));
        } else {
            values.push(Component::Token(token));
        }
    }
    Some(values)
}
