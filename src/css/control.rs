//! Bounded token-based preludes for cascade-control rules.

use super::tokenizer::{Token, TokenKind, Tokenizer};

#[derive(Clone, Debug)]
pub(crate) enum Component {
    Token(TokenKind),
    Group {
        name: Option<String>,
        source: String,
    },
}

impl Component {
    pub(crate) fn ident(&self, expected: &str) -> bool {
        matches!(self, Self::Token(TokenKind::Ident(name)) if name.eq_ignore_ascii_case(expected))
    }
}

pub(crate) fn components(source: &str) -> Option<Vec<Component>> {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let tokens: Vec<Token> = std::iter::from_fn(|| tokenizer.next_token()).collect();
    if !tokenizer.errors().is_empty() || tokenizer.dropped_errors() > 0 {
        return None;
    }
    let mut result = Vec::new();
    let mut index = 0;
    while index + 1 < tokens.len() {
        match &tokens[index].kind {
            TokenKind::Whitespace => index += 1,
            TokenKind::OpenParen | TokenKind::Function(_) => {
                let name = match &tokens[index].kind {
                    TokenKind::Function(name) => Some(name.clone()),
                    _ => None,
                };
                let start = tokens[index].span.end.byte_offset;
                let mut depth = 1;
                index += 1;
                while index + 1 < tokens.len() && depth > 0 {
                    match tokens[index].kind {
                        TokenKind::OpenParen | TokenKind::Function(_) => depth += 1,
                        TokenKind::CloseParen => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        index += 1;
                    }
                }
                if depth != 0 {
                    return None;
                }
                result.push(Component::Group {
                    name,
                    source: source[start..tokens[index].span.start.byte_offset].to_owned(),
                });
                index += 1;
            }
            TokenKind::CloseParen | TokenKind::BadString | TokenKind::BadUrl => return None,
            token => {
                result.push(Component::Token(token.clone()));
                index += 1;
            }
        }
    }
    Some(result)
}

pub(crate) fn layer_names(source: &str) -> Option<Vec<Vec<String>>> {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let tokens: Vec<_> = std::iter::from_fn(|| tokenizer.next_token())
        .map(|token| token.kind)
        .filter(|token| *token != TokenKind::Eof)
        .collect();
    if !tokenizer.errors().is_empty() || tokenizer.dropped_errors() > 0 {
        return None;
    }
    if tokens.is_empty() {
        return None;
    }
    let mut names = Vec::new();
    let mut path = Vec::new();
    let mut ident = true;
    let mut gap = false;
    for token in tokens {
        match token {
            TokenKind::Whitespace => {
                gap = true;
                continue;
            }
            TokenKind::Ident(name)
                if ident
                    && (path.is_empty() || !gap)
                    && !matches!(
                        name.to_ascii_lowercase().as_str(),
                        "initial" | "inherit" | "unset" | "revert" | "revert-layer"
                    ) =>
            {
                path.push(name);
                ident = false;
            }
            TokenKind::Delim('.') if !ident && !gap => ident = true,
            TokenKind::Comma if !ident => {
                names.push(std::mem::take(&mut path));
                ident = true;
            }
            _ => return None,
        }
        gap = false;
    }
    if ident {
        return None;
    }
    names.push(path);
    Some(names)
}

