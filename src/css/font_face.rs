//! Finite CSS Fonts descriptors for static, variable and color faces.
use super::{AtRule, BlockItem, tokenizer::TokenKind, values::FontStyle};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FontSource {
    Url(String),
    Local(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FontDescriptor {
    pub family: String,
    pub sources: Vec<FontSource>,
    pub weight: u16,
    pub weight_range: (u16, u16),
    pub style: FontStyle,
    pub ranges: Vec<(u32, u32)>,
}

impl FontDescriptor {
    pub fn covers(&self, text: &str) -> bool {
        text.chars().all(|c| {
            crate::text::invisible(c)
                || self
                    .ranges
                    .iter()
                    .any(|&(a, b)| (a..=b).contains(&(c as u32)))
        })
    }
}

fn significant(tokens: &[TokenKind]) -> Vec<TokenKind> {
    tokens
        .iter()
        .filter(|t| !matches!(t, TokenKind::Whitespace))
        .cloned()
        .collect()
}

fn name(tokens: &[TokenKind]) -> Option<String> {
    if tokens
        .iter()
        .map(|t| match t {
            TokenKind::String(s) | TokenKind::Ident(s) => s.len(),
            _ => 0,
        })
        .sum::<usize>()
        > 4096
    {
        return None;
    }
    match tokens {
        [TokenKind::String(value)] if !value.is_empty() => Some(value.clone()),
        tokens if !tokens.is_empty() && tokens.iter().all(|t| matches!(t, TokenKind::Ident(_))) => {
            let value = tokens
                .iter()
                .map(|t| match t {
                    TokenKind::Ident(s) => s.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            (!matches!(
                value.to_ascii_lowercase().as_str(),
                "inherit"
                    | "initial"
                    | "unset"
                    | "revert"
                    | "revert-layer"
                    | "default"
                    | "serif"
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
            ))
            .then_some(value)
        }
        _ => None,
    }
}

fn function<'a>(
    tokens: &'a [TokenKind],
    index: &mut usize,
    expected: &str,
) -> Option<&'a [TokenKind]> {
    if !matches!(tokens.get(*index), Some(TokenKind::Function(s)) if s.eq_ignore_ascii_case(expected))
    {
        return None;
    }
    let start = *index + 1;
    let end = tokens[start..]
        .iter()
        .position(|t| *t == TokenKind::CloseParen)?
        + start;
    *index = end + 1;
    Some(&tokens[start..end])
}

fn source(tokens: &[TokenKind]) -> Option<FontSource> {
    let mut index = 0;
    let source = match tokens.first()? {
        TokenKind::Url(url) => {
            index = 1;
            FontSource::Url(url.clone())
        }
        TokenKind::Function(s) if s.eq_ignore_ascii_case("url") => {
            let [TokenKind::String(url)] = function(tokens, &mut index, "url")? else {
                return None;
            };
            FontSource::Url(url.clone())
        }
        TokenKind::Function(s) if s.eq_ignore_ascii_case("local") => {
            FontSource::Local(name(function(tokens, &mut index, "local")?)?)
        }
        _ => return None,
    };
    if index < tokens.len() {
        if matches!(source, FontSource::Local(_)) {
            return None;
        }
        let hints = function(tokens, &mut index, "format")?;
        let supported = hints.split(|t| *t == TokenKind::Comma).any(|hint| {
            matches!(hint, [TokenKind::String(s) | TokenKind::Ident(s)]
                if matches!(s.to_ascii_lowercase().as_str(), "truetype" | "opentype" | "woff" | "woff2"))
        });
        if !supported {
            return None;
        }
    }
    // tech(), variation format hints, SVG and EOT are explicitly unsupported.
    (index == tokens.len()).then_some(source)
}

fn sources(tokens: &[TokenKind], messages: &mut Vec<String>) -> Option<Vec<FontSource>> {
    let tokens = significant(tokens);
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for i in 0..=tokens.len() {
        if i == tokens.len() || (tokens[i] == TokenKind::Comma && depth == 0) {
            match source(&tokens[start..i]) {
                Some(source) if result.len() < 16 => result.push(source),
                _ => messages.push(
                    "invalid or unsupported @font-face src candidate; trying next source".into(),
                ),
            }
            start = i + 1;
        } else {
            match tokens[i] {
                TokenKind::Function(_) | TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    (!result.is_empty()).then_some(result)
}

fn ranges(source: &str) -> Option<Vec<(u32, u32)>> {
    let mut ranges = Vec::new();
    for part in source.split(',').take(129) {
        let part = part.trim();
        let rest = part
            .get(..2)
            .filter(|s| s.eq_ignore_ascii_case("u+"))
            .and_then(|_| part.get(2..))?;
        let hex = |s: &str| {
            (!s.is_empty() && s.len() <= 6 && s.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(s, 16).ok())
                .flatten()
        };
        let (a, b) = if rest.contains('?') {
            if rest.len() > 6
                || rest.is_empty()
                || !rest.chars().all(|c| c.is_ascii_hexdigit() || c == '?')
                || rest.trim_end_matches('?').contains('?')
            {
                return None;
            }
            (hex(&rest.replace('?', "0"))?, hex(&rest.replace('?', "F"))?)
        } else if let Some((a, b)) = rest.split_once('-') {
            (hex(a)?, hex(b)?)
        } else {
            let value = hex(rest)?;
            (value, value)
        };
        if a > b || b > 0x10ffff || ranges.len() >= 128 {
            return None;
        }
        ranges.push((a, b));
    }
    (!ranges.is_empty()).then_some(ranges)
}

pub(crate) fn parse(rule: &AtRule) -> (Option<FontDescriptor>, Vec<String>) {
    let mut messages = Vec::new();
    if !rule.prelude().trim().is_empty() || rule.block().is_none() {
        return (None, vec!["invalid @font-face rule".into()]);
    }
    if rule.block().unwrap().len() > 128 {
        return (
            None,
            vec!["@font-face descriptor count exceeds 128; ignored".into()],
        );
    }
    let mut family = None;
    let mut src = None;
    let mut weight = 400;
    let mut weight_range = (400, 400);
    let mut style = FontStyle::Normal;
    let mut unicode = vec![(0, 0x10ffff)];
    for item in rule.block().unwrap().iter().take(129) {
        let BlockItem::Declaration(d) = item else {
            messages.push("@font-face cannot contain nested rules".into());
            continue;
        };
        if d.is_important() || d.value_tokens().len() > 4096 {
            messages.push(format!("invalid @font-face {} descriptor", d.property()));
            continue;
        }
        let tokens = significant(d.value_tokens());
        let valid = match d.property().to_ascii_lowercase().as_str() {
            "font-family" => name(&tokens).map(|n| family = Some(n)).is_some(),
            "src" => sources(d.value_tokens(), &mut messages)
                .map(|s| src = Some(s))
                .is_some(),
            "font-weight" => {
                if let [TokenKind::Number(a), TokenKind::Number(b)] = tokens.as_slice() {
                    let values = a.value().ok().zip(b.value().ok()).filter(|(a, b)| {
                        (1.0..=1000.0).contains(a)
                            && a <= b
                            && *b <= 1000.0
                            && a.fract() == 0.0
                            && b.fract() == 0.0
                    });
                    if let Some((a, b)) = values {
                        weight = a as u16;
                        weight_range = (a as u16, b as u16);
                        true
                    } else {
                        false
                    }
                } else {
                    let value = match tokens.as_slice() {
                        [TokenKind::Ident(s)] if s.eq_ignore_ascii_case("normal") => Some(400),
                        [TokenKind::Ident(s)] if s.eq_ignore_ascii_case("bold") => Some(700),
                        [TokenKind::Number(n)] => n
                            .value()
                            .ok()
                            .filter(|n| (1.0..=1000.0).contains(n) && n.fract() == 0.0)
                            .map(|n| n as u16),
                        _ => None,
                    };
                    value
                        .map(|n| {
                            weight = n;
                            weight_range = (n, n);
                        })
                        .is_some()
                }
            }
            "font-style" => match tokens.as_slice() {
                [TokenKind::Ident(s)] if s.eq_ignore_ascii_case("normal") => {
                    style = FontStyle::Normal;
                    true
                }
                [TokenKind::Ident(s)] if s.eq_ignore_ascii_case("italic") => {
                    style = FontStyle::Italic;
                    true
                }
                [TokenKind::Ident(s)] if s.eq_ignore_ascii_case("oblique") => {
                    style = FontStyle::Oblique;
                    true
                }
                _ => false,
            },
            "unicode-range" => ranges(d.value()).map(|r| unicode = r).is_some(),
            "font-display" => match tokens.as_slice() {
                [TokenKind::Ident(s)]
                    if matches!(
                        s.to_ascii_lowercase().as_str(),
                        "auto" | "swap" | "block" | "fallback" | "optional"
                    ) =>
                {
                    if !s.eq_ignore_ascii_case("auto") && !s.eq_ignore_ascii_case("swap") {
                        messages.push(format!("font-display:{s}: progressive fallback-then-swap; timed block/expiry phases are not implemented"));
                    }
                    true
                }
                _ => false,
            },
            "font-stretch" | "font-width" if matches!(tokens.as_slice(), [TokenKind::Ident(s)] if s.eq_ignore_ascii_case("normal")) => {
                true
            }
            _ => {
                messages.push(format!("unsupported @font-face {} descriptor; ignored (static default-instance outlines only)", d.property()));
                true
            }
        };
        if !valid {
            messages.push(format!(
                "invalid or unsupported @font-face {} descriptor; ignored",
                d.property()
            ));
        }
        if messages.len() > 128 {
            messages.truncate(128);
            break;
        }
    }
    let descriptor = family.zip(src).map(|(family, sources)| FontDescriptor {
        family,
        sources,
        weight,
        weight_range,
        style,
        ranges: unicode,
    });
    if descriptor.is_none() {
        messages.push("@font-face requires valid font-family and src descriptors; ignored".into())
    }
    (descriptor, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(source: &str) -> (Option<FontDescriptor>, Vec<String>) {
        let sheet = crate::css::parse_recovering(&format!("@font-face {{{source}}}")).value;
        let crate::css::Rule::At(rule) = &sheet.rules()[0] else {
            panic!("at-rule")
        };
        parse(rule)
    }

    #[test]
    fn invalid_duplicate_descriptors_do_not_erase_valid_values_and_ranges_are_strict() {
        let (value, messages) = descriptor(
            r#"
          font-family:"serif";src:url(a\2e ttf);font-weight:700;font-style:italic;
          unicode-range:U+4??, U+1000-10FF;
          src:broken();font-weight:0;font-style:oblique 40deg;
          unicode-range:U+110000;font-family:serif !important;
        "#,
        );
        let value = value.unwrap();
        assert_eq!(value.family, "serif");
        assert_eq!(value.sources, [FontSource::Url("a.ttf".into())]);
        assert_eq!(value.weight, 700);
        assert_eq!(value.style, FontStyle::Italic);
        assert_eq!(value.ranges, [(0x400, 0x4ff), (0x1000, 0x10ff)]);
        assert!(messages.len() >= 5);
        for invalid in ["U+??0", "U+FFFFFF", "U+200-100", "U+10FFFFF", "U +0041"] {
            assert!(ranges(invalid).is_none(), "{invalid}");
        }
        assert!(descriptor("font-family:serif;src:url(a.ttf)").0.is_none());
    }

    #[test]
    fn source_lists_skip_unsupported_technology_and_formats_without_losing_following_candidates() {
        let (value, messages) = descriptor(
            r#"font-family:Web;
          src:url(skip.svg) format(svg),url(skip.ttf) tech(variations),
          url("font.woff2") format("woff2"),local(Web\20 Italic)"#,
        );
        assert_eq!(
            value.unwrap().sources,
            [
                FontSource::Url("font.woff2".into()),
                FontSource::Local("Web Italic".into()),
            ]
        );
        assert_eq!(messages.len(), 2);
    }
}
