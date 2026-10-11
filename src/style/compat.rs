//! Declaration-local quirks, never tokenizer or standalone value-parser state.
//! https://drafts.csswg.org/css-values-4/#deprecated-quirky-length
//! https://drafts.csswg.org/css-color-4/#quirky-color

use std::borrow::Cow;

use crate::{
    css::tokenizer::{HashKind, Numeric, NumericKind, TokenKind},
    dom::{Document, DocumentMode, Namespace},
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DeclarationContext {
    quirks: bool,
}

impl DeclarationContext {
    pub(crate) fn for_document(document: &Document) -> Self {
        let html = document
            .preorder()
            .find_map(|id| document.node(id)?.as_element())
            .is_some_and(|element| {
                *element.namespace() == Namespace::Html && !element.has_case_sensitive_names()
            });
        Self {
            quirks: html && document.mode() == DocumentMode::Quirks,
        }
    }

    pub(super) fn tokens<'a>(self, name: &str, tokens: &'a [TokenKind]) -> Cow<'a, [TokenKind]> {
        let length = self.quirks
            && matches!(
                name,
                "width"
                    | "height"
                    | "font-size"
                    | "margin"
                    | "margin-top"
                    | "margin-right"
                    | "margin-bottom"
                    | "margin-left"
                    | "padding"
                    | "padding-top"
                    | "padding-right"
                    | "padding-bottom"
                    | "padding-left"
                    | "border-width"
                    | "border-top-width"
                    | "border-right-width"
                    | "border-bottom-width"
                    | "border-left-width"
            );
        let color = self.quirks
            && matches!(
                name,
                "color"
                    | "background-color"
                    | "border-color"
                    | "border-top-color"
                    | "border-right-color"
                    | "border-bottom-color"
                    | "border-left-color"
            );
        if !length && !color {
            return Cow::Borrowed(tokens);
        }
        let mut depth = 0usize;
        Cow::Owned(
            tokens
                .iter()
                .map(|token| {
                    match token {
                        TokenKind::Function(_)
                        | TokenKind::OpenParen
                        | TokenKind::OpenSquare
                        | TokenKind::OpenCurly => depth += 1,
                        TokenKind::CloseParen | TokenKind::CloseSquare | TokenKind::CloseCurly => {
                            depth = depth.saturating_sub(1)
                        }
                        _ => {}
                    }
                    if depth == 0 {
                        if length && let TokenKind::Number(number) = token {
                            return TokenKind::Dimension {
                                number: number.clone(),
                                unit: "px".into(),
                            };
                        }
                        if color && let Some(value) = quirky_hex(token) {
                            return TokenKind::Hash {
                                value,
                                kind: HashKind::Unrestricted,
                            };
                        }
                    }
                    token.clone()
                })
                .collect(),
        )
    }
}

fn integer(number: &Numeric) -> Option<String> {
    if number.kind != NumericKind::Integer {
        return None;
    }
    let value = number.value().ok()?;
    // A hex color has at most six digits. Check before formatting, including
    // overflow and negative values; signs/leading zeros are not serialized.
    (value.is_finite() && (0.0..=999_999.0).contains(&value)).then(|| {
        if value == 0.0 {
            "0".into()
        } else {
            format!("{value:.0}")
        }
    })
}

fn quirky_hex(token: &TokenKind) -> Option<String> {
    let value = match token {
        TokenKind::Ident(value) if matches!(value.len(), 3 | 6) => value.clone(),
        TokenKind::Number(number) => format!("{:0>6}", integer(number)?),
        TokenKind::Dimension { number, unit } => {
            let value = integer(number)? + unit;
            if value.len() > 6 {
                return None;
            }
            format!("{value:0>6}")
        }
        _ => return None,
    };
    (matches!(value.len(), 3 | 6) && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(value)
}