pub(crate) fn namespace(source: &str) -> Option<(Option<String>, String)> {
    let mut tokens = components(source)?;
    let prefix = if tokens.len() == 2 {
        match tokens.remove(0) {
            Component::Token(TokenKind::Ident(prefix)) => Some(prefix),
            _ => return None,
        }
    } else {
        None
    };
    match tokens.as_slice() {
        [Component::Token(TokenKind::String(uri) | TokenKind::Url(uri))] => {
            Some((prefix, uri.clone()))
        }
        [
            Component::Group {
                name: Some(name),
                source,
            },
        ] if name.eq_ignore_ascii_case("url") => match components(source)?.as_slice() {
            [Component::Token(TokenKind::String(uri))] => Some((prefix, uri.clone())),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn single_declaration(source: &str) -> Option<super::Declaration> {
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let mut depth = 0usize;
    while let Some(token) = tokenizer.next_token() {
        match token.kind {
            TokenKind::Function(_)
            | TokenKind::OpenParen
            | TokenKind::OpenSquare
            | TokenKind::OpenCurly => depth += 1,
            TokenKind::CloseParen | TokenKind::CloseSquare | TokenKind::CloseCurly => {
                depth = depth.checked_sub(1)?
            }
            TokenKind::Semicolon if depth == 0 => return None,
            _ => {}
        }
    }
    let mut parsed = super::parse_declarations_recovering(source);
    (depth == 0
        && parsed.errors.is_empty()
        && parsed.dropped_errors == 0
        && parsed.value.len() == 1)
        .then(|| parsed.value.remove(0))
}

#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub url: String,
    pub layer: Option<Vec<String>>,
    pub supports: Option<String>,
    pub media: String,
}

pub(crate) fn import(source: &str) -> Option<Import> {
    let tokens = components(source)?;
    let first = tokens.first()?;
    let url = match first {
        Component::Token(TokenKind::String(url) | TokenKind::Url(url)) => url.clone(),
        Component::Group {
            name: Some(name),
            source,
        } if name.eq_ignore_ascii_case("url") => match components(source)?.as_slice() {
            [Component::Token(TokenKind::String(url))] => url.clone(),
            _ => return None,
        },
        _ => return None,
    };
    // Spans, rather than concatenating decoded tokens, preserve media source.
    let mut tokenizer = Tokenizer::default();
    tokenizer.feed(source);
    tokenizer.finish();
    let raw: Vec<_> = std::iter::from_fn(|| tokenizer.next_token()).collect();
    let mut ends = Vec::new();
    let mut depth = 0;
    for token in &raw {
        match token.kind {
            TokenKind::Function(_) | TokenKind::OpenParen => depth += 1,
            TokenKind::CloseParen => depth -= 1,
            TokenKind::Whitespace | TokenKind::Eof => continue,
            _ => {}
        }
        if depth == 0 {
            ends.push(token.span.end.byte_offset);
        }
    }
    let mut index = 1;
    let mut layer = None;
    if tokens.get(index).is_some_and(|token| token.ident("layer")) {
        layer = Some(Vec::new());
        index += 1;
    } else if let Some(Component::Group {
        name: Some(name),
        source,
    }) = tokens.get(index)
        && name.eq_ignore_ascii_case("layer")
    {
        let names = if source.trim().is_empty() {
            vec![Vec::new()]
        } else {
            layer_names(source)?
        };
        if names.len() != 1 {
            return None;
        }
        layer = names.into_iter().next();
        index += 1;
    }
    let mut supports = None;
    if let Some(Component::Group {
        name: Some(name),
        source,
    }) = tokens.get(index)
        && name.eq_ignore_ascii_case("supports")
    {
        if source.trim().is_empty() {
            return None;
        }
        // supports() also accepts an unparenthesized declaration.
        supports = Some(if single_declaration(source).is_some() {
            format!("({source})")
        } else {
            source.clone()
        });
        index += 1;
    }
    let media = source.get(*ends.get(index - 1)?..)?.trim().to_owned();
    Some(Import {
        url,
        layer,
        supports,
        media,
    })
}

pub(crate) fn scope(source: &str) -> Option<(Option<String>, Option<String>)> {
    let tokens = components(source)?;
    match tokens.as_slice() {
        [] => Some((None, None)),
        [Component::Group { name: None, source }] => Some((Some(source.clone()), None)),
        [
            Component::Group {
                name: None,
                source: root,
            },
            to,
            Component::Group {
                name: None,
                source: limit,
            },
        ] if to.ident("to") => Some((Some(root.clone()), Some(limit.clone()))),
        [
            to,
            Component::Group {
                name: None,
                source: limit,
            },
        ] if to.ident("to") => Some((None, Some(limit.clone()))),
        _ => None,
    }
}
