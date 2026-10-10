mod color;

pub use color::{Color, parse_color};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Em(f32),
    Percent(f32),
}

impl Default for Length {
    fn default() -> Self {
        Self::Px(0.0)
    }
}

impl Length {
    pub fn parse(source: &str) -> Option<Self> {
        let source = source.trim().to_ascii_lowercase();
        let (number, unit) = if let Some(number) = source.strip_suffix("px") {
            (number, "px")
        } else if let Some(number) = source.strip_suffix("em") {
            (number, "em")
        } else if let Some(number) = source.strip_suffix('%') {
            (number, "%")
        } else {
            (source.as_str(), "")
        };
        if number.is_empty() || number.chars().any(char::is_whitespace) {
            return None;
        }
        let value: f32 = number.parse().ok()?;
        if !value.is_finite() {
            return None;
        }
        match unit {
            "px" => Some(Self::Px(value)),
            "em" => Some(Self::Em(value)),
            "%" => Some(Self::Percent(value)),
            "" if value == 0.0 => Some(Self::Px(0.0)),
            _ => None,
        }
    }

    pub fn resolve(self, font_size: f32, reference: f32) -> f32 {
        match self {
            Self::Px(value) => value,
            Self::Em(value) => value * font_size,
            Self::Percent(value) => value * reference / 100.0,
        }
    }

    pub(crate) fn compute(self, font_size: f32) -> Self {
        match self {
            Self::Em(value) => Self::Px(value * font_size),
            other => other,
        }
    }

    pub(crate) fn is_nonnegative(self) -> bool {
        match self {
            Self::Px(value) | Self::Em(value) | Self::Percent(value) => value >= 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl<T> Edges<T> {
    pub(crate) fn get(&self, side: Side) -> &T {
        match side {
            Side::Top => &self.top,
            Side::Right => &self.right,
            Side::Bottom => &self.bottom,
            Side::Left => &self.left,
        }
    }

    pub(crate) fn get_mut(&mut self, side: Side) -> &mut T {
        match side {
            Side::Top => &mut self.top,
            Side::Right => &mut self.right,
            Side::Bottom => &mut self.bottom,
            Side::Left => &mut self.left,
        }
    }
}

impl<T: Copy> Edges<T> {
    pub fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn map<U>(self, mut map: impl FnMut(T) -> U) -> Edges<U> {
        Edges {
            top: map(self.top),
            right: map(self.right),
            bottom: map(self.bottom),
            left: map(self.left),
        }
    }
}

pub(crate) fn expand_four<T: Clone>(values: &[T]) -> Option<[T; 4]> {
    let [first, rest @ ..] = values else {
        return None;
    };
    match rest {
        [] => Some([first.clone(), first.clone(), first.clone(), first.clone()]),
        [second] => Some([first.clone(), second.clone(), first.clone(), second.clone()]),
        [second, third] => Some([first.clone(), second.clone(), third.clone(), second.clone()]),
        [second, third, fourth] => {
            Some([first.clone(), second.clone(), third.clone(), fourth.clone()])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_resolves_supported_units() {
        assert_eq!(Length::parse("2EM").unwrap().resolve(16.0, 200.0), 32.0);
        assert_eq!(Length::parse("25%").unwrap().resolve(16.0, 200.0), 50.0);
        assert_eq!(Length::parse("-3px"), Some(Length::Px(-3.0)));
        assert_eq!(Length::parse("0"), Some(Length::Px(0.0)));
        for source in ["3", "NaNpx", "infem", "1e99px", "2 px", "auto", "2rem"] {
            assert!(Length::parse(source).is_none(), "{source}");
        }
    }

    #[test]
    fn expands_shorthand_edges_in_clockwise_order() {
        assert_eq!(expand_four(&[1]), Some([1, 1, 1, 1]));
        assert_eq!(expand_four(&[1, 2]), Some([1, 2, 1, 2]));
        assert_eq!(expand_four(&[1, 2, 3]), Some([1, 2, 3, 2]));
        assert_eq!(expand_four(&[1, 2, 3, 4]), Some([1, 2, 3, 4]));
        assert_eq!(expand_four::<i32>(&[]), None);
        assert_eq!(expand_four(&[1, 2, 3, 4, 5]), None);
    }
}
