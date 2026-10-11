use std::fmt;

use super::{Length, LengthContext, UsedLength};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSizeKeyword {
    XxSmall,
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
    XxLarge,
    XxxLarge,
}

impl FontSizeKeyword {
    pub(crate) fn resolve(self, initial_font_size: f32) -> f32 {
        let at_sixteen = match self {
            Self::XxSmall => 9.0,
            Self::XSmall => 10.0,
            Self::Small => 13.0,
            Self::Medium => 16.0,
            Self::Large => 18.0,
            Self::XLarge => 24.0,
            Self::XxLarge => 32.0,
            Self::XxxLarge => 48.0,
        };
        initial_font_size * at_sixteen / 16.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Hidden,
    Solid,
    Dashed,
    Dotted,
    Double,
}

impl BorderStyle {
    pub fn visible(self) -> bool {
        !matches!(self, Self::None | Self::Hidden)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackgroundImage {
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontWeight(pub f32);

impl Default for FontWeight {
    fn default() -> Self {
        Self(400.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum LineHeight {
    #[default]
    Normal,
    Number(f32),
    Length(Length),
}

impl LineHeight {
    pub fn used_value(&self, font_size: f32) -> Option<UsedLength> {
        let value = match self {
            Self::Normal => font_size * 1.25,
            Self::Number(number) => font_size * number,
            Self::Length(length) => {
                return length.used(
                    &LengthContext {
                        font_size,
                        ..LengthContext::default()
                    },
                    font_size,
                );
            }
        };
        value.is_finite().then_some(UsedLength(value))
    }

    pub fn used(&self, font_size: f32) -> f32 {
        self.used_with_diagnostic(font_size, |message| eprintln!("CSS: {message}"))
    }

    pub(crate) fn used_with_diagnostic(&self, font_size: f32, report: impl FnOnce(String)) -> f32 {
        match self.used_value(font_size) {
            Some(value) => value.0,
            None => {
                report(format!(
                    "invalid at used-value time: non-finite CSS line-height result; \
                     value={self:?}, font-size={font_size}px; compatibility fallback is 0px"
                ));
                0.0
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FamilyName {
    Generic(String),
    Named(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFamily(pub Vec<FamilyName>);

impl Default for FontFamily {
    fn default() -> Self {
        Self(vec![FamilyName::Generic("sans-serif".into())])
    }
}

impl fmt::Display for FontFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, name) in self.0.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            match name {
                FamilyName::Generic(name) => f.write_str(name)?,
                FamilyName::Named(name) => {
                    write!(f, "\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))?
                }
            }
        }
        Ok(())
    }
}
