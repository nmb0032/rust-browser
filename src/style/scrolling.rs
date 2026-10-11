//! Finite Scroll Snap, Overscroll and Scrollbars values shared by input/paint.
use std::collections::BTreeMap;

use crate::css::values::{
    Color, Length, LengthContext, ResolvedColor, Side, components::Component,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Property {
    Behavior,
    SnapType,
    SnapAlign,
    SnapStop,
    Padding(Side),
    Margin(Side),
    OverscrollX,
    OverscrollY,
    Color,
    Width,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Behavior {
    #[default]
    Auto,
    Smooth,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Block,
    Inline,
    Both,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Start,
    Center,
    End,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overscroll {
    #[default]
    Auto,
    Contain,
    None,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Width {
    #[default]
    Auto,
    Thin,
    None,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Behavior(Behavior),
    SnapType(Option<(Axis, bool)>),
    Align(Align, Align),
    Stop(bool),
    Length(Option<Length>),
    Overscroll(Overscroll),
    Color(Option<(Color, Color)>),
    Width(Width),
}
#[derive(Clone, Debug, Default)]
pub struct Scrolling(BTreeMap<Property, Value>);
impl PartialEq for Scrolling {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .keys()
            .chain(other.0.keys())
            .all(|&p| self.get(p) == other.get(p))
    }
}
impl Scrolling {
    pub(crate) fn get(&self, p: Property) -> Value {
        self.0.get(&p).cloned().unwrap_or_else(|| initial(p))
    }
    pub(crate) fn set(&mut self, p: Property, v: Value) {
        self.0.insert(p, v);
    }
    pub fn behavior(&self) -> Behavior {
        let Value::Behavior(v) = self.get(Property::Behavior) else {
            unreachable!()
        };
        v
    }
    pub fn overscroll(&self, vertical: bool) -> Overscroll {
        let Value::Overscroll(v) = self.get(if vertical {
            Property::OverscrollY
        } else {
            Property::OverscrollX
        }) else {
            unreachable!()
        };
        v
    }
    pub fn width(&self) -> Width {
        let Value::Width(v) = self.get(Property::Width) else {
            unreachable!()
        };
        v
    }
    pub fn colors(&self, current: ResolvedColor) -> (ResolvedColor, ResolvedColor) {
        match self.get(Property::Color) {
            Value::Color(Some((thumb, track))) => (
                thumb
                    .resolve(current)
                    .unwrap_or_else(|| ResolvedColor::from_rgb(0x737373)),
                track
                    .resolve(current)
                    .unwrap_or_else(|| ResolvedColor::from_rgb(0xd8d8d8)),
            ),
            _ => (
                ResolvedColor::from_rgb(0x737373),
                ResolvedColor::from_rgb(0xd8d8d8),
            ),
        }
    }
    pub(crate) fn edge(&self, side: Side, padding: bool, basis: f32) -> f32 {
        let Value::Length(value) = self.get(if padding {
            Property::Padding(side)
        } else {
            Property::Margin(side)
        }) else {
            unreachable!()
        };
        value
            .and_then(|v| v.used(&LengthContext::default(), basis).map(|v| v.0))
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    }
    pub(crate) fn inherit(&self) -> Self {
        let mut result = Self::default();
        result.set(Property::Color, self.get(Property::Color));
        result
    }
}
pub(crate) fn initial(p: Property) -> Value {
    match p {
        Property::Behavior => Value::Behavior(Behavior::Auto),
        Property::SnapType => Value::SnapType(None),
        Property::SnapAlign => Value::Align(Align::None, Align::None),
        Property::SnapStop => Value::Stop(false),
        Property::Padding(_) => Value::Length(None),
        Property::Margin(_) => Value::Length(Some(Length::Px(0.0))),
        Property::OverscrollX | Property::OverscrollY => Value::Overscroll(Overscroll::Auto),
        Property::Color => Value::Color(None),
        Property::Width => Value::Width(Width::Auto),
    }
}
pub(crate) fn parse(p: Property, t: &[Component]) -> Option<Value> {
    let word = |t: &Component| t.ident().map(str::to_ascii_lowercase);
    let align = |t: &Component| {
        Some(match word(t)?.as_str() {
            "none" => Align::None,
            "start" => Align::Start,
            "end" => Align::End,
            "center" => Align::Center,
            _ => return None,
        })
    };
    match p {
        Property::SnapType => {
            if matches!(t, [a] if a.is_ident("none")) {
                return Some(Value::SnapType(None));
            }
            let (axis, strict) = match t {
                [a] => (a, None),
                [a, b] => (a, Some(b)),
                _ => return None,
            };
            let axis = match word(axis)?.as_str() {
                "x" => Axis::X,
                "y" => Axis::Y,
                "block" => Axis::Block,
                "inline" => Axis::Inline,
                "both" => Axis::Both,
                _ => return None,
            };
            let mandatory = if let Some(strict) = strict {
                match word(strict)?.as_str() {
                    "mandatory" => true,
                    "proximity" => false,
                    _ => return None,
                }
            } else {
                false
            };
            Some(Value::SnapType(Some((axis, mandatory))))
        }
        Property::SnapAlign => match t {
            [a] => Some(Value::Align(align(a)?, align(a)?)),
            [a, b] => Some(Value::Align(align(a)?, align(b)?)),
            _ => None,
        },
        Property::Color => match t {
            [a] if a.is_ident("auto") => Some(Value::Color(None)),
            [a, b] => Some(Value::Color(Some((a.color()?, b.color()?)))),
            _ => None,
        },
        _ => {
            let [a] = t else { return None };
            Some(match p {
                Property::Behavior => Value::Behavior(match word(a)?.as_str() {
                    "auto" => Behavior::Auto,
                    "smooth" => Behavior::Smooth,
                    _ => return None,
                }),
                Property::SnapStop => Value::Stop(match word(a)?.as_str() {
                    "normal" => false,
                    "always" => true,
                    _ => return None,
                }),
                Property::Padding(_) if a.is_ident("auto") => Value::Length(None),
                Property::Padding(_) | Property::Margin(_) => {
                    let l = a.length()?;
                    if matches!(p, Property::Padding(_)) && !l.is_nonnegative()
                        || matches!(p, Property::Margin(_)) && l.has_percentage()
                    {
                        return None;
                    }
                    Value::Length(Some(l))
                }
                Property::OverscrollX | Property::OverscrollY => {
                    Value::Overscroll(match word(a)?.as_str() {
                        "auto" => Overscroll::Auto,
                        "contain" => Overscroll::Contain,
                        "none" => Overscroll::None,
                        _ => return None,
                    })
                }
                Property::Width => Value::Width(match word(a)?.as_str() {
                    "auto" => Width::Auto,
                    "thin" => Width::Thin,
                    "none" => Width::None,
                    _ => return None,
                }),
                _ => unreachable!(),
            })
        }
    }
}
pub(crate) fn compute(v: Value, context: &LengthContext) -> Option<Value> {
    Some(match v {
        Value::Length(Some(l)) => Value::Length(Some(l.computed(context)?.0)),
        v => v,
    })
}
pub(crate) fn compute_colors(v: Value, current: ResolvedColor) -> Option<Value> {
    Some(match v {
        Value::Color(Some((a, b))) => {
            Value::Color(Some((a.computed(current)?, b.computed(current)?)))
        }
        v => v,
    })
}
