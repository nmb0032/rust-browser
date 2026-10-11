//! Finite Grid Level 1 values. Unsupported subgrid/masonry and nested repeats
//! are rejected atomically, rather than silently turned into auto tracks.
use crate::css::{
    tokenizer::TokenKind,
    values::{
        Length,
        components::{self, Component},
    },
};

pub(crate) const MAX_GRID_TRACKS: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub enum TrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    Length(Length),
    Fraction(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrackSize {
    Breadth(TrackBreadth),
    MinMax(TrackBreadth, TrackBreadth),
    FitContent(Length),
}

impl Default for TrackSize {
    fn default() -> Self {
        Self::Breadth(TrackBreadth::Auto)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridRepeat {
    Count(usize),
    AutoFill,
    AutoFit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridTrack {
    Size(TrackSize),
    Names(Vec<String>),
    Repeat(GridRepeat, Vec<GridTrack>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GridLine {
    #[default]
    Auto,
    Line(i32, Option<String>),
    Span(usize, Option<String>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridAutoFlow {
    pub column: bool,
    pub dense: bool,
}

pub(super) fn flow(tokens: &[Component]) -> Option<GridAutoFlow> {
    if tokens.is_empty() || tokens.len() > 2 {
        return None;
    }
    let mut result = GridAutoFlow::default();
    let mut axis = false;
    for token in tokens {
        if token.is_ident("dense") && !result.dense {
            result.dense = true;
        } else if (token.is_ident("row") || token.is_ident("column")) && !axis {
            axis = true;
            result.column = token.is_ident("column");
        } else {
            return None;
        }
    }
    Some(result)
}

fn custom(name: &str) -> bool {
    ![
        "auto",
        "span",
        "default",
        "inherit",
        "initial",
        "unset",
        "revert",
        "revert-layer",
    ]
    .iter()
    .any(|word| name.eq_ignore_ascii_case(word))
}

pub(super) fn line(tokens: &[Component]) -> Option<GridLine> {
    if matches!(tokens, [token] if token.is_ident("auto")) {
        return Some(GridLine::Auto);
    }
    if tokens.is_empty() || tokens.len() > 3 {
        return None;
    }
    let (mut span, mut number, mut name) = (false, None, None);
    for token in tokens {
        match token {
            Component::Token(TokenKind::Number(n)) => {
                if number
                    .replace(n.representation.parse::<i32>().ok()?)
                    .is_some()
                {
                    return None;
                }
            }
            _ if token.is_ident("span") && !span => span = true,
            _ => {
                let value = token.ident()?;
                if !custom(value) || name.replace(value.to_owned()).is_some() {
                    return None;
                }
            }
        }
    }
    let number = number.unwrap_or(1);
    if number == 0 || span && number < 1 || span && tokens.len() == 1 {
        return None;
    }
    Some(if span {
        GridLine::Span(number as usize, name)
    } else {
        GridLine::Line(number, name)
    })
}

fn breadth(token: &Component) -> Option<TrackBreadth> {
    if token.is_ident("auto") {
        Some(TrackBreadth::Auto)
    } else if token.is_ident("min-content") {
        Some(TrackBreadth::MinContent)
    } else if token.is_ident("max-content") {
        Some(TrackBreadth::MaxContent)
    } else if let Component::Token(TokenKind::Dimension { number, unit }) = token
        && unit.eq_ignore_ascii_case("fr")
    {
        let value = number.value().ok()? as f32;
        (value.is_finite() && value >= 0.0).then_some(TrackBreadth::Fraction(value))
    } else {
        let value = token.length()?;
        value
            .is_nonnegative()
            .then_some(TrackBreadth::Length(value))
    }
}

fn size(token: &Component) -> Option<TrackSize> {
    match token {
        Component::Function(name, args) if name.eq_ignore_ascii_case("minmax") => {
            let args = components::from_tokens(args)?;
            let [min, Component::Token(TokenKind::Comma), max] = args.as_slice() else {
                return None;
            };
            let min = breadth(min)?;
            if matches!(min, TrackBreadth::Fraction(_)) {
                return None;
            }
            Some(TrackSize::MinMax(min, breadth(max)?))
        }
        Component::Function(name, args) if name.eq_ignore_ascii_case("fit-content") => {
            let args = components::from_tokens(args)?;
            let [token] = args.as_slice() else {
                return None;
            };
            let length = token.length()?;
            length
                .is_nonnegative()
                .then_some(TrackSize::FitContent(length))
        }
        _ => breadth(token).map(TrackSize::Breadth),
    }
}

fn fixed(size: &TrackSize) -> bool {
    matches!(
        size,
        TrackSize::Breadth(TrackBreadth::Length(_))
            | TrackSize::MinMax(TrackBreadth::Length(_), _)
            | TrackSize::MinMax(_, TrackBreadth::Length(_))
    )
}

pub(super) fn tracks(tokens: &[Component], automatic: bool) -> Option<Vec<GridTrack>> {
    if matches!(tokens, [token] if token.is_ident("none")) && !automatic {
        return Some(Vec::new());
    }
    parse_tracks(tokens, automatic, false)
}

fn parse_tracks(tokens: &[Component], automatic: bool, nested: bool) -> Option<Vec<GridTrack>> {
    if tokens.is_empty() || tokens.len() > MAX_GRID_TRACKS * 8 {
        return None;
    }
    let mut result = Vec::new();
    let mut index = 0;
    let mut auto_repeat = false;
    while index < tokens.len() {
        let token = &tokens[index];
        if matches!(token, Component::Token(TokenKind::OpenSquare)) && !automatic {
            let mut names = Vec::new();
            index += 1;
            while !matches!(tokens.get(index)?, Component::Token(TokenKind::CloseSquare)) {
                let name = tokens[index].ident()?;
                if !custom(name) {
                    return None;
                }
                names.push(name.to_owned());
                index += 1;
            }
            result.push(GridTrack::Names(names));
        } else if let Component::Function(name, args) = token
            && name.eq_ignore_ascii_case("repeat")
            && !automatic
            && !nested
        {
            let args = components::from_tokens(args)?;
            if !matches!(args.get(1), Some(Component::Token(TokenKind::Comma))) {
                return None;
            }
            let count = if args[0].is_ident("auto-fill") {
                GridRepeat::AutoFill
            } else if args[0].is_ident("auto-fit") {
                GridRepeat::AutoFit
            } else {
                let Component::Token(TokenKind::Number(number)) = &args[0] else {
                    return None;
                };
                let n = number.representation.parse::<usize>().ok()?;
                if n == 0 {
                    return None;
                }
                GridRepeat::Count(n)
            };
            let inner = parse_tracks(&args[2..], false, true)?;
            if !inner.iter().any(|t| matches!(t, GridTrack::Size(_))) {
                return None;
            }
            if !matches!(count, GridRepeat::Count(_)) {
                if auto_repeat {
                    return None;
                }
                auto_repeat = true;
                if inner
                    .iter()
                    .any(|t| matches!(t, GridTrack::Size(s) if !fixed(s)))
                {
                    return None;
                }
            }
            result.push(GridTrack::Repeat(count, inner));
        } else {
            result.push(GridTrack::Size(size(token)?));
        }
        index += 1;
    }
    if !result.iter().any(|t| !matches!(t, GridTrack::Names(_))) {
        return None;
    }
    if auto_repeat
        && result.iter().any(|t| match t {
            GridTrack::Size(s) => !fixed(s),
            GridTrack::Repeat(GridRepeat::Count(_), ts) => ts
                .iter()
                .any(|t| matches!(t, GridTrack::Size(s) if !fixed(s))),
            _ => false,
        })
    {
        return None;
    }
    Some(result)
}

pub(super) fn areas(tokens: &[Component]) -> Option<Vec<Vec<Option<String>>>> {
    if matches!(tokens, [token] if token.is_ident("none")) {
        return Some(Vec::new());
    }
    if tokens.is_empty() || tokens.len() > MAX_GRID_TRACKS {
        return None;
    }
    let mut rows = Vec::new();
    for token in tokens {
        let Component::Token(TokenKind::String(text)) = token else {
            return None;
        };
        // Dots are a null cell even without intervening whitespace.
        let mut cells = Vec::new();
        let mut value = String::new();
        let mut dots = false;
        for ch in text.chars().chain(std::iter::once(' ')) {
            if !value.is_empty() && (ch.is_whitespace() || (ch == '.') != dots) {
                cells.push(if dots {
                    None
                } else {
                    Some(std::mem::take(&mut value))
                });
                value.clear();
            }
            if !ch.is_whitespace() {
                if ch != '.' && !(ch.is_alphanumeric() || ch == '_' || ch == '-') {
                    return None;
                }
                dots = ch == '.';
                value.push(ch);
            }
        }
        if cells.is_empty()
            || cells.len() > MAX_GRID_TRACKS
            || rows
                .first()
                .is_some_and(|r: &Vec<_>| r.len() != cells.len())
        {
            return None;
        }
        rows.push(cells);
    }
    let mut bounds = std::collections::BTreeMap::<&str, (usize, usize, usize, usize)>::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, name) in row.iter().enumerate() {
            let Some(name) = name else { continue };
            let bound = bounds.entry(name).or_insert((r, c, r, c));
            bound.2 = r;
            bound.1 = bound.1.min(c);
            bound.3 = bound.3.max(c);
        }
    }
    for (name, (r, c, last_row, last_col)) in bounds {
        if rows[r..=last_row].iter().any(|row| {
            row[c..=last_col]
                .iter()
                .any(|cell| cell.as_deref() != Some(name))
        }) {
            return None;
        }
    }
    Some(rows)
}

pub(super) fn computed_tracks(
    values: Vec<GridTrack>,
    context: &crate::css::values::LengthContext,
) -> Option<Vec<GridTrack>> {
    fn breadth(
        value: TrackBreadth,
        context: &crate::css::values::LengthContext,
    ) -> Option<TrackBreadth> {
        Some(match value {
            TrackBreadth::Length(length) => TrackBreadth::Length(length.computed(context)?.0),
            other => other,
        })
    }
    values
        .into_iter()
        .map(|value| {
            Some(match value {
                GridTrack::Size(size) => GridTrack::Size(match size {
                    TrackSize::Breadth(value) => TrackSize::Breadth(breadth(value, context)?),
                    TrackSize::MinMax(min, max) => {
                        TrackSize::MinMax(breadth(min, context)?, breadth(max, context)?)
                    }
                    TrackSize::FitContent(length) => {
                        TrackSize::FitContent(length.computed(context)?.0)
                    }
                }),
                GridTrack::Repeat(count, values) => {
                    GridTrack::Repeat(count, computed_tracks(values, context)?)
                }
                other => other,
            })
        })
        .collect()
}

pub(super) fn placement(tokens: &[Component], count: usize) -> Option<Vec<GridLine>> {
    let parts: Vec<_> = tokens
        .split(|t| matches!(t, Component::Token(TokenKind::Delim('/'))))
        .collect();
    if parts.len() > count {
        return None;
    }
    let mut result: Vec<_> = parts.iter().map(|part| line(part)).collect::<Option<_>>()?;
    let omitted = |index: usize, line: &GridLine| {
        if parts
            .get(index)
            .is_some_and(|part| matches!(part, [token] if token.ident().is_some_and(custom)))
        {
            line.clone()
        } else {
            GridLine::Auto
        }
    };
    if count == 2 && result.len() == 1 {
        result.push(omitted(0, &result[0]));
    }
    if count == 4 {
        if result.len() == 1 {
            result.push(omitted(0, &result[0]));
        }
        if result.len() == 2 {
            result.push(omitted(0, &result[0]));
        }
        if result.len() == 3 {
            result.push(if parts.len() == 1 {
                omitted(0, &result[1])
            } else {
                omitted(1, &result[1])
            });
        }
    }
    Some(result)
}
