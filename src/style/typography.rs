//! Finite CSS Text/Decoration/Fonts/Lists values. Every accepted value has a
//! consumer; unknown algorithms are rejected rather than stored as strings.
use std::collections::BTreeMap;

use crate::css::{
    tokenizer::TokenKind,
    values::{Color, Length, ResolvedColor, components::Component},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Property {
    WordBreak,
    OverflowWrap,
    LineBreak,
    Hyphens,
    TextTransform,
    TextJustify,
    LetterSpacing,
    WordSpacing,
    DecorationLine,
    DecorationStyle,
    DecorationColor,
    DecorationThickness,
    UnderlineOffset,
    UnderlinePosition,
    DecorationSkipInk,
    FontFeatures,
    FontVariations,
    FontOpticalSizing,
    FontKerning,
    FontLigatures,
    FontWidth,
    ListType,
    ListPosition,
    ListImage,
    CounterReset,
    CounterSet,
    CounterIncrement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    Normal,
    None,
    Auto,
    KeepAll,
    BreakAll,
    Anywhere,
    BreakWord,
    Strict,
    Loose,
    Manual,
    InterWord,
    InterCharacter,
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
    FromFont,
    Under,
    Left,
    Right,
    Inside,
    Outside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transform {
    /// 0 none, 1 uppercase, 2 lowercase, 3 capitalize.
    pub case: u8,
    pub full_width: bool,
    pub full_size_kana: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Keyword(Keyword),
    Length(Length),
    Color(Color),
    Lines(u8),
    Transform(Transform),
    Settings(BTreeMap<[u8; 4], f32>),
    Ligatures {
        common: bool,
        discretionary: bool,
        historical: bool,
        contextual: bool,
    },
    Width(f32),
    Name(String),
    Image(Option<String>),
    Counters(Vec<(String, i32)>),
}

#[derive(Clone, Debug, Default)]
pub struct Typography(BTreeMap<Property, Value>);

impl PartialEq for Typography {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .keys()
            .chain(other.0.keys())
            .all(|&p| self.get(p) == other.get(p))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decoration {
    pub lines: u8,
    pub style: Keyword,
    pub color: ResolvedColor,
    pub thickness: Value,
    pub offset: f32,
    pub position: Keyword,
    pub skip: Keyword,
    pub size: f32,
    pub face: std::sync::Arc<crate::text::FontFace>,
    pub coordinates: Vec<([u8; 4], f32)>,
}

impl Decoration {
    pub(crate) fn from_style(style: &super::ComputedStyle) -> Self {
        let t = &style.typography;
        Self {
            lines: t.lines(),
            style: t.keyword(Property::DecorationStyle),
            color: match t.get(Property::DecorationColor) {
                Value::Color(c) => c.resolve(style.text_color).unwrap_or(style.text_color),
                _ => style.text_color,
            },
            thickness: t.get(Property::DecorationThickness),
            offset: t.length(Property::UnderlineOffset, style.font_size),
            position: t.keyword(Property::UnderlinePosition),
            skip: t.keyword(Property::DecorationSkipInk),
            size: style.font_size,
            face: style.font_face.clone(),
            coordinates: style.font_face.coordinates(style),
        }
    }
}

impl Typography {
    pub fn contains(&self, p: Property) -> bool {
        self.0.contains_key(&p)
    }
    pub fn get(&self, property: Property) -> Value {
        self.0
            .get(&property)
            .cloned()
            .unwrap_or_else(|| initial(property))
    }
    pub fn keyword(&self, property: Property) -> Keyword {
        match self.get(property) {
            Value::Keyword(k) => k,
            _ => Keyword::Normal,
        }
    }
    pub fn length(&self, property: Property, size: f32) -> f32 {
        match self.get(property) {
            Value::Length(l) => l.resolve(size, size),
            _ => 0.0,
        }
    }
    pub fn settings(&self, property: Property) -> BTreeMap<[u8; 4], f32> {
        match self.get(property) {
            Value::Settings(v) => v,
            _ => BTreeMap::new(),
        }
    }
    pub fn lines(&self) -> u8 {
        match self.get(Property::DecorationLine) {
            Value::Lines(v) => v,
            _ => 0,
        }
    }
    pub fn set(&mut self, property: Property, value: Value, size: f32, color: ResolvedColor) {
        let value = match value {
            Value::Length(l)
                if matches!(
                    property,
                    Property::LetterSpacing | Property::WordSpacing | Property::UnderlineOffset
                ) =>
            {
                Value::Length(Length::Px(l.resolve(size, size)))
            }
            Value::Color(Color::CurrentColor) => Value::Color(Color::CurrentColor),
            Value::Color(c) => Value::Color(Color::Absolute(c.resolve(color).unwrap_or(color))),
            v => v,
        };
        self.0.insert(property, value);
    }
    pub(crate) fn inherit(&self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(p, _)| inherited(**p))
                .map(|(p, v)| (*p, v.clone()))
                .collect(),
        )
    }
    pub fn counters(&self, property: Property) -> Vec<(String, i32)> {
        match self.get(property) {
            Value::Counters(v) => v,
            _ => Vec::new(),
        }
    }
    pub fn name(&self, property: Property) -> String {
        match self.get(property) {
            Value::Name(v) => v,
            _ => "disc".into(),
        }
    }
}

pub fn inherited(p: Property) -> bool {
    !matches!(
        p,
        Property::DecorationLine
            | Property::DecorationStyle
            | Property::DecorationColor
            | Property::DecorationThickness
            | Property::CounterReset
            | Property::CounterSet
            | Property::CounterIncrement
    )
}

fn initial(p: Property) -> Value {
    use Keyword as K;
    use Property as P;
    match p {
        P::Hyphens => Value::Keyword(K::Manual),
        P::TextJustify
        | P::LineBreak
        | P::DecorationSkipInk
        | P::UnderlineOffset
        | P::UnderlinePosition
        | P::DecorationThickness
        | P::FontOpticalSizing
        | P::FontKerning => Value::Keyword(K::Auto),
        P::DecorationStyle => Value::Keyword(K::Solid),
        P::DecorationColor => Value::Color(Color::CurrentColor),
        P::DecorationLine => Value::Lines(0),
        P::TextTransform => Value::Transform(Transform::default()),
        P::FontFeatures | P::FontVariations => Value::Settings(BTreeMap::new()),
        P::FontLigatures => Value::Ligatures {
            common: true,
            discretionary: false,
            historical: false,
            contextual: true,
        },
        P::FontWidth => Value::Width(100.0),
        P::ListType => Value::Name("disc".into()),
        P::ListPosition => Value::Keyword(K::Outside),
        P::ListImage => Value::Image(None),
        P::CounterReset | P::CounterSet | P::CounterIncrement => Value::Counters(Vec::new()),
        _ => Value::Keyword(K::Normal),
    }
}

fn string(c: &Component) -> Option<&str> {
    match c {
        Component::Token(TokenKind::String(s)) => Some(s),
        _ => None,
    }
}

pub(crate) fn parse(p: Property, tokens: &[Component]) -> Option<Value> {
    use Keyword as K;
    use Property as P;
    let ident = if let [c] = tokens {
        c.ident().map(str::to_ascii_lowercase)
    } else {
        None
    };
    let key = match ident.as_deref() {
        Some("normal") => Some(K::Normal),
        Some("none") => Some(K::None),
        Some("auto") => Some(K::Auto),
        Some("keep-all") => Some(K::KeepAll),
        Some("break-all") => Some(K::BreakAll),
        Some("anywhere") => Some(K::Anywhere),
        Some("break-word") => Some(K::BreakWord),
        Some("strict") => Some(K::Strict),
        Some("loose") => Some(K::Loose),
        Some("manual") => Some(K::Manual),
        Some("inter-word") => Some(K::InterWord),
        Some("inter-character") => Some(K::InterCharacter),
        Some("solid") => Some(K::Solid),
        Some("double") => Some(K::Double),
        Some("dotted") => Some(K::Dotted),
        Some("dashed") => Some(K::Dashed),
        Some("wavy") => Some(K::Wavy),
        Some("from-font") => Some(K::FromFont),
        Some("under") => Some(K::Under),
        Some("left") => Some(K::Left),
        Some("right") => Some(K::Right),
        Some("inside") => Some(K::Inside),
        Some("outside") => Some(K::Outside),
        _ => None,
    };
    let allowed: &[Keyword] = match p {
        P::WordBreak => &[K::Normal, K::KeepAll, K::BreakAll],
        P::OverflowWrap => &[K::Normal, K::Anywhere, K::BreakWord],
        P::LineBreak => &[K::Auto, K::Normal, K::Strict, K::Loose, K::Anywhere],
        P::Hyphens => &[K::None, K::Manual, K::Auto],
        P::TextJustify => &[K::Auto, K::None, K::InterWord, K::InterCharacter],
        P::DecorationStyle => &[K::Solid, K::Double, K::Dotted, K::Dashed, K::Wavy],
        P::DecorationThickness => &[K::Auto, K::FromFont],
        P::UnderlineOffset => &[K::Auto],
        P::UnderlinePosition => &[K::Auto, K::Under, K::Left, K::Right],
        P::DecorationSkipInk => &[K::Auto, K::None],
        P::FontOpticalSizing => &[K::Auto, K::None],
        P::FontKerning => &[K::Auto, K::None, K::Normal],
        P::LetterSpacing | P::WordSpacing => &[K::Normal],
        P::ListPosition => &[K::Inside, K::Outside],
        _ => &[],
    };
    if let Some(k) = key
        && allowed.contains(&k)
    {
        return Some(Value::Keyword(k));
    }
    match p {
        P::LetterSpacing | P::WordSpacing | P::UnderlineOffset | P::DecorationThickness => {
            let [c] = tokens else { return None };
            let l = c.length()?;
            if matches!(p, P::LetterSpacing | P::WordSpacing) && l.has_percentage()
                || p == P::DecorationThickness && !l.is_nonnegative()
            {
                return None;
            }
            Some(Value::Length(l))
        }
        P::DecorationColor => {
            let [c] = tokens else { return None };
            c.color().map(Value::Color)
        }
        P::DecorationLine => {
            if ident.as_deref() == Some("none") {
                return Some(Value::Lines(0));
            }
            let mut lines = 0;
            for c in tokens {
                let bit = match c.ident()?.to_ascii_lowercase().as_str() {
                    "underline" => 1,
                    "overline" => 2,
                    "line-through" => 4,
                    _ => return None,
                };
                if lines & bit != 0 {
                    return None;
                }
                lines |= bit;
            }
            (lines != 0).then_some(Value::Lines(lines))
        }
        P::TextTransform => {
            if ident.as_deref() == Some("none") {
                return Some(initial(p));
            }
            let mut value = Transform::default();
            if tokens.is_empty() {
                return None;
            }
            for c in tokens {
                match c.ident()?.to_ascii_lowercase().as_str() {
                    "uppercase" if value.case == 0 => value.case = 1,
                    "lowercase" if value.case == 0 => value.case = 2,
                    "capitalize" if value.case == 0 => value.case = 3,
                    "full-width" if !value.full_width => value.full_width = true,
                    "full-size-kana" if !value.full_size_kana => value.full_size_kana = true,
                    _ => return None,
                }
            }
            Some(Value::Transform(value))
        }
        P::FontFeatures | P::FontVariations => {
            if ident.as_deref() == Some("normal") {
                return Some(initial(p));
            }
            let mut settings = BTreeMap::new();
            for part in tokens.split(|c| matches!(c, Component::Token(TokenKind::Comma))) {
                if settings.len() >= 32 || part.is_empty() || part.len() > 2 {
                    return None;
                }
                let name = string(&part[0])?;
                if name.len() != 4 || !name.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
                    return None;
                }
                let number = match part.get(1) {
                    None if p == P::FontFeatures => 1.0,
                    Some(c) if p == P::FontFeatures && c.is_ident("on") => 1.0,
                    Some(c) if p == P::FontFeatures && c.is_ident("off") => 0.0,
                    Some(c) => c.number()?,
                    _ => return None,
                };
                if p == P::FontFeatures
                    && (number < 0.0 || number.fract() != 0.0 || number > u16::MAX as f32)
                {
                    return None;
                }
                settings.insert(name.as_bytes().try_into().ok()?, number);
            }
            Some(Value::Settings(settings))
        }
        P::FontLigatures => {
            if ident.as_deref() == Some("normal") {
                return Some(initial(p));
            }
            if ident.as_deref() == Some("none") {
                return Some(Value::Ligatures {
                    common: false,
                    discretionary: false,
                    historical: false,
                    contextual: false,
                });
            }
            let mut v = [true, false, false, true];
            let mut seen = [false; 4];
            for c in tokens {
                let word = c.ident()?.to_ascii_lowercase();
                let (on, word) = word
                    .strip_prefix("no-")
                    .map_or((true, word.as_str()), |s| (false, s));
                let i = match word {
                    "common-ligatures" => 0,
                    "discretionary-ligatures" => 1,
                    "historical-ligatures" => 2,
                    "contextual" => 3,
                    _ => return None,
                };
                if seen[i] {
                    return None;
                }
                seen[i] = true;
                v[i] = on;
            }
            (!tokens.is_empty()).then_some(Value::Ligatures {
                common: v[0],
                discretionary: v[1],
                historical: v[2],
                contextual: v[3],
            })
        }
        P::FontWidth => {
            let [c] = tokens else { return None };
            let width = match c {
                Component::Token(TokenKind::Percentage(n)) => n.value().ok()? as f32,
                _ => match c.ident()?.to_ascii_lowercase().as_str() {
                    "ultra-condensed" => 50.0,
                    "extra-condensed" => 62.5,
                    "condensed" => 75.0,
                    "semi-condensed" => 87.5,
                    "normal" => 100.0,
                    "semi-expanded" => 112.5,
                    "expanded" => 125.0,
                    "extra-expanded" => 150.0,
                    "ultra-expanded" => 200.0,
                    _ => return None,
                },
            };
            (width.is_finite() && width > 0.0 && width <= 1000.0).then_some(Value::Width(width))
        }
        P::ListType => {
            let [c] = tokens else { return None };
            if let Some(s) = string(c) {
                return Some(Value::Name(format!("\"{s}")));
            }
            let name = c.ident()?;
            (!matches!(
                name.to_ascii_lowercase().as_str(),
                "initial" | "inherit" | "unset" | "revert" | "default"
            ))
            .then(|| Value::Name(name.into()))
        }
        P::ListImage => {
            if ident.as_deref() == Some("none") {
                return Some(Value::Image(None));
            }
            let [c] = tokens else { return None };
            match c {
                Component::Token(TokenKind::Url(s)) => Some(Value::Image(Some(s.clone()))),
                Component::Function(name, args) if name.eq_ignore_ascii_case("url") => {
                    let compact = args
                        .iter()
                        .filter(|t| !matches!(t, TokenKind::Whitespace))
                        .collect::<Vec<_>>();
                    match compact.as_slice() {
                        [TokenKind::String(s)] => Some(Value::Image(Some(s.clone()))),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        P::CounterReset | P::CounterSet | P::CounterIncrement => {
            if ident.as_deref() == Some("none") {
                return Some(initial(p));
            }
            let mut entries = Vec::new();
            let mut index = 0;
            while index < tokens.len() {
                if entries.len() >= 64 {
                    return None;
                }
                let name = tokens[index].ident()?;
                if matches!(
                    name,
                    "none" | "initial" | "inherit" | "unset" | "revert" | "default"
                ) {
                    return None;
                }
                index += 1;
                let value = if let Some(number) = tokens.get(index).and_then(Component::number) {
                    if number.fract() != 0.0
                        || !(i32::MIN as f32..i32::MAX as f32).contains(&number)
                    {
                        return None;
                    }
                    index += 1;
                    number as i32
                } else {
                    i32::from(p == P::CounterIncrement)
                };
                entries.push((name.into(), value));
            }
            (!entries.is_empty()).then_some(Value::Counters(entries))
        }
        _ => None,
    }
}
