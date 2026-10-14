//! Declarative CSS timing and the native Web Animations model. No JS dispatch.
use std::collections::BTreeMap;

use crate::css::{
    tokenizer::TokenKind,
    values::{
        Length,
        components::{self, Component},
    },
};

pub const MAX_EFFECTS: usize = 2048;
pub const MAX_LIST: usize = 64;
pub const MAX_KEYFRAMES: usize = 256;
pub const MAX_TIME: f64 = 1_000_000_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Property {
    Name,
    Duration,
    Delay,
    EndDelay,
    Iterations,
    Direction,
    Fill,
    Play,
    Easing,
    Composition,
    Timeline,
    RangeStart,
    RangeEnd,
    TransitionProperty,
    TransitionDuration,
    TransitionDelay,
    TransitionEasing,
    ScrollName,
    ScrollAxis,
    ViewName,
    ViewAxis,
    ViewInset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    Forwards,
    Backwards,
    Both,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Composition {
    Replace,
    Add,
    Accumulate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Block,
    Inline,
    X,
    Y,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroller {
    Nearest,
    Root,
    Self_,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnimationName {
    None,
    Name(String),
}
#[derive(Clone, Debug, PartialEq)]
pub enum Timeline {
    Auto,
    None,
    Named(String),
    Scroll(Scroller, Axis),
    View(Axis, [Option<Length>; 2]),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeName {
    Normal,
    Cover,
    Contain,
    Entry,
    Exit,
    EntryCrossing,
    ExitCrossing,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RangeOffset {
    pub name: RangeName,
    pub offset: Length,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    Start,
    End,
    None,
    Both,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Easing {
    Linear,
    Bezier([f64; 4]),
    Steps(u32, Jump),
}

impl Default for Easing {
    fn default() -> Self {
        Self::Bezier([0.25, 0.1, 0.25, 1.0])
    }
}

impl Easing {
    pub fn sample(&self, x: f64, before: bool) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match *self {
            Self::Linear => x,
            Self::Bezier([x1, y1, x2, y2]) => {
                if x == 0.0 || x == 1.0 {
                    return x;
                }
                let cubic = |t: f64, a: f64, b: f64| {
                    3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
                };
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..48 {
                    let t = (lo + hi) * 0.5;
                    if cubic(t, x1, x2) < x {
                        lo = t;
                    } else {
                        hi = t;
                    }
                }
                cubic((lo + hi) * 0.5, y1, y2)
            }
            Self::Steps(n, jump) => {
                let n = f64::from(n);
                let mut step = (x * n).floor();
                if matches!(jump, Jump::Start | Jump::Both) {
                    step += 1.0;
                }
                if before && (x * n).fract() == 0.0 {
                    step -= 1.0;
                }
                let jumps = n + match jump {
                    Jump::Both => 1.0,
                    Jump::None => -1.0,
                    _ => 0.0,
                };
                (step / jumps).clamp(0.0, 1.0)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    AnimationNames(Vec<AnimationName>),
    Names(Vec<String>),
    Times(Vec<Option<f64>>),
    Counts(Vec<f64>),
    Directions(Vec<Direction>),
    Fills(Vec<Fill>),
    Plays(Vec<bool>),
    Easings(Vec<Easing>),
    Compositions(Vec<Composition>),
    Timelines(Vec<Timeline>),
    Ranges(Vec<RangeOffset>),
    Axes(Vec<Axis>),
    Insets(Vec<[Option<Length>; 2]>),
}

pub(crate) fn compute(
    mut value: Value,
    context: &crate::css::values::LengthContext,
) -> Option<Value> {
    fn inset(
        values: &mut [Option<Length>; 2],
        context: &crate::css::values::LengthContext,
    ) -> Option<()> {
        for length in values.iter_mut().flatten() {
            *length = length.computed(context)?.0;
        }
        Some(())
    }
    match &mut value {
        Value::Ranges(values) => {
            for value in values {
                value.offset = value.offset.computed(context)?.0;
            }
        }
        Value::Insets(values) => {
            for value in values {
                inset(value, context)?;
            }
        }
        Value::Timelines(values) => {
            for value in values {
                if let Timeline::View(_, values) = value {
                    inset(values, context)?;
                }
            }
        }
        _ => {}
    }
    Some(value)
}

#[derive(Clone, Debug, Default)]
pub struct Motion(BTreeMap<Property, Value>);
impl PartialEq for Motion {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .keys()
            .chain(other.0.keys())
            .all(|&p| self.get(p) == other.get(p))
    }
}
impl Motion {
    /// Declares a named animation or a positive transition duration; otherwise
    /// the animation engine never creates a target for the element.
    pub(crate) fn active(&self) -> bool {
        matches!(self.0.get(&Property::Name), Some(Value::AnimationNames(names)) if names.iter().any(|n| matches!(n, AnimationName::Name(_))))
            || matches!(self.0.get(&Property::TransitionDuration), Some(Value::Times(v)) if v.iter().any(|t| t.is_some_and(|t| t > 0.0)))
    }
    pub fn get(&self, p: Property) -> Value {
        self.0.get(&p).cloned().unwrap_or_else(|| initial(p))
    }
    pub fn set(&mut self, p: Property, v: Value) {
        self.0.insert(p, v);
    }
    pub(crate) fn configs(&self) -> Vec<Config> {
        let Value::AnimationNames(names) = self.get(Property::Name) else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for (i, name) in names.into_iter().enumerate().take(MAX_LIST) {
            let (name, enabled) = match name {
                AnimationName::None => (String::new(), false),
                AnimationName::Name(name) => (name, true),
            };
            let time = |p| match self.get(p) {
                Value::Times(v) => v[i % v.len()],
                _ => Some(0.0),
            };
            let mut c = Config {
                name,
                enabled,
                duration: time(Property::Duration),
                delay: time(Property::Delay).unwrap_or(0.0),
                ..Config::default()
            };
            if let Value::Counts(v) = self.get(Property::Iterations) {
                c.timing.iterations = v[i % v.len()];
            }
            if let Value::Directions(v) = self.get(Property::Direction) {
                c.timing.direction = v[i % v.len()];
            }
            if let Value::Fills(v) = self.get(Property::Fill) {
                c.timing.fill = v[i % v.len()];
            }
            if let Value::Plays(v) = self.get(Property::Play) {
                c.paused = v[i % v.len()];
            }
            if let Value::Easings(v) = self.get(Property::Easing) {
                c.easing = v[i % v.len()].clone();
            }
            if let Value::Compositions(v) = self.get(Property::Composition) {
                c.composition = v[i % v.len()];
            }
            if let Value::Timelines(v) = self.get(Property::Timeline) {
                c.timeline = v[i % v.len()].clone();
            }
            if let Value::Ranges(v) = self.get(Property::RangeStart) {
                c.range[0] = v[i % v.len()].clone();
            }
            if let Value::Ranges(v) = self.get(Property::RangeEnd) {
                c.range[1] = v[i % v.len()].clone();
            }
            c.timing.duration = c.duration.unwrap_or(if c.timeline == Timeline::Auto {
                0.0
            } else {
                1.0
            });
            c.timing.delay = c.delay;
            c.timing.end_delay = time(Property::EndDelay).unwrap_or(0.0);
            result.push(c);
        }
        result
    }
}

fn initial(p: Property) -> Value {
    use Property::*;
    match p {
        Name => Value::AnimationNames(vec![AnimationName::None]),
        ScrollName | ViewName => Value::Names(vec!["none".into()]),
        TransitionProperty => Value::Names(vec!["all".into()]),
        Duration => Value::Times(vec![None]),
        Delay | EndDelay | TransitionDuration | TransitionDelay => Value::Times(vec![Some(0.0)]),
        Iterations => Value::Counts(vec![1.0]),
        Direction => Value::Directions(vec![self::Direction::Normal]),
        Fill => Value::Fills(vec![self::Fill::None]),
        Play => Value::Plays(vec![false]),
        Easing | TransitionEasing => Value::Easings(vec![self::Easing::default()]),
        Composition => Value::Compositions(vec![self::Composition::Replace]),
        Timeline => Value::Timelines(vec![self::Timeline::Auto]),
        RangeStart | RangeEnd => Value::Ranges(vec![RangeOffset {
            name: RangeName::Normal,
            offset: Length::Percent(if p == RangeStart { 0.0 } else { 100.0 }),
        }]),
        ScrollAxis | ViewAxis => Value::Axes(vec![Axis::Block]),
        ViewInset => Value::Insets(vec![[None, None]]),
    }
}

fn lists(tokens: &[Component]) -> Option<Vec<&[Component]>> {
    let parts: Vec<_> = tokens
        .split(|c| matches!(c, Component::Token(TokenKind::Comma)))
        .collect();
    (parts.len() <= MAX_LIST && parts.iter().all(|p| !p.is_empty())).then_some(parts)
}
fn ident(c: &Component) -> Option<&str> {
    c.ident()
}
fn name(c: &Component) -> Option<String> {
    if let Component::Token(TokenKind::String(s)) = c {
        return (s.len() <= 4096).then(|| s.clone());
    }
    let value = ident(c)?;
    (value.len() <= 4096
        && ![
            "initial",
            "inherit",
            "unset",
            "revert",
            "revert-layer",
            "default",
        ]
        .iter()
        .any(|s| value.eq_ignore_ascii_case(s)))
    .then(|| value.to_owned())
}
fn time(c: &Component) -> Option<f64> {
    let Component::Token(TokenKind::Dimension { number: n, unit: u }) = c else {
        return None;
    };
    let n = n.value().ok()?;
    let n = match u.to_ascii_lowercase().as_str() {
        "ms" => n / 1000.0,
        "s" => n,
        _ => return None,
    };
    (n.is_finite() && n.abs() <= MAX_TIME).then_some(n)
}
fn axis(c: &Component) -> Option<Axis> {
    Some(match ident(c)?.to_ascii_lowercase().as_str() {
        "block" => Axis::Block,
        "inline" => Axis::Inline,
        "x" => Axis::X,
        "y" => Axis::Y,
        _ => return None,
    })
}
fn inset(tokens: &[Component]) -> Option<[Option<Length>; 2]> {
    let one = |c: &Component| {
        if c.is_ident("auto") {
            Some(None)
        } else {
            c.length().map(Some)
        }
    };
    match tokens {
        [a] => Some([one(a)?, one(a)?]),
        [a, b] => Some([one(a)?, one(b)?]),
        _ => None,
    }
}
fn easing(c: &Component) -> Option<Easing> {
    if let Some(s) = ident(c) {
        return Some(match s.to_ascii_lowercase().as_str() {
            "linear" => Easing::Linear,
            "ease" => Easing::default(),
            "ease-in" => Easing::Bezier([0.42, 0.0, 1.0, 1.0]),
            "ease-out" => Easing::Bezier([0.0, 0.0, 0.58, 1.0]),
            "ease-in-out" => Easing::Bezier([0.42, 0.0, 0.58, 1.0]),
            "step-start" => Easing::Steps(1, Jump::Start),
            "step-end" => Easing::Steps(1, Jump::End),
            _ => return None,
        });
    }
    let Component::Function(name, args) = c else {
        return None;
    };
    let args = components::from_tokens(args)?;
    let parts = lists(&args)?;
    if name.eq_ignore_ascii_case("cubic-bezier") && parts.len() == 4 {
        let mut v = [0.0; 4];
        for (i, p) in parts.iter().enumerate() {
            let [c] = *p else { return None };
            v[i] = f64::from(c.number()?);
        }
        return ((0.0..=1.0).contains(&v[0]) && (0.0..=1.0).contains(&v[2]))
            .then_some(Easing::Bezier(v));
    }
    if name.eq_ignore_ascii_case("steps") && (1..=2).contains(&parts.len()) {
        let [n] = parts[0] else { return None };
        let n = n.number()?;
        if n.fract() != 0.0 || !(1.0..=1_000_000.0).contains(&n) {
            return None;
        }
        let j = if parts.len() == 1 {
            Jump::End
        } else {
            let [c] = parts[1] else { return None };
            match ident(c)?.to_ascii_lowercase().as_str() {
                "start" | "jump-start" => Jump::Start,
                "end" | "jump-end" => Jump::End,
                "jump-none" => Jump::None,
                "jump-both" => Jump::Both,
                _ => return None,
            }
        };
        return (n > 1.0 || j != Jump::None).then_some(Easing::Steps(n as u32, j));
    }
    None // linear() is deliberately diagnosed until its stop grammar is implemented.
}
fn timeline(c: &Component) -> Option<Timeline> {
    if c.is_ident("auto") {
        return Some(Timeline::Auto);
    }
    if c.is_ident("none") {
        return Some(Timeline::None);
    }
    if let Some(s) = ident(c) {
        return s.starts_with("--").then(|| Timeline::Named(s.into()));
    }
    let Component::Function(n, args) = c else {
        return None;
    };
    let args = components::from_tokens(args)?;
    if n.eq_ignore_ascii_case("scroll") {
        let (mut a, mut s) = (None, None);
        for c in &args {
            if let Some(v) = axis(c) {
                if a.replace(v).is_some() {
                    return None;
                }
            } else {
                let v = match ident(c)?.to_ascii_lowercase().as_str() {
                    "nearest" => Scroller::Nearest,
                    "root" => Scroller::Root,
                    "self" => Scroller::Self_,
                    _ => return None,
                };
                if s.replace(v).is_some() {
                    return None;
                }
            }
        }
        Some(Timeline::Scroll(
            s.unwrap_or(Scroller::Nearest),
            a.unwrap_or(Axis::Block),
        ))
    } else if n.eq_ignore_ascii_case("view") {
        let mut a = None;
        let mut rest = Vec::new();
        for c in args {
            if let Some(v) = axis(&c) {
                if a.replace(v).is_some() {
                    return None;
                }
            } else {
                rest.push(c)
            }
        }
        Some(Timeline::View(
            a.unwrap_or(Axis::Block),
            if rest.is_empty() {
                [None, None]
            } else {
                inset(&rest)?
            },
        ))
    } else {
        None
    }
}
fn range(tokens: &[Component], end: bool) -> Option<RangeOffset> {
    if tokens.len() == 1 && tokens[0].is_ident("normal") {
        return Some(RangeOffset {
            name: RangeName::Normal,
            offset: Length::Percent(if end { 100.0 } else { 0.0 }),
        });
    }
    let parse_name = |c: &Component| {
        Some(match ident(c)?.to_ascii_lowercase().as_str() {
            "cover" => RangeName::Cover,
            "contain" => RangeName::Contain,
            "entry" => RangeName::Entry,
            "exit" => RangeName::Exit,
            "entry-crossing" => RangeName::EntryCrossing,
            "exit-crossing" => RangeName::ExitCrossing,
            _ => return None,
        })
    };
    match tokens {
        [c] if c.length().is_some() => Some(RangeOffset {
            name: RangeName::Normal,
            offset: c.length()?,
        }),
        [c] => Some(RangeOffset {
            name: parse_name(c)?,
            offset: Length::Percent(if end { 100.0 } else { 0.0 }),
        }),
        [a, b] => Some(RangeOffset {
            name: parse_name(a)?,
            offset: b.length()?,
        }),
        _ => None,
    }
}

pub(crate) fn parse(p: Property, tokens: &[Component]) -> Option<Value> {
    use Property::*;
    let parts = lists(tokens)?;
    let single = || {
        parts
            .iter()
            .map(|p| match *p {
                [c] => Some(c),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
    };
    Some(match p {
        Name => Value::AnimationNames(
            single()?
                .into_iter()
                .map(|c| {
                    if c.is_ident("none") {
                        Some(AnimationName::None)
                    } else {
                        name(c).map(AnimationName::Name)
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        TransitionProperty | ScrollName | ViewName => {
            if single()?.iter().any(|c| c.ident().is_none()) {
                return None;
            }
            let v = single()?
                .into_iter()
                .map(name)
                .collect::<Option<Vec<_>>>()?;
            if matches!(p, ScrollName | ViewName)
                && v.iter().any(|s| s != "none" && !s.starts_with("--"))
            {
                return None;
            }
            if p == TransitionProperty
                && v.len() > 1
                && v.iter().any(|s| s.eq_ignore_ascii_case("none"))
            {
                return None;
            }
            Value::Names(v)
        }
        Duration | Delay | EndDelay | TransitionDuration | TransitionDelay => Value::Times(
            single()?
                .into_iter()
                .map(|c| {
                    if p == Duration && c.is_ident("auto") {
                        Some(None)
                    } else {
                        let n = time(c)?;
                        (!(matches!(p, Duration | EndDelay | TransitionDuration) && n < 0.0))
                            .then_some(Some(n))
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Iterations => Value::Counts(
            single()?
                .into_iter()
                .map(|c| {
                    if c.is_ident("infinite") {
                        Some(f64::INFINITY)
                    } else {
                        let n = f64::from(c.number()?);
                        (0.0..=MAX_TIME).contains(&n).then_some(n)
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Direction => Value::Directions(
            single()?
                .into_iter()
                .map(|c| {
                    Some(match ident(c)?.to_ascii_lowercase().as_str() {
                        "normal" => self::Direction::Normal,
                        "reverse" => self::Direction::Reverse,
                        "alternate" => self::Direction::Alternate,
                        "alternate-reverse" => self::Direction::AlternateReverse,
                        _ => return None,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Fill => Value::Fills(
            single()?
                .into_iter()
                .map(|c| {
                    Some(match ident(c)?.to_ascii_lowercase().as_str() {
                        "none" => self::Fill::None,
                        "forwards" => self::Fill::Forwards,
                        "backwards" => self::Fill::Backwards,
                        "both" => self::Fill::Both,
                        _ => return None,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Play => Value::Plays(
            single()?
                .into_iter()
                .map(|c| match ident(c)?.to_ascii_lowercase().as_str() {
                    "running" => Some(false),
                    "paused" => Some(true),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Easing | TransitionEasing => Value::Easings(
            single()?
                .into_iter()
                .map(easing)
                .collect::<Option<Vec<_>>>()?,
        ),
        Composition => Value::Compositions(
            single()?
                .into_iter()
                .map(|c| {
                    Some(match ident(c)?.to_ascii_lowercase().as_str() {
                        "replace" => self::Composition::Replace,
                        "add" => self::Composition::Add,
                        "accumulate" => self::Composition::Accumulate,
                        _ => return None,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Timeline => Value::Timelines(
            single()?
                .into_iter()
                .map(timeline)
                .collect::<Option<Vec<_>>>()?,
        ),
        RangeStart | RangeEnd => Value::Ranges(
            parts
                .into_iter()
                .map(|v| range(v, p == RangeEnd))
                .collect::<Option<Vec<_>>>()?,
        ),
        ScrollAxis | ViewAxis => Value::Axes(
            single()?
                .into_iter()
                .map(axis)
                .collect::<Option<Vec<_>>>()?,
        ),
        ViewInset => Value::Insets(parts.into_iter().map(inset).collect::<Option<Vec<_>>>()?),
    })
}

pub(super) fn shorthand(
    name: &str,
    properties: &[super::declarations::Property],
    tokens: &[Component],
) -> Option<Vec<super::declarations::Value>> {
    use super::declarations::{Property as P, Value as V};
    let parts = lists(tokens)?;
    let mut columns: Vec<Vec<Value>> = vec![Vec::new(); properties.len()];
    for part in parts {
        let mut row: Vec<_> = properties
            .iter()
            .map(|p| {
                if let P::Animation(p) = p {
                    Some(initial(*p))
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>()?;
        let mut seen = vec![false; properties.len()];
        if name == "animation-delay" {
            if part.is_empty() || part.len() > 2 {
                return None;
            }
            row[0] = parse(Property::Delay, &part[..1])?;
            if part.len() == 2 {
                row[1] = parse(Property::EndDelay, &part[1..])?;
            }
        } else if name == "animation-range" {
            let mut matched = false;
            for split in (1..=part.len()).rev() {
                if let Some(a) = range(&part[..split], false) {
                    let b = if split == part.len() {
                        RangeOffset {
                            name: a.name,
                            offset: Length::Percent(100.0),
                        }
                    } else {
                        let Some(b) = range(&part[split..], true) else {
                            continue;
                        };
                        b
                    };
                    row = vec![Value::Ranges(vec![a]), Value::Ranges(vec![b])];
                    matched = true;
                    break;
                }
            }
            if !matched {
                return None;
            }
        } else {
            // Ambiguous identifiers first consume timing/direction/fill slots;
            // only a second occurrence can be the animation/property name.
            for c in part {
                if matches!(name, "animation" | "transition") && time(c).is_some() {
                    let i = properties
                        .iter()
                        .enumerate()
                        .find(|(i, p)| {
                            !seen[*i]
                                && matches!(
                                    p,
                                    P::Animation(
                                        Property::Duration
                                            | Property::Delay
                                            | Property::EndDelay
                                            | Property::TransitionDuration
                                            | Property::TransitionDelay
                                    )
                                )
                        })?
                        .0;
                    let P::Animation(p) = properties[i] else {
                        return None;
                    };
                    row[i] = parse(p, std::slice::from_ref(c))?;
                    seen[i] = true;
                    continue;
                }
                let mut matched = false;
                for (i, p) in properties.iter().enumerate() {
                    let P::Animation(p) = p else { return None };
                    if seen[i]
                        || matches!(
                            p,
                            Property::Name
                                | Property::TransitionProperty
                                | Property::ScrollName
                                | Property::ViewName
                                | Property::Timeline
                                | Property::RangeStart
                                | Property::RangeEnd
                        )
                    {
                        continue;
                    }
                    if let Some(v) = parse(*p, std::slice::from_ref(c)) {
                        row[i] = v;
                        seen[i] = true;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    let i = properties.iter().position(|p| {
                        matches!(
                            p,
                            P::Animation(
                                Property::Name
                                    | Property::TransitionProperty
                                    | Property::ScrollName
                                    | Property::ViewName
                            )
                        )
                    })?;
                    let P::Animation(p) = properties[i] else {
                        return None;
                    };
                    if seen[i] {
                        return None;
                    }
                    row[i] = parse(p, std::slice::from_ref(c))?;
                    seen[i] = true;
                }
            }
        }
        for (i, v) in row.into_iter().enumerate() {
            columns[i].push(v)
        }
    }
    columns
        .into_iter()
        .map(|v| {
            let first = v.first()?.clone();
            macro_rules! merge {
                ($variant:ident) => {{
                    let mut out = Vec::new();
                    for v in v {
                        let Value::$variant(mut a) = v else {
                            return None;
                        };
                        out.append(&mut a)
                    }
                    Value::$variant(out)
                }};
            }
            let v = match first {
                Value::AnimationNames(_) => merge!(AnimationNames),
                Value::Names(_) => merge!(Names),
                Value::Times(_) => merge!(Times),
                Value::Counts(_) => merge!(Counts),
                Value::Directions(_) => merge!(Directions),
                Value::Fills(_) => merge!(Fills),
                Value::Plays(_) => merge!(Plays),
                Value::Easings(_) => merge!(Easings),
                Value::Compositions(_) => merge!(Compositions),
                Value::Timelines(_) => merge!(Timelines),
                Value::Ranges(_) => merge!(Ranges),
                Value::Axes(_) => merge!(Axes),
                Value::Insets(_) => merge!(Insets),
            };
            Some(V::Animation(v))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    pub duration: f64,
    pub delay: f64,
    pub end_delay: f64,
    pub iterations: f64,
    pub direction: Direction,
    pub fill: Fill,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            duration: 0.0,
            delay: 0.0,
            end_delay: 0.0,
            iterations: 1.0,
            direction: Direction::Normal,
            fill: Fill::None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Before,
    Active,
    After,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedTiming {
    pub phase: Phase,
    pub progress: Option<f64>,
    pub iteration: f64,
    pub active_duration: f64,
    pub end_time: f64,
    pub before: bool,
    pub active_time: f64,
    pub duration: f64,
    pub delay: f64,
}
impl Timing {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_time(self.duration)
            || self.duration < 0.0
            || !valid_time(self.delay)
            || !valid_time(self.end_delay)
            || self.end_delay < 0.0
            || !(self.iterations == f64::INFINITY || (0.0..=MAX_TIME).contains(&self.iterations))
        {
            return Err("invalid or out-of-range effect timing");
        }
        Ok(())
    }
    pub fn active_duration(&self) -> f64 {
        if self.duration == 0.0 || self.iterations == 0.0 {
            0.0
        } else {
            self.duration * self.iterations
        }
    }
    pub fn end_time(&self) -> f64 {
        (self.delay + self.active_duration() + self.end_delay).max(0.0)
    }
    pub fn sample(&self, local: f64) -> ComputedTiming {
        self.sample_direction(local, false)
    }
    fn sample_direction(&self, local: f64, backwards: bool) -> ComputedTiming {
        let active = self.active_duration();
        let before = self.delay.min(self.end_time());
        let after = (self.delay + active).min(self.end_time());
        let phase = if local < before || backwards && local == before {
            Phase::Before
        } else if local > after || !backwards && local == after {
            Phase::After
        } else {
            Phase::Active
        };
        let contributes = match phase {
            Phase::Before => matches!(self.fill, Fill::Backwards | Fill::Both),
            Phase::After => matches!(self.fill, Fill::Forwards | Fill::Both),
            Phase::Active => true,
        };
        let elapsed = match phase {
            Phase::Before => 0.0,
            Phase::Active => (local - self.delay).max(0.0),
            Phase::After => active,
        };
        let overall = if self.duration == 0.0 {
            if phase == Phase::Before || !self.iterations.is_finite() {
                0.0
            } else {
                self.iterations
            }
        } else {
            elapsed / self.duration
        };
        let mut iteration = overall.floor();
        let mut progress = overall.fract();
        if elapsed == active && active.is_finite() && progress == 0.0 && self.iterations > 0.0 {
            progress = 1.0;
            iteration = (iteration - 1.0).max(0.0);
        }
        let reverse = match self.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => iteration.rem_euclid(2.0) == 1.0,
            Direction::AlternateReverse => iteration.rem_euclid(2.0) == 0.0,
        };
        if reverse {
            progress = 1.0 - progress
        }
        ComputedTiming {
            phase,
            progress: contributes.then_some(progress),
            iteration,
            active_duration: active,
            end_time: self.end_time(),
            before: phase == Phase::Before && !reverse || phase == Phase::After && reverse,
            active_time: elapsed,
            duration: self.duration,
            delay: self.delay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Idle,
    Running,
    Paused,
    Finished,
}
#[derive(Clone, Debug)]
pub struct Animation {
    timing: Timing,
    start_time: Option<f64>,
    hold_time: Option<f64>,
    playback_rate: f64,
    cancelled: bool,
    paused: bool,
}
impl Animation {
    pub fn new(timing: Timing, now: f64) -> Result<Self, &'static str> {
        timing.validate()?;
        if !valid_time(now) {
            return Err("invalid animation start time");
        }
        Ok(Self {
            timing,
            start_time: Some(now),
            hold_time: None,
            playback_rate: 1.0,
            cancelled: false,
            paused: false,
        })
    }
    pub fn timing(&self) -> &Timing {
        &self.timing
    }
    pub fn update_timing(&mut self, timing: Timing) -> Result<(), &'static str> {
        timing.validate()?;
        self.timing = timing;
        Ok(())
    }
    pub fn start_time(&self) -> Option<f64> {
        self.start_time
    }
    pub fn playback_rate(&self) -> f64 {
        self.playback_rate
    }
    pub fn current_time(&self, now: f64) -> Option<f64> {
        if self.cancelled || !valid_time(now) {
            None
        } else {
            self.hold_time
                .or_else(|| self.start_time.map(|s| (now - s) * self.playback_rate))
                .filter(|t| t.is_finite())
        }
    }
    pub fn play_state(&self, now: f64) -> PlayState {
        if self.cancelled {
            return PlayState::Idle;
        }
        if self.paused {
            return PlayState::Paused;
        }
        let t = self.current_time(now).unwrap_or(0.0);
        if self.playback_rate > 0.0 && t >= self.timing.end_time()
            || self.playback_rate < 0.0 && t <= 0.0
        {
            PlayState::Finished
        } else {
            PlayState::Running
        }
    }
    pub fn pause(&mut self, now: f64) -> Result<(), &'static str> {
        if !valid_time(now) {
            return Err("invalid animation pause time");
        }
        self.hold_time = Some(self.current_time(now).unwrap_or(0.0));
        self.cancelled = false;
        self.paused = true;
        self.start_time = None;
        Ok(())
    }
    pub fn play(&mut self, now: f64) -> Result<(), &'static str> {
        if !valid_time(now) || self.playback_rate < 0.0 && !self.timing.end_time().is_finite() {
            return Err("invalid play time or unbounded reverse start");
        }
        let mut t = self.current_time(now).unwrap_or(0.0);
        if self.cancelled || self.play_state(now) == PlayState::Finished {
            t = if self.playback_rate < 0.0 {
                self.timing.end_time()
            } else {
                0.0
            };
        }
        let start = if self.playback_rate == 0.0 {
            None
        } else {
            Some(now - t / self.playback_rate)
        };
        if start.is_some_and(|s| !s.is_finite()) {
            return Err("unrepresentable animation start time");
        }
        self.cancelled = false;
        self.paused = false;
        if self.playback_rate == 0.0 {
            self.hold_time = Some(t);
            self.start_time = None;
        } else {
            self.start_time = start;
            self.hold_time = None;
        }
        Ok(())
    }
    pub fn seek(&mut self, time: f64, now: f64) -> Result<(), &'static str> {
        if !valid_time(time) || !valid_time(now) {
            return Err("nonfinite/out-of-range animation time");
        }
        let start = if self.hold_time.is_some() || self.playback_rate == 0.0 {
            None
        } else {
            Some(now - time / self.playback_rate)
        };
        if start.is_some_and(|s| !s.is_finite()) {
            return Err("unrepresentable animation start time");
        }
        self.cancelled = false;
        if self.hold_time.is_some() || self.playback_rate == 0.0 {
            self.hold_time = Some(time)
        } else {
            self.start_time = start
        }
        Ok(())
    }
    pub fn set_playback_rate(&mut self, rate: f64, now: f64) -> Result<(), &'static str> {
        if !valid_time(now) || !rate.is_finite() || rate.abs() > 1_000_000.0 {
            return Err("invalid playback rate");
        }
        let t = self.current_time(now).unwrap_or(0.0);
        let paused = self.paused;
        let start = if rate == 0.0 || paused {
            None
        } else {
            Some(now - t / rate)
        };
        if start.is_some_and(|s| !s.is_finite()) {
            return Err("unrepresentable playback rate/start time");
        }
        self.playback_rate = rate;
        if rate == 0.0 || paused {
            self.hold_time = Some(t);
            self.start_time = None;
        } else {
            self.hold_time = None;
            self.start_time = start;
        }
        Ok(())
    }
    pub fn finish(&mut self, now: f64) -> Result<(), &'static str> {
        if !valid_time(now) || self.playback_rate == 0.0 || !self.timing.end_time().is_finite() {
            return Err("cannot finish a zero-rate or infinite animation");
        }
        let time = if self.playback_rate < 0.0 {
            0.0
        } else {
            self.timing.end_time()
        };
        let start = now - time / self.playback_rate;
        if !start.is_finite() {
            return Err("unrepresentable finished animation start time");
        }
        self.hold_time = None;
        self.cancelled = false;
        self.paused = false;
        self.start_time = Some(start);
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.start_time = None;
        self.hold_time = None;
        self.paused = false;
    }
    pub fn computed_timing(&self, now: f64) -> Option<ComputedTiming> {
        self.current_time(now)
            .map(|t| self.timing.sample_direction(t, self.playback_rate < 0.0))
    }
}
pub fn valid_time(t: f64) -> bool {
    t.is_finite() && t.abs() <= MAX_TIME
}

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub name: String,
    pub enabled: bool,
    pub duration: Option<f64>,
    pub delay: f64,
    pub timing: Timing,
    pub paused: bool,
    pub easing: Easing,
    pub composition: Composition,
    pub timeline: Timeline,
    pub range: [RangeOffset; 2],
}
impl Default for Config {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: false,
            duration: None,
            delay: 0.0,
            timing: Timing::default(),
            paused: false,
            easing: Easing::default(),
            composition: Composition::Replace,
            timeline: Timeline::Auto,
            range: [
                RangeOffset {
                    name: RangeName::Normal,
                    offset: Length::Percent(0.0),
                },
                RangeOffset {
                    name: RangeName::Normal,
                    offset: Length::Percent(100.0),
                },
            ],
        }
    }
}

pub(super) mod effects;
pub(crate) mod timelines;
pub(super) use effects::Keyframes;
pub use effects::{AnimationEvent, EffectId, Engine, EventKind};

#[cfg(test)]
mod tests;
