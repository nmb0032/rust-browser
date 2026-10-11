//! Bounded Counter Styles 3 formatting, independent of DOM and painting.
use super::{
    AtRule, BlockItem,
    tokenizer::TokenKind,
    values::components::{self, Component},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum System {
    Cyclic,
    Fixed(i32),
    Symbolic,
    Alphabetic,
    Numeric,
    Additive,
    Extends(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterStyle {
    pub name: String,
    system: System,
    symbols: Vec<String>,
    additive: Vec<(u32, String)>,
    negative: (String, String),
    prefix: String,
    suffix: String,
    range: Vec<(i64, i64)>,
    pad: (usize, String),
    fallback: String,
    supplied: BTreeSet<String>,
}

fn symbol(c: &Component) -> Option<String> {
    match c {
        Component::Token(TokenKind::String(s) | TokenKind::Ident(s)) if s.len() <= 4096 => {
            Some(s.clone())
        }
        _ => None,
    }
}
fn integer(c: &Component) -> Option<i64> {
    let n = c.number()? as f64;
    (n.fract() == 0.0 && n.abs() <= i32::MAX as f64).then_some(n as i64)
}

impl CounterStyle {
    fn new(name: String) -> Self {
        Self {
            name,
            system: System::Symbolic,
            symbols: Vec::new(),
            additive: Vec::new(),
            negative: ("-".into(), "".into()),
            prefix: String::new(),
            suffix: ". ".into(),
            range: Vec::new(),
            pad: (0, String::new()),
            fallback: "decimal".into(),
            supplied: BTreeSet::new(),
        }
    }
    pub(crate) fn parse(rule: &AtRule) -> Result<(Self, Vec<String>), String> {
        let name = components::from_tokens(
            &super::values::math::tokenize(rule.prelude()).ok_or("invalid counter-style name")?,
        )
        .and_then(|v| match v.as_slice() {
            [c] => c.ident().map(str::to_owned),
            _ => None,
        })
        .ok_or("counter-style requires a custom identifier")?;
        if matches!(
            name.as_str(),
            "none" | "decimal" | "disc" | "initial" | "inherit" | "unset" | "revert" | "default"
        ) {
            return Err("reserved counter-style name".into());
        }
        let mut style = Self::new(name);
        let mut diagnostics = Vec::new();
        for item in rule.block().ok_or("counter-style requires a block")? {
            let BlockItem::Declaration(d) = item else {
                return Err("counter-style cannot contain nested rules".into());
            };
            if d.is_important() {
                return Err("counter-style descriptors cannot be important".into());
            }
            let name = d.property().to_ascii_lowercase();
            let tokens = components::from_tokens(d.value_tokens())
                .ok_or("invalid counter-style descriptor")?;
            match name.as_str() {
                "system" => {
                    style.system = match tokens.as_slice() {
                        [c] => match c.ident().unwrap_or("").to_ascii_lowercase().as_str() {
                            "cyclic" => System::Cyclic,
                            "fixed" => System::Fixed(1),
                            "symbolic" => System::Symbolic,
                            "alphabetic" => System::Alphabetic,
                            "numeric" => System::Numeric,
                            "additive" => System::Additive,
                            _ => return Err("invalid counter-style system".into()),
                        },
                        [c, value] if c.is_ident("fixed") => {
                            System::Fixed(integer(value).ok_or("invalid fixed start")? as i32)
                        }
                        [c, value] if c.is_ident("extends") => {
                            System::Extends(value.ident().ok_or("invalid extends name")?.into())
                        }
                        _ => return Err("invalid counter-style system".into()),
                    };
                }
                "symbols" => {
                    if tokens.is_empty() || tokens.len() > 256 {
                        return Err("symbols limit (1..256)".into());
                    }
                    style.symbols = tokens
                        .iter()
                        .map(symbol)
                        .collect::<Option<_>>()
                        .ok_or("only text symbols are supported")?;
                }
                "additive-symbols" => {
                    style.additive.clear();
                    for part in tokens.split(|c| matches!(c, Component::Token(TokenKind::Comma))) {
                        let [a, b] = part else {
                            return Err("invalid additive symbol".into());
                        };
                        let (weight, text) = if let Some(i) = integer(a) {
                            (i, symbol(b))
                        } else {
                            (integer(b).ok_or("invalid additive weight")?, symbol(a))
                        };
                        if weight < 0
                            || style
                                .additive
                                .last()
                                .is_some_and(|&(last, _)| i64::from(last) <= weight)
                            || style.additive.len() >= 256
                        {
                            return Err("additive weights must be descending, distinct, nonnegative and bounded".into());
                        }
                        style
                            .additive
                            .push((weight as u32, text.ok_or("unsupported additive symbol")?));
                    }
                }
                "negative" => {
                    style.negative = match tokens.as_slice() {
                        [a] => (symbol(a).ok_or("invalid negative")?, String::new()),
                        [a, b] => (
                            symbol(a).ok_or("invalid negative")?,
                            symbol(b).ok_or("invalid negative")?,
                        ),
                        _ => return Err("invalid negative descriptor".into()),
                    };
                }
                "prefix" | "suffix" => {
                    let [c] = tokens.as_slice() else {
                        return Err("invalid prefix/suffix".into());
                    };
                    let s = symbol(c).ok_or("invalid prefix/suffix")?;
                    if name == "prefix" {
                        style.prefix = s;
                    } else {
                        style.suffix = s;
                    }
                }
                "pad" => {
                    let [a, b] = tokens.as_slice() else {
                        return Err("invalid pad".into());
                    };
                    let (n, s) = if let Some(n) = integer(a) {
                        (n, symbol(b))
                    } else {
                        (integer(b).ok_or("invalid pad width")?, symbol(a))
                    };
                    if !(0..=256).contains(&n) {
                        return Err("counter pad limit is 256".into());
                    }
                    style.pad = (n as usize, s.ok_or("invalid pad symbol")?);
                }
                "range" => {
                    style.range.clear();
                    if tokens.len() == 1 && tokens[0].is_ident("auto") {
                        continue;
                    }
                    for part in tokens.split(|c| matches!(c, Component::Token(TokenKind::Comma))) {
                        let [a, b] = part else {
                            return Err("invalid range".into());
                        };
                        let low = if a.is_ident("infinite") {
                            i64::MIN
                        } else {
                            integer(a).ok_or("invalid range bound")?
                        };
                        let high = if b.is_ident("infinite") {
                            i64::MAX
                        } else {
                            integer(b).ok_or("invalid range bound")?
                        };
                        if low > high || style.range.len() >= 64 {
                            return Err("invalid/bounded range".into());
                        }
                        style.range.push((low, high));
                    }
                }
                "fallback" => {
                    let [c] = tokens.as_slice() else {
                        return Err("invalid fallback".into());
                    };
                    style.fallback = c.ident().ok_or("invalid fallback")?.into();
                }
                "speak-as" => {
                    diagnostics.push("@counter-style speak-as is not implemented (no speech engine); visual formatting retained".into());
                    continue;
                }
                _ => {
                    diagnostics.push(format!("unsupported @counter-style descriptor: {name}"));
                    continue;
                }
            }
            style.supplied.insert(name);
        }
        let valid = match style.system {
            System::Extends(_) => style.symbols.is_empty() && style.additive.is_empty(),
            System::Numeric | System::Alphabetic => style.symbols.len() >= 2,
            System::Additive => !style.additive.is_empty(),
            _ => !style.symbols.is_empty(),
        };
        if !valid {
            return Err(
                "counter-style lacks required symbols or has invalid extends descriptors".into(),
            );
        }
        Ok((style, diagnostics))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CounterStyles(pub(crate) BTreeMap<String, CounterStyle>);

impl CounterStyles {
    pub(crate) fn insert(&mut self, style: CounterStyle) {
        if self.0.len() < 256 || self.0.contains_key(&style.name) {
            self.0.insert(style.name.clone(), style);
        }
    }
    fn resolved(&self, name: &str, visited: &mut Vec<String>) -> Option<CounterStyle> {
        if visited.len() >= 16 || visited.iter().any(|s| s == name) {
            return None;
        }
        let mut style = self.0.get(name).cloned().or_else(|| builtin(name))?;
        if let System::Extends(ref base) = style.system {
            visited.push(name.into());
            let inherited = self.resolved(base, visited)?;
            style.system = inherited.system;
            style.symbols = inherited.symbols;
            style.additive = inherited.additive;
            if !style.supplied.contains("negative") {
                style.negative = inherited.negative;
            }
            if !style.supplied.contains("prefix") {
                style.prefix = inherited.prefix;
            }
            if !style.supplied.contains("suffix") {
                style.suffix = inherited.suffix;
            }
            if !style.supplied.contains("range") {
                style.range = inherited.range;
            }
            if !style.supplied.contains("pad") {
                style.pad = inherited.pad;
            }
            if !style.supplied.contains("fallback") {
                style.fallback = inherited.fallback;
            }
            visited.pop();
        }
        Some(style)
    }
    pub fn format(&self, name: &str, value: i32, marker: bool) -> (String, Option<String>) {
        if name == "none" {
            return (String::new(), None);
        }
        if let Some(s) = name.strip_prefix('"') {
            return (if marker { format!("{s} ") } else { s.into() }, None);
        }
        let mut visited = Vec::new();
        let mut name = name.to_owned();
        let mut issue = None;
        loop {
            if visited.len() >= 16 || visited.contains(&name) {
                issue = Some(
                    "counter-style fallback/extends cycle or depth limit; decimal fallback".into(),
                );
                return (
                    if marker {
                        format!("{value}. ")
                    } else {
                        value.to_string()
                    },
                    issue,
                );
            }
            let Some(style) = self.resolved(&name, &mut Vec::new()) else {
                return (
                    if marker {
                        format!("{value}. ")
                    } else {
                        value.to_string()
                    },
                    Some(format!(
                        "unknown/cyclic counter-style {name:?}; decimal fallback"
                    )),
                );
            };
            visited.push(name.clone());
            if let Some(mut text) = representation(&style, value) {
                if marker {
                    text = format!("{}{}{}", style.prefix, text, style.suffix);
                }
                if text.len() <= 65_536 {
                    return (text, issue);
                }
                issue =
                    Some("counter-style output exceeds 65536-byte limit; decimal fallback".into());
                name = "decimal".into();
            } else {
                name = style.fallback;
            }
        }
    }
}

fn representation(style: &CounterStyle, value: i32) -> Option<String> {
    use unicode_segmentation::UnicodeSegmentation;
    if !style.range.is_empty()
        && !style
            .range
            .iter()
            .any(|&(a, b)| a <= i64::from(value) && i64::from(value) <= b)
    {
        return None;
    }
    let mut n = i64::from(value).unsigned_abs();
    let negative = value < 0 && !matches!(style.system, System::Cyclic | System::Fixed(_));
    let mut parts = Vec::new();
    let mut work = 0usize;
    let mut push = |s: &str| -> Option<()> {
        work = work.checked_add(s.len())?;
        if parts.len() >= 4096 || work > 65_536 {
            return None;
        }
        parts.push(s.to_owned());
        Some(())
    };
    let count = style.symbols.len() as u64;
    match style.system {
        System::Cyclic => {
            if count == 0 {
                return None;
            }
            push(&style.symbols[(i64::from(value) - 1).rem_euclid(count as i64) as usize])?;
        }
        System::Fixed(first) => {
            let i = i64::from(value) - i64::from(first);
            if i < 0 || i as u64 >= count {
                return None;
            }
            push(&style.symbols[i as usize])?;
        }
        System::Symbolic => {
            if value < 1 || count == 0 {
                return None;
            }
            let repeats = ((n - 1) / count) + 1;
            if repeats > 4096 {
                return None;
            }
            for _ in 0..repeats {
                push(&style.symbols[((n - 1) % count) as usize])?;
            }
        }
        System::Numeric | System::Alphabetic => {
            if count < 2 || matches!(style.system, System::Alphabetic) && value < 1 {
                return None;
            }
            if n == 0 {
                push(&style.symbols[0])?;
            }
            while n != 0 {
                if style.system == System::Alphabetic {
                    n -= 1;
                }
                push(&style.symbols[(n % count) as usize])?;
                n /= count;
            }
            parts.reverse();
        }
        System::Additive => {
            if value == 0 {
                push(&style.additive.iter().find(|(w, _)| *w == 0)?.1)?;
            }
            for (weight, s) in &style.additive {
                if *weight == 0 {
                    continue;
                }
                let copies = n / u64::from(*weight);
                if copies > 4096 {
                    return None;
                }
                for _ in 0..copies {
                    push(s)?;
                }
                n %= u64::from(*weight);
            }
            if n != 0 {
                return None;
            }
        }
        System::Extends(_) => return None,
    }
    let mut text = parts.concat();
    let length = text.graphemes(true).count()
        + if negative {
            style.negative.0.graphemes(true).count() + style.negative.1.graphemes(true).count()
        } else {
            0
        };
    if style.pad.0 > length {
        text = format!("{}{}", style.pad.1.repeat(style.pad.0 - length), text);
    }
    if negative {
        text = format!("{}{}{}", style.negative.0, text, style.negative.1);
    }
    (text.len() <= 65_536).then_some(text)
}

fn builtin(name: &str) -> Option<CounterStyle> {
    let mut style = CounterStyle::new(name.into());
    style.system = System::Numeric;
    let alphabet = match name {
        "decimal" | "decimal-leading-zero" => "0123456789",
        "arabic-indic" => "٠١٢٣٤٥٦٧٨٩",
        "persian" => "۰۱۲۳۴۵۶۷۸۹",
        "devanagari" => "०१२३४५६७८९",
        "bengali" => "০১২৩৪৫৬৭৮৯",
        "thai" => "๐๑๒๓๔๕๖๗๘๙",
        "cjk-decimal" => "〇一二三四五六七八九",
        "lower-alpha" | "lower-latin" => {
            style.system = System::Alphabetic;
            "abcdefghijklmnopqrstuvwxyz"
        }
        "upper-alpha" | "upper-latin" => {
            style.system = System::Alphabetic;
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        }
        "lower-greek" => {
            style.system = System::Alphabetic;
            "αβγδεζηθικλμνξοπρστυφχψω"
        }
        "disc" | "circle" | "square" => {
            style.system = System::Cyclic;
            style.suffix = " ".into();
            match name {
                "disc" => "•",
                "circle" => "◦",
                _ => "▪",
            }
        }
        "lower-roman" | "upper-roman" => {
            style.system = System::Additive;
            style.range.push((1, 3999));
            style.additive = [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ]
            .into_iter()
            .map(|(n, s)| {
                (
                    n,
                    if name == "lower-roman" {
                        s.to_lowercase()
                    } else {
                        s.into()
                    },
                )
            })
            .collect();
            ""
        }
        _ => return None,
    };
    style.symbols = alphabet.chars().map(|c| c.to_string()).collect();
    if name == "decimal-leading-zero" {
        style.pad = (2, "0".into());
    }
    Some(style)
}
