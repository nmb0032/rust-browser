//! Typed container establishment values. This engine lays out horizontal-tb
//! block containers; inline non-replaced boxes do not establish size containers.
use super::tokenizer::TokenKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContainerType {
    #[default]
    Normal,
    InlineSize,
    Size,
}

impl ContainerType {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "normal" => Self::Normal,
            "inline-size" => Self::InlineSize,
            "size" => Self::Size,
            _ => return None,
        })
    }

    pub(crate) fn supports(self, width: bool, height: bool) -> bool {
        match self {
            Self::Normal => false,
            Self::InlineSize => width && !height,
            Self::Size => true,
        }
    }
}

pub(crate) fn custom_name(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "none"
            | "initial"
            | "inherit"
            | "unset"
            | "revert"
            | "revert-layer"
            | "default"
            | "and"
            | "or"
            | "not"
    )
}

pub(crate) fn names(tokens: &[TokenKind]) -> Option<Vec<String>> {
    let significant = tokens
        .iter()
        .filter(|t| **t != TokenKind::Whitespace)
        .collect::<Vec<_>>();
    if matches!(significant.as_slice(), [TokenKind::Ident(name)] if name.eq_ignore_ascii_case("none"))
    {
        return Some(Vec::new());
    }
    if significant.is_empty() || significant.len() > 256 {
        return None;
    }
    significant
        .into_iter()
        .map(|token| match token {
            TokenKind::Ident(name) if custom_name(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}
