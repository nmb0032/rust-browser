//! Typed fragmentation values. Fragmentainer-specific break selection belongs
//! to layout; a page break is never interpreted as a column break.
use std::collections::BTreeMap;

use crate::css::values::{
    BorderStyle, Color, Length, LengthContext, ResolvedColor, components::Component,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Property {
    Count,
    Width,
    Fill,
    Span,
    RuleWidth,
    RuleStyle,
    RuleColor,
    Before,
    After,
    Inside,
    Widows,
    Orphans,
    Decoration,
    Page,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Legacy {
    Before,
    After,
    Inside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Break {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    AvoidColumn,
    Column,
    Always,
    Page,
    Left,
    Right,
    Recto,
    Verso,
}
impl Break {
    pub fn forced(self, page: bool) -> bool {
        if page {
            matches!(
                self,
                Self::Always | Self::Page | Self::Left | Self::Right | Self::Recto | Self::Verso
            )
        } else {
            matches!(self, Self::Column | Self::Always)
        }
    }
    pub fn avoids(self, page: bool) -> bool {
        self == Self::Avoid
            || self
                == if page {
                    Self::AvoidPage
                } else {
                    Self::AvoidColumn
                }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Integer(Option<u16>),
    Length(Option<Length>),
    Color(Color),
    Style(BorderStyle),
    Break(Break),
    Flag(bool),
    Page(Option<String>),
}

#[derive(Clone, Debug, Default)]
pub struct Fragmentation(BTreeMap<Property, Value>);
impl PartialEq for Fragmentation {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .keys()
            .chain(other.0.keys())
            .all(|&p| self.get(p) == other.get(p))
    }
}
impl Fragmentation {
    pub(crate) fn get(&self, p: Property) -> Value {
        self.0.get(&p).cloned().unwrap_or_else(|| initial(p))
    }
    pub(crate) fn set(&mut self, p: Property, v: Value) {
        self.0.insert(p, v);
    }
    pub fn count(&self) -> Option<u16> {
        let Value::Integer(v) = self.get(Property::Count) else {
            unreachable!()
        };
        v
    }
    pub fn width(&self) -> Option<Length> {
        let Value::Length(v) = self.get(Property::Width) else {
            unreachable!()
        };
        v
    }
    pub fn columns(&self) -> bool {
        self.count().is_some() || self.width().is_some()
    }
    pub fn balance(&self) -> bool {
        self.flag(Property::Fill)
    }
    pub fn span(&self) -> bool {
        self.flag(Property::Span)
    }
    pub fn clone_decoration(&self) -> bool {
        self.flag(Property::Decoration)
    }
    fn flag(&self, p: Property) -> bool {
        let Value::Flag(v) = self.get(p) else {
            unreachable!()
        };
        v
    }
    pub fn before(&self) -> Break {
        self.break_value(Property::Before)
    }
    pub fn after(&self) -> Break {
        self.break_value(Property::After)
    }
    pub fn inside(&self) -> Break {
        self.break_value(Property::Inside)
    }
    fn break_value(&self, p: Property) -> Break {
        let Value::Break(v) = self.get(p) else {
            unreachable!()
        };
        v
    }
    pub fn widows(&self) -> usize {
        self.integer(Property::Widows)
    }
    pub fn orphans(&self) -> usize {
        self.integer(Property::Orphans)
    }
    fn integer(&self, p: Property) -> usize {
        let Value::Integer(v) = self.get(p) else {
            unreachable!()
        };
        usize::from(v.unwrap_or(2))
    }
    pub fn page(&self) -> Option<String> {
        let Value::Page(v) = self.get(Property::Page) else {
            unreachable!()
        };
        v
    }
    pub fn rule(&self, color: ResolvedColor) -> (f32, BorderStyle, ResolvedColor) {
        let Value::Length(v) = self.get(Property::RuleWidth) else {
            unreachable!()
        };
        let Value::Style(style) = self.get(Property::RuleStyle) else {
            unreachable!()
        };
        let Value::Color(c) = self.get(Property::RuleColor) else {
            unreachable!()
        };
        (
            v.and_then(|v| v.used(&LengthContext::default(), 0.0).map(|v| v.0))
                .unwrap_or(3.0),
            style,
            c.resolve(color).unwrap_or(color),
        )
    }
    pub(crate) fn inherit(&self) -> Self {
        let mut result = Self::default();
        for p in [Property::Widows, Property::Orphans] {
            result.set(p, self.get(p));
        }
        result
    }
}
pub(crate) fn initial(p: Property) -> Value {
    match p {
        Property::Count => Value::Integer(None),
        Property::Width => Value::Length(None),
        Property::RuleWidth => Value::Length(Some(Length::Px(3.0))),
        Property::RuleColor => Value::Color(Color::CurrentColor),
        Property::RuleStyle => Value::Style(BorderStyle::None),
        Property::Before | Property::After | Property::Inside => Value::Break(Break::Auto),
        Property::Widows | Property::Orphans => Value::Integer(Some(2)),
        Property::Fill => Value::Flag(true),
        Property::Span | Property::Decoration => Value::Flag(false),
        Property::Page => Value::Page(None),
    }
}
pub(crate) fn parse(p: Property, t: &[Component]) -> Option<Value> {
    let [a] = t else { return None };
    let word = a.ident().unwrap_or("").to_ascii_lowercase();
    Some(match p {
        Property::Count | Property::Widows | Property::Orphans => {
            if p == Property::Count && word == "auto" {
                Value::Integer(None)
            } else {
                let n = a.number()?;
                if n.fract() != 0.0 || !(1.0..=256.0).contains(&n) {
                    return None;
                }
                Value::Integer(Some(n as u16))
            }
        }
        Property::Width | Property::RuleWidth => {
            if p == Property::Width && word == "auto" {
                Value::Length(None)
            } else if p == Property::RuleWidth
                && matches!(word.as_str(), "thin" | "medium" | "thick")
            {
                Value::Length(Some(Length::Px(match word.as_str() {
                    "thin" => 1.0,
                    "thick" => 5.0,
                    _ => 3.0,
                })))
            } else {
                let length = a.length()?;
                if length.has_percentage()
                    || !length.is_nonnegative()
                    || p == Property::Width && length == Length::Px(0.0)
                {
                    return None;
                }
                Value::Length(Some(length))
            }
        }
        Property::RuleColor => Value::Color(a.color()?),
        Property::RuleStyle => Value::Style(match word.as_str() {
            "none" => BorderStyle::None,
            "hidden" => BorderStyle::Hidden,
            "solid" => BorderStyle::Solid,
            "dashed" => BorderStyle::Dashed,
            "dotted" => BorderStyle::Dotted,
            "double" => BorderStyle::Double,
            _ => return None,
        }),
        Property::Fill => Value::Flag(match word.as_str() {
            "balance" => true,
            "auto" => false,
            _ => return None,
        }),
        Property::Span => Value::Flag(match word.as_str() {
            "all" => true,
            "none" => false,
            _ => return None,
        }),
        Property::Decoration => Value::Flag(match word.as_str() {
            "clone" => true,
            "slice" => false,
            _ => return None,
        }),
        Property::Page => {
            if word == "auto" {
                Value::Page(None)
            } else if matches!(
                word.as_str(),
                "none" | "default" | "initial" | "inherit" | "unset" | "revert" | "revert-layer"
            ) {
                return None;
            } else {
                Value::Page(Some(a.ident()?.into()))
            }
        }
        Property::Before | Property::After | Property::Inside => {
            let value = match word.as_str() {
                "auto" => Break::Auto,
                "avoid" => Break::Avoid,
                "avoid-page" => Break::AvoidPage,
                "avoid-column" => Break::AvoidColumn,
                "column" => Break::Column,
                "page" => Break::Page,
                "always" => Break::Always,
                "left" => Break::Left,
                "right" => Break::Right,
                "recto" => Break::Recto,
                "verso" => Break::Verso,
                _ => return None,
            };
            if p == Property::Inside
                && !matches!(
                    value,
                    Break::Auto | Break::Avoid | Break::AvoidPage | Break::AvoidColumn
                )
            {
                return None;
            }
            Value::Break(value)
        }
    })
}
pub(crate) fn compute(v: Value, context: &LengthContext) -> Option<Value> {
    Some(match v {
        Value::Length(Some(length)) => Value::Length(Some(length.computed(context)?.0)),
        v => v,
    })
}
pub(crate) fn compute_colors(v: Value, current: ResolvedColor) -> Option<Value> {
    Some(match v {
        Value::Color(c) => Value::Color(c.computed(current)?),
        v => v,
    })
}
