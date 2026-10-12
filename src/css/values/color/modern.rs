use std::sync::Arc;

use crate::css::tokenizer::TokenKind;

use super::{Color, keyword};
use crate::css::values::{Dimension, LengthContext, MathExpression, math};

const MAX_DEPTH: usize = 32;
const MAX_TOKENS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
    XyzD65,
    Lab,
    Lch,
    Oklab,
    Oklch,
    Hsl,
    Hwb,
}

impl ColorSpace {
    fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "srgb" => Self::Srgb,
            "srgb-linear" => Self::SrgbLinear,
            "display-p3" => Self::DisplayP3,
            "a98-rgb" => Self::A98Rgb,
            "prophoto-rgb" => Self::ProphotoRgb,
            "rec2020" => Self::Rec2020,
            "xyz" | "xyz-d65" => Self::XyzD65,
            "xyz-d50" => Self::XyzD50,
            "lab" => Self::Lab,
            "lch" => Self::Lch,
            "oklab" => Self::Oklab,
            "oklch" => Self::Oklch,
            "hsl" => Self::Hsl,
            "hwb" => Self::Hwb,
            _ => return None,
        })
    }

    fn hue(self) -> Option<usize> {
        match self {
            Self::Hsl | Self::Hwb => Some(0),
            Self::Lch | Self::Oklch => Some(2),
            _ => None,
        }
    }

    fn categories(self) -> [u8; 3] {
        match self {
            Self::Lab | Self::Oklab => [3, 6, 7],
            Self::Lch | Self::Oklch => [3, 4, 5],
            Self::Hsl => [5, 4, 3],
            // Whiteness and blackness have no analogous components.
            Self::Hwb => [5, 8, 9],
            _ => [0, 1, 2],
        }
    }
}

/// Unpremultiplied, extended-range coordinates. Missing components are retained
/// for interpolation; zero is used only for conversion or final painting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedColor {
    pub space: ColorSpace,
    pub channels: [f64; 3],
    pub alpha: f64,
    /// Bits 0..2 are channels, bit 3 is alpha.
    pub missing: u8,
}

impl ResolvedColor {
    pub const BLACK: Self = Self::srgb(0.0, 0.0, 0.0, 1.0);
    pub const TRANSPARENT: Self = Self::srgb(0.0, 0.0, 0.0, 0.0);

    pub const fn srgb(r: f64, g: f64, b: f64, alpha: f64) -> Self {
        Self {
            space: ColorSpace::Srgb,
            channels: [r, g, b],
            alpha,
            missing: 0,
        }
    }

    pub fn from_rgb(rgb: u32) -> Self {
        Self::srgb(
            f64::from((rgb >> 16) & 255) / 255.0,
            f64::from((rgb >> 8) & 255) / 255.0,
            f64::from(rgb & 255) / 255.0,
            1.0,
        )
    }

    pub fn convert(self, space: ColorSpace) -> Self {
        if self.space == space {
            return self;
        }
        let channels = from_xyz(space, to_xyz(self.space, self.channels));
        let mut missing = self.missing & 8;
        for (index, category) in space.categories().iter().enumerate() {
            if self
                .space
                .categories()
                .iter()
                .enumerate()
                .any(|(i, c)| c == category && self.missing & (1 << i) != 0)
            {
                missing |= 1 << index;
            }
        }
        let mut result = Self {
            space,
            channels,
            alpha: self.alpha,
            missing,
        };
        for i in 0..3 {
            if missing & (1 << i) != 0 {
                result.channels[i] = 0.0;
            }
        }
        result.mark_powerless();
        result
    }

    /// Clip only at the selected output encoding, not during computation.
    pub fn to_srgb8(self) -> (u32, u32) {
        self.to_rgb8(ColorSpace::Srgb)
    }

    pub fn to_rgb8(self, space: ColorSpace) -> (u32, u32) {
        let color = self.convert(space);
        let bytes = color
            .channels
            .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u32);
        (
            (bytes[0] << 16) | (bytes[1] << 8) | bytes[2],
            (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u32,
        )
    }

    pub(crate) fn legacy(self) -> Option<u32> {
        (self.alpha > 0.0).then(|| self.to_srgb8().0)
    }

    pub(crate) fn opaque_rgb(self) -> Option<u32> {
        if self.space != ColorSpace::Srgb || self.alpha != 1.0 || self.missing != 0 {
            return None;
        }
        let (rgb, _) = self.to_srgb8();
        (Self::from_rgb(rgb).channels == self.channels).then_some(rgb)
    }

    fn finite(self) -> Option<Self> {
        (self.channels.iter().all(|v| v.is_finite()) && self.alpha.is_finite()).then_some(self)
    }

    fn powerless(self) -> bool {
        match self.space {
            ColorSpace::Hsl => {
                self.channels[1] <= 0.00001 || self.channels[2] <= 0.0 || self.channels[2] >= 1.0
            }
            ColorSpace::Hwb => self.channels[1] + self.channels[2] >= 0.99999,
            ColorSpace::Lch => self.channels[1] <= 0.0015,
            ColorSpace::Oklch => self.channels[1] <= 0.000004,
            _ => false,
        }
    }

    fn mark_powerless(&mut self) {
        if let Some(hue) = self.space.hue()
            && self.powerless()
        {
            self.missing |= 1 << hue;
            self.channels[hue] = 0.0;
            let epsilon = match self.space {
                ColorSpace::Hsl => 0.00001,
                ColorSpace::Lch => 0.0015,
                ColorSpace::Oklch => 0.000004,
                _ => 0.0,
            };
            if self.space != ColorSpace::Hwb
                && self.channels[1] > 0.0
                && self.channels[1] <= epsilon
            {
                self.channels[1] = 0.0;
            }
        }
    }
}

type Matrix = [[f64; 3]; 3];

const SRGB: Matrix = [
    [0.4123907992659595, 0.357584339383878, 0.1804807884018343],
    [0.2126390058715104, 0.715168678767756, 0.0721923153607337],
    [0.0193308187155918, 0.119194779794626, 0.9505321522496607],
];
const P3: Matrix = [
    [0.4865709486482162, 0.2656676931690931, 0.1982172852343625],
    [0.2289745640697488, 0.6917385218365064, 0.079286914093745],
    [0.0, 0.0451133818589026, 1.043944368900976],
];
const A98: Matrix = [
    [0.5766690429101305, 0.1855582379065463, 0.1882286462349947],
    [0.2973449752505362, 0.6273635662554661, 0.0752914584939979],
    [0.0270313613864123, 0.0706888525358272, 0.9913375368376388],
];
const PROPHOTO: Matrix = [
    [0.7977666449006423, 0.1351812974005331, 0.0313477341283922],
    [0.2880748288194013, 0.711835234241873, 0.0000899369387256],
    [0.0, 0.0, 0.8251046025104601],
];
const REC2020: Matrix = [
    [0.6369580483012914, 0.1446169035862083, 0.1688809751641721],
    [0.2627002120112671, 0.6779980715188708, 0.059301716469862],
    [0.0, 0.0280726930490874, 1.060985057710791],
];
const D50_TO_D65: Matrix = [
    [0.955473421488075, -0.023098454948765, 0.0632592432005707],
    [-0.0283697093338637, 1.0099953980813041, 0.0210414411919173],
    [0.0123140148644819, -0.0205076492988989, 1.330365926242124],
];
const D50: [f64; 3] = [0.9642956764295677, 1.0, 0.8251046025104601];

fn mul(m: Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

fn inverse(m: Matrix) -> Matrix {
    let [a, b, c] = m;
    let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0]);
    [
        [
            b[1] * c[2] - b[2] * c[1],
            a[2] * c[1] - a[1] * c[2],
            a[1] * b[2] - a[2] * b[1],
        ],
        [
            b[2] * c[0] - b[0] * c[2],
            a[0] * c[2] - a[2] * c[0],
            a[2] * b[0] - a[0] * b[2],
        ],
        [
            b[0] * c[1] - b[1] * c[0],
            a[1] * c[0] - a[0] * c[1],
            a[0] * b[1] - a[1] * b[0],
        ],
    ]
    .map(|row| row.map(|v| v / det))
}

fn srgb_decode(v: f64) -> f64 {
    v.signum()
        * if v.abs() <= 0.04045 {
            v.abs() / 12.92
        } else {
            ((v.abs() + 0.055) / 1.055).powf(2.4)
        }
}
fn srgb_encode(v: f64) -> f64 {
    v.signum()
        * if v.abs() <= 0.0031308 {
            v.abs() * 12.92
        } else {
            1.055 * v.abs().powf(1.0 / 2.4) - 0.055
        }
}
fn power(v: f64, p: f64) -> f64 {
    v.signum() * v.abs().powf(p)
}
fn decode(space: ColorSpace, v: f64) -> f64 {
    match space {
        ColorSpace::Srgb | ColorSpace::DisplayP3 => srgb_decode(v),
        ColorSpace::A98Rgb => power(v, 563.0 / 256.0),
        ColorSpace::ProphotoRgb => {
            if v.abs() <= 16.0 / 512.0 {
                v / 16.0
            } else {
                power(v, 1.8)
            }
        }
        ColorSpace::Rec2020 => {
            let a = 1.09929682680944;
            if v.abs() < 0.018053968510807 * 4.5 {
                v / 4.5
            } else {
                v.signum() * ((v.abs() + a - 1.0) / a).powf(1.0 / 0.45)
            }
        }
        _ => v,
    }
}
fn encode(space: ColorSpace, v: f64) -> f64 {
    match space {
        ColorSpace::Srgb | ColorSpace::DisplayP3 => srgb_encode(v),
        ColorSpace::A98Rgb => power(v, 256.0 / 563.0),
        ColorSpace::ProphotoRgb => {
            if v.abs() <= 1.0 / 512.0 {
                v * 16.0
            } else {
                power(v, 1.0 / 1.8)
            }
        }
        ColorSpace::Rec2020 => {
            let a = 1.09929682680944;
            if v.abs() < 0.018053968510807 {
                v * 4.5
            } else {
                v.signum() * (a * v.abs().powf(0.45) - (a - 1.0))
            }
        }
        _ => v,
    }
}

fn polar(v: [f64; 3]) -> [f64; 3] {
    [
        v[0],
        v[1].hypot(v[2]),
        v[2].atan2(v[1]).to_degrees().rem_euclid(360.0),
    ]
}
fn rectangular(v: [f64; 3]) -> [f64; 3] {
    [
        v[0],
        v[1] * v[2].to_radians().cos(),
        v[1] * v[2].to_radians().sin(),
    ]
}
fn lab_to_xyz([l, a, b]: [f64; 3]) -> [f64; 3] {
    let f = (l + 16.0) / 116.0;
    let f = [f + a / 500.0, f, f - b / 200.0];
    std::array::from_fn(|i| {
        D50[i]
            * if f[i] > 6.0 / 29.0 {
                f[i].powi(3)
            } else {
                (116.0 * f[i] - 16.0) / (24389.0 / 27.0)
            }
    })
}
fn xyz_to_lab(v: [f64; 3]) -> [f64; 3] {
    let f: [f64; 3] = std::array::from_fn(|i| {
        let v = v[i] / D50[i];
        if v > 216.0 / 24389.0 {
            v.cbrt()
        } else {
            ((24389.0 / 27.0) * v + 16.0) / 116.0
        }
    });
    [
        116.0 * f[1] - 16.0,
        500.0 * (f[0] - f[1]),
        200.0 * (f[1] - f[2]),
    ]
}

const OK_LMS: Matrix = [
    [0.4122214708, 0.5363325363, 0.0514459929],
    [0.2119034982, 0.6806995451, 0.1073969566],
    [0.0883024619, 0.2817188376, 0.6299787005],
];
const OK_LAB: Matrix = [
    [0.2104542553, 0.7936177850, -0.0040720468],
    [1.9779984951, -2.4285922050, 0.4505937099],
    [0.0259040371, 0.7827717662, -0.8086757660],
];

fn hsl_to_rgb([h, s, l]: [f64; 3]) -> [f64; 3] {
    let a = s * l.min(1.0 - l);
    [0.0, 8.0, 4.0].map(|n| {
        let k = (n + h / 30.0).rem_euclid(12.0);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    })
}
fn rgb_to_hsl([r, g, b]: [f64; 3]) -> [f64; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let mut s = if d == 0.0 || l == 0.0 || l == 1.0 {
        0.0
    } else {
        (max - l) / l.min(1.0 - l)
    };
    let mut h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if s < 0.0 {
        s = -s;
        h += 180.0;
    }
    [h.rem_euclid(360.0), s, l]
}

fn to_xyz(space: ColorSpace, v: [f64; 3]) -> [f64; 3] {
    use ColorSpace::*;
    match space {
        XyzD65 => v,
        XyzD50 => mul(D50_TO_D65, v),
        Lab | Lch => mul(
            D50_TO_D65,
            lab_to_xyz(if space == Lch { rectangular(v) } else { v }),
        ),
        Oklab | Oklch => {
            let lab = if space == Oklch { rectangular(v) } else { v };
            let lms = mul(inverse(OK_LAB), lab).map(|v| v.powi(3));
            mul(SRGB, mul(inverse(OK_LMS), lms))
        }
        Hsl => to_xyz(Srgb, hsl_to_rgb(v)),
        Hwb => {
            let [h, w, b] = v;
            let rgb = if w + b >= 1.0 {
                [w / (w + b); 3]
            } else {
                hsl_to_rgb([h, 1.0, 0.5]).map(|c| c * (1.0 - w - b) + w)
            };
            to_xyz(Srgb, rgb)
        }
        _ => {
            let matrix = match space {
                DisplayP3 => P3,
                A98Rgb => A98,
                ProphotoRgb => PROPHOTO,
                Rec2020 => REC2020,
                _ => SRGB,
            };
            let xyz = mul(matrix, v.map(|c| decode(space, c)));
            if space == ProphotoRgb {
                mul(D50_TO_D65, xyz)
            } else {
                xyz
            }
        }
    }
}
fn from_xyz(space: ColorSpace, xyz: [f64; 3]) -> [f64; 3] {
    use ColorSpace::*;
    match space {
        XyzD65 => xyz,
        XyzD50 => mul(inverse(D50_TO_D65), xyz),
        Lab | Lch => {
            let lab = xyz_to_lab(mul(inverse(D50_TO_D65), xyz));
            if space == Lch { polar(lab) } else { lab }
        }
        Oklab | Oklch => {
            let lab = mul(OK_LAB, mul(OK_LMS, mul(inverse(SRGB), xyz)).map(f64::cbrt));
            if space == Oklch { polar(lab) } else { lab }
        }
        Hsl => rgb_to_hsl(from_xyz(Srgb, xyz)),
        Hwb => {
            let rgb = from_xyz(Srgb, xyz);
            [
                rgb_to_hsl(rgb)[0],
                rgb[0].min(rgb[1]).min(rgb[2]),
                1.0 - rgb[0].max(rgb[1]).max(rgb[2]),
            ]
        }
        _ => {
            let matrix = match space {
                DisplayP3 => P3,
                A98Rgb => A98,
                ProphotoRgb => PROPHOTO,
                Rec2020 => REC2020,
                _ => SRGB,
            };
            let xyz = if space == ProphotoRgb {
                mul(inverse(D50_TO_D65), xyz)
            } else {
                xyz
            };
            mul(inverse(matrix), xyz).map(|c| encode(space, c))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HueMethod {
    #[default]
    Shorter,
    Longer,
    Increasing,
    Decreasing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mix {
    space: ColorSpace,
    hue: HueMethod,
    left: Color,
    right: Color,
    weight: f64,
    alpha_multiplier: f64,
}

pub(crate) fn animation_mix(left: ResolvedColor, right: ResolvedColor, progress: f64) -> Mix {
    Mix {
        space: ColorSpace::Oklab,
        hue: HueMethod::Shorter,
        left: Color::Absolute(left),
        right: Color::Absolute(right),
        weight: 1.0 - progress,
        alpha_multiplier: 1.0,
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Channel {
    None,
    Value(f64),
    Reference(usize),
    Math(Vec<TokenKind>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relative {
    origin: Color,
    space: ColorSpace,
    rgb: bool,
    channels: [Channel; 3],
    alpha: Channel,
}

fn mix(m: &Mix, current: ResolvedColor, depth: usize) -> Option<ResolvedColor> {
    let mut a = resolve(&m.left, current, depth + 1)?.convert(m.space);
    let mut b = resolve(&m.right, current, depth + 1)?.convert(m.space);
    a.mark_powerless();
    b.mark_powerless();
    let missing = a.missing & b.missing;
    for i in 0..4 {
        let av = if i == 3 { a.alpha } else { a.channels[i] };
        let bv = if i == 3 { b.alpha } else { b.channels[i] };
        if a.missing & (1 << i) != 0 {
            if i == 3 {
                a.alpha = bv;
            } else {
                a.channels[i] = bv;
            }
        } else if b.missing & (1 << i) != 0 {
            if i == 3 {
                b.alpha = av;
            } else {
                b.channels[i] = av;
            }
        }
    }
    let t = m.weight;
    let alpha = a.alpha * t + b.alpha * (1.0 - t);
    let mut channels = std::array::from_fn(|i| {
        if alpha == 0.0 {
            0.0
        } else {
            (a.channels[i] * a.alpha * t + b.channels[i] * b.alpha * (1.0 - t)) / alpha
        }
    });
    if let Some(i) = m.space.hue() {
        let mut ah = a.channels[i].rem_euclid(360.0);
        let mut bh = b.channels[i].rem_euclid(360.0);
        let delta = bh - ah;
        match m.hue {
            HueMethod::Shorter if delta > 180.0 => ah += 360.0,
            HueMethod::Shorter if delta < -180.0 => bh += 360.0,
            HueMethod::Longer if delta > 0.0 && delta < 180.0 => ah += 360.0,
            HueMethod::Longer if delta <= 0.0 && delta > -180.0 => bh += 360.0,
            HueMethod::Increasing if bh < ah => bh += 360.0,
            HueMethod::Decreasing if ah < bh => ah += 360.0,
            _ => {}
        }
        channels[i] = (ah * t + bh * (1.0 - t)).rem_euclid(360.0);
    }
    for (i, channel) in channels.iter_mut().enumerate() {
        if missing & (1 << i) != 0 {
            *channel = 0.0;
        }
    }
    ResolvedColor {
        space: m.space,
        channels,
        alpha: alpha * m.alpha_multiplier,
        missing,
    }
    .finite()
}

pub(super) fn resolve(
    color: &Color,
    current: ResolvedColor,
    depth: usize,
) -> Option<ResolvedColor> {
    if depth > MAX_DEPTH {
        return None;
    }
    match color {
        Color::Rgb(rgb) => Some(ResolvedColor::from_rgb(*rgb)),
        Color::Transparent => Some(ResolvedColor::TRANSPARENT),
        Color::CurrentColor => Some(current),
        Color::Absolute(color) => color
            .finite()
            .filter(|c| to_xyz(c.space, c.channels).iter().all(|v| v.is_finite())),
        Color::System(name) => Some(ResolvedColor::from_rgb(system_rgb(name)?)),
        Color::Mix(m) => mix(m, current, depth),
        Color::Relative(r) => relative(r, current, depth, true),
    }
}

/// Retain currentcolor dependencies until use time, including when inherited
/// as a decoration. Context-independent relative colors compute coordinates.
pub(super) fn compute(color: &Color, current: ResolvedColor) -> Option<Color> {
    Some(match color {
        Color::Relative(r) => {
            let origin = compute(&r.origin, current)?;
            if depends_current(&origin) {
                Color::Relative(Arc::new(Relative {
                    origin,
                    ..r.as_ref().clone()
                }))
            } else {
                Color::Absolute(resolve(color, current, 0)?)
            }
        }
        Color::System(_) => Color::Absolute(resolve(color, current, 0)?),
        Color::Mix(m) => {
            if depends_current(color) {
                Color::Mix(Arc::new(Mix {
                    left: compute(&m.left, current)?,
                    right: compute(&m.right, current)?,
                    ..m.as_ref().clone()
                }))
            } else {
                Color::Absolute(resolve(color, current, 0)?)
            }
        }
        other => other.clone(),
    })
}

fn depends_current(color: &Color) -> bool {
    match color {
        Color::CurrentColor => true,
        Color::Relative(r) => depends_current(&r.origin),
        Color::Mix(m) => depends_current(&m.left) || depends_current(&m.right),
        _ => false,
    }
}

pub(super) fn system_rgb(name: &str) -> Option<u32> {
    // Deterministic light UA palette, not an OS theme/forced-colors adapter.
    Some(match name {
        "canvas" | "buttonface" | "field" | "window" | "background" | "menu" | "highlighttext"
        | "selecteditemtext" => 0xffffff,
        "canvastext" | "buttontext" | "fieldtext" | "windowtext" | "menutext" | "text"
        | "captiontext" | "infotext" => 0x000000,
        "linktext" => 0x0000ee,
        "visitedtext" => 0x551a8b,
        "activetext" => 0xff0000,
        "highlight" | "selecteditem" | "accentcolor" | "activecaption" => 0x0066cc,
        "accentcolortext" => 0xffffff,
        "graytext" | "inactivecaptiontext" => 0x808080,
        "buttonborder" | "windowframe" => 0x767676,
        "activeborder" | "inactiveborder" | "inactivecaption" | "scrollbar" | "threedface" => {
            0xc0c0c0
        }
        "buttonhighlight" | "threedhighlight" | "threedlightshadow" => 0xffffff,
        "buttonshadow" | "threedshadow" | "threeddarkshadow" => 0x808080,
        "appworkspace" => 0xababab,
        "infobackground" => 0xffffe1,
        _ => return None,
    })
}

struct Cursor<'a> {
    tokens: &'a [TokenKind],
    pos: usize,
}
impl<'a> Cursor<'a> {
    fn new(tokens: &'a [TokenKind]) -> Self {
        Self { tokens, pos: 0 }
    }
    fn ws(&mut self) -> bool {
        let start = self.pos;
        while self.tokens.get(self.pos) == Some(&TokenKind::Whitespace) {
            self.pos += 1;
        }
        self.pos > start
    }
    fn ident(&mut self) -> Option<&'a str> {
        self.ws();
        let TokenKind::Ident(name) = self.tokens.get(self.pos)? else {
            return None;
        };
        self.pos += 1;
        Some(name)
    }
    fn take(&mut self, expected: TokenKind) -> bool {
        self.ws();
        if self.tokens.get(self.pos) == Some(&expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn end(&mut self) -> bool {
        self.ws();
        self.pos == self.tokens.len()
    }
    fn component(&mut self) -> Option<&'a [TokenKind]> {
        self.ws();
        let start = self.pos;
        let token = self.tokens.get(self.pos)?;
        self.pos += 1;
        if matches!(token, TokenKind::Function(_)) {
            let mut depth = 1;
            while depth > 0 {
                match self.tokens.get(self.pos)? {
                    TokenKind::Function(_) | TokenKind::OpenParen => depth += 1,
                    TokenKind::CloseParen => depth -= 1,
                    _ => {}
                }
                if depth > MAX_DEPTH {
                    return None;
                }
                self.pos += 1;
            }
        }
        Some(&self.tokens[start..self.pos])
    }
}

pub(super) fn parse_tokens(tokens: &[TokenKind]) -> Option<Color> {
    if tokens.len() > MAX_TOKENS {
        return None;
    }
    parse(tokens, 0)
}
fn parse(tokens: &[TokenKind], depth: usize) -> Option<Color> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut cursor = Cursor::new(tokens);
    let component = cursor.component()?;
    if !cursor.end() {
        return None;
    }
    match component {
        [TokenKind::Ident(name)] => keyword(name),
        [TokenKind::Hash { value, .. }] => {
            if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            match value.len() {
                3 | 6 => keyword(&format!("#{value}")),
                4 | 8 => {
                    let value = if value.len() == 4 {
                        value.chars().flat_map(|c| [c, c]).collect::<String>()
                    } else {
                        value.clone()
                    };
                    let rgba = u32::from_str_radix(&value, 16).ok()?;
                    let mut color = ResolvedColor::from_rgb(rgba >> 8);
                    color.alpha = f64::from(rgba & 255) / 255.0;
                    Some(Color::Absolute(color))
                }
                _ => None,
            }
        }
        [
            TokenKind::Function(name),
            arguments @ ..,
            TokenKind::CloseParen,
        ] => {
            let name = name.to_ascii_lowercase();
            if name == "color-mix" {
                return parse_mix(arguments, depth);
            }
            parse_function(&name, arguments, depth)
        }
        _ => None,
    }
}

fn syntax_space(name: &str) -> Option<ColorSpace> {
    Some(match name {
        "rgb" | "rgba" => ColorSpace::Srgb,
        "hsl" | "hsla" => ColorSpace::Hsl,
        "hwb" => ColorSpace::Hwb,
        "lab" => ColorSpace::Lab,
        "lch" => ColorSpace::Lch,
        "oklab" => ColorSpace::Oklab,
        "oklch" => ColorSpace::Oklch,
        _ => return None,
    })
}
fn names(space: ColorSpace, rgb: bool) -> [&'static str; 4] {
    use ColorSpace::*;
    match space {
        Srgb if rgb => ["r", "g", "b", "alpha"],
        Hsl => ["h", "s", "l", "alpha"],
        Hwb => ["h", "w", "b", "alpha"],
        Lab | Oklab => ["l", "a", "b", "alpha"],
        Lch | Oklch => ["l", "c", "h", "alpha"],
        XyzD50 | XyzD65 => ["x", "y", "z", "alpha"],
        _ => ["r", "g", "b", "alpha"],
    }
}
fn percent_scale(space: ColorSpace, rgb: bool, index: usize) -> f64 {
    if index == 3 {
        return 1.0;
    }
    use ColorSpace::*;
    match space {
        Srgb if rgb => 255.0,
        Hsl | Hwb if index == 0 => 360.0,
        Hsl | Hwb => 100.0,
        Lab if index == 0 => 100.0,
        Lab => 125.0,
        Lch => [100.0, 150.0, 360.0][index],
        Oklab if index == 0 => 1.0,
        Oklab => 0.4,
        Oklch => [1.0, 0.4, 360.0][index],
        _ => 1.0,
    }
}
fn number(tokens: &[TokenKind]) -> Option<f64> {
    let [TokenKind::Number(value)] = tokens else {
        return None;
    };
    let value = value.value().ok()?;
    value.is_finite().then_some(value)
}
fn percentage(tokens: &[TokenKind]) -> Option<f64> {
    let [TokenKind::Percentage(value)] = tokens else {
        return None;
    };
    let value = value.value().ok()?;
    value.is_finite().then_some(value)
}

fn channel(
    tokens: &[TokenKind],
    space: ColorSpace,
    rgb: bool,
    index: usize,
    relative: bool,
) -> Option<Channel> {
    if matches!(tokens, [TokenKind::Ident(n)] if n.eq_ignore_ascii_case("none")) {
        return Some(Channel::None);
    }
    if let [TokenKind::Ident(name)] = tokens {
        let reference = names(space, rgb)
            .iter()
            .position(|n| name.eq_ignore_ascii_case(n))?;
        return relative.then_some(Channel::Reference(reference));
    }
    if let Some(v) = number(tokens) {
        return Some(Channel::Value(v));
    }
    if let Some(v) = percentage(tokens) {
        if space.hue() == Some(index) {
            return None;
        }
        return Some(Channel::Value(v * percent_scale(space, rgb, index) / 100.0));
    }
    if space.hue() == Some(index) && matches!(tokens, [TokenKind::Dimension { .. }]) {
        let expression = MathExpression::from_tokens(tokens, None)?;
        if expression.dimension != Dimension::ANGLE {
            return None;
        }
        return Some(Channel::Value(
            expression.evaluate64(&LengthContext::default(), 0.0)?,
        ));
    }
    if !matches!(tokens.first(), Some(TokenKind::Function(_))) {
        return None;
    }
    if relative && tokens.iter().any(|t| matches!(t, TokenKind::Percentage(_))) {
        return None;
    }
    let replaced = replace_references(tokens, &[0.0; 4], space, rgb, relative)?;
    let expression = MathExpression::from_tokens(&replaced, Some(Dimension::NUMBER))?;
    if expression.dimension != Dimension::NUMBER
        && !(space.hue() == Some(index) && expression.dimension == Dimension::ANGLE)
    {
        return None;
    }
    Some(Channel::Math(tokens.to_vec()))
}

fn replace_references(
    tokens: &[TokenKind],
    values: &[f64; 4],
    space: ColorSpace,
    rgb: bool,
    relative: bool,
) -> Option<Vec<TokenKind>> {
    let mut result = Vec::with_capacity(tokens.len());
    for token in tokens {
        if let TokenKind::Ident(name) = token
            && let Some(index) = names(space, rgb)
                .iter()
                .position(|n| name.eq_ignore_ascii_case(n))
        {
            if !relative {
                return None;
            }
            result.push(
                math::tokenize(&values[index].to_string())?
                    .into_iter()
                    .next()?,
            );
            continue;
        }
        result.push(token.clone());
    }
    Some(result)
}

fn eval_channel(
    channel: &Channel,
    values: &[f64; 4],
    space: ColorSpace,
    rgb: bool,
    index: usize,
) -> Option<(f64, bool)> {
    Some(match channel {
        Channel::None => (0.0, true),
        Channel::Value(v) => (*v, false),
        Channel::Reference(i) => (values[*i], false),
        Channel::Math(tokens) => {
            let replaced = replace_references(tokens, values, space, rgb, true)?;
            let expression = MathExpression::from_tokens(&replaced, Some(Dimension::NUMBER))?;
            (
                expression
                    .evaluate64(&LengthContext::default(), percent_scale(space, rgb, index))?,
                false,
            )
        }
    })
}
fn normalize(mut color: ResolvedColor, rgb: bool, relative: bool) -> ResolvedColor {
    use ColorSpace::*;
    if rgb {
        color.channels = color.channels.map(|v| {
            if relative {
                v / 255.0
            } else {
                v.clamp(0.0, 255.0) / 255.0
            }
        });
    }
    if matches!(color.space, Hsl | Hwb) {
        for i in 1..3 {
            color.channels[i] = if relative {
                color.channels[i] / 100.0
            } else {
                color.channels[i].clamp(0.0, 100.0) / 100.0
            };
        }
    }
    if !relative {
        if matches!(color.space, Lab | Lch) {
            color.channels[0] = color.channels[0].clamp(0.0, 100.0);
        }
        if matches!(color.space, Oklab | Oklch) {
            color.channels[0] = color.channels[0].clamp(0.0, 1.0);
        }
        if matches!(color.space, Lch | Oklch) {
            color.channels[1] = color.channels[1].max(0.0);
        }
    }
    if let Some(i) = color.space.hue() {
        color.channels[i] = color.channels[i].rem_euclid(360.0);
    }
    color.alpha = color.alpha.clamp(0.0, 1.0);
    color
}
fn relative(
    r: &Relative,
    current: ResolvedColor,
    depth: usize,
    relative_syntax: bool,
) -> Option<ResolvedColor> {
    let origin = resolve(&r.origin, current, depth + 1)?.convert(r.space);
    let mut values = [
        origin.channels[0],
        origin.channels[1],
        origin.channels[2],
        origin.alpha,
    ];
    if r.rgb {
        for v in &mut values[..3] {
            *v *= 255.0;
        }
    }
    if matches!(r.space, ColorSpace::Hsl | ColorSpace::Hwb) {
        values[1] *= 100.0;
        values[2] *= 100.0;
    }
    let mut result = ResolvedColor {
        space: r.space,
        channels: [0.0; 3],
        alpha: 1.0,
        missing: 0,
    };
    for (i, channel) in r
        .channels
        .iter()
        .chain(std::iter::once(&r.alpha))
        .enumerate()
    {
        let (v, missing) = eval_channel(channel, &values, r.space, r.rgb, i)?;
        if i == 3 {
            result.alpha = v;
        } else {
            result.channels[i] = v;
        }
        // A directly referenced missing origin component stays missing.
        if missing || matches!(channel, Channel::Reference(j) if origin.missing & (1 << j) != 0) {
            result.missing |= 1 << i;
        }
    }
    normalize(result, r.rgb, relative_syntax).finite()
}

fn parse_function(name: &str, arguments: &[TokenKind], depth: usize) -> Option<Color> {
    let mut cursor = Cursor::new(arguments);
    cursor.ws();
    let relative = matches!(cursor.tokens.get(cursor.pos), Some(TokenKind::Ident(n)) if n.eq_ignore_ascii_case("from"));
    let origin = if relative {
        cursor.pos += 1;
        if !cursor.ws() {
            return None;
        }
        let origin = parse(cursor.component()?, depth + 1)?;
        if !cursor.ws() {
            return None;
        }
        Some(origin)
    } else {
        None
    };
    let rgb = matches!(name, "rgb" | "rgba");
    let space = if name == "color" {
        let space = ColorSpace::parse(cursor.ident()?)?;
        if matches!(
            space,
            ColorSpace::Lab
                | ColorSpace::Lch
                | ColorSpace::Oklab
                | ColorSpace::Oklch
                | ColorSpace::Hsl
                | ColorSpace::Hwb
        ) || !cursor.ws()
        {
            return None;
        }
        space
    } else {
        syntax_space(name)?
    };
    let first_tokens = cursor.component()?;
    let first = channel(first_tokens, space, rgb, 0, relative)?;
    let after_first = cursor.pos;
    let comma = cursor.take(TokenKind::Comma);
    // take() consumes whitespace, so inspect original separator for modern syntax.
    if !comma && !matches!(arguments.get(after_first), Some(TokenKind::Whitespace)) {
        return None;
    }
    if comma && (relative || !matches!(name, "rgb" | "rgba" | "hsl" | "hsla")) {
        return None;
    }
    if comma && matches!(first, Channel::None) {
        return None;
    }
    let second_tokens = cursor.component()?;
    let second = channel(second_tokens, space, rgb, 1, relative)?;
    if comma {
        if !cursor.take(TokenKind::Comma) {
            return None;
        }
    } else if !cursor.ws() {
        return None;
    }
    let third_tokens = cursor.component()?;
    let third = channel(third_tokens, space, rgb, 2, relative)?;
    if comma {
        if matches!(second, Channel::None) || matches!(third, Channel::None) {
            return None;
        }
        let is_percent = |tokens: &[TokenKind]| {
            percentage(tokens).is_some()
                || MathExpression::from_tokens(tokens, Some(Dimension::NUMBER))
                    .is_some_and(|e| e.has_percentage())
        };
        if rgb {
            let percent = is_percent(first_tokens);
            if is_percent(second_tokens) != percent || is_percent(third_tokens) != percent {
                return None;
            }
        } else if !is_percent(second_tokens) || !is_percent(third_tokens) {
            return None;
        }
    }
    let alpha = if cursor.take(if comma {
        TokenKind::Comma
    } else {
        TokenKind::Delim('/')
    }) {
        let a = channel(cursor.component()?, space, rgb, 3, relative)?;
        if comma && matches!(a, Channel::None) {
            return None;
        }
        a
    } else if relative {
        Channel::Reference(3)
    } else {
        Channel::Value(1.0)
    };
    if !cursor.end() {
        return None;
    }
    let r = Relative {
        origin: origin.unwrap_or(Color::Transparent),
        space,
        rgb,
        channels: [first, second, third],
        alpha,
    };
    if relative {
        return Some(Color::Relative(Arc::new(r)));
    }
    let color = self::relative(&r, ResolvedColor::BLACK, depth, false)?;
    Some(if let Some(rgb) = color.opaque_rgb() {
        Color::Rgb(rgb)
    } else {
        Color::Absolute(color)
    })
}

fn parse_mix(arguments: &[TokenKind], depth: usize) -> Option<Color> {
    let mut cursor = Cursor::new(arguments);
    if !cursor.ident()?.eq_ignore_ascii_case("in") || !cursor.ws() {
        return None;
    }
    let space = ColorSpace::parse(cursor.ident()?)?;
    cursor.ws();
    let hue = if matches!(cursor.tokens.get(cursor.pos), Some(TokenKind::Ident(_))) {
        space.hue()?;
        let method = match cursor.ident()?.to_ascii_lowercase().as_str() {
            "shorter" => HueMethod::Shorter,
            "longer" => HueMethod::Longer,
            "increasing" => HueMethod::Increasing,
            "decreasing" => HueMethod::Decreasing,
            _ => return None,
        };
        if !cursor.ws() || !cursor.ident()?.eq_ignore_ascii_case("hue") {
            return None;
        }
        method
    } else {
        HueMethod::Shorter
    };
    if !cursor.take(TokenKind::Comma) {
        return None;
    }
    let (left, p1) = mix_stop(&mut cursor, depth)?;
    if !cursor.take(TokenKind::Comma) {
        return None;
    }
    let (right, p2) = mix_stop(&mut cursor, depth)?;
    if !cursor.end() {
        return None;
    }
    let (p1, p2) = match (p1, p2) {
        (None, None) => (0.5, 0.5),
        (Some(p), None) => (p, 1.0 - p),
        (None, Some(p)) => (1.0 - p, p),
        (Some(a), Some(b)) => (a, b),
    };
    let sum = p1 + p2;
    if sum <= 0.0 {
        return None;
    }
    Some(Color::Mix(Arc::new(Mix {
        space,
        hue,
        left,
        right,
        weight: p1 / sum,
        alpha_multiplier: sum.min(1.0),
    })))
}
fn mix_stop(cursor: &mut Cursor<'_>, depth: usize) -> Option<(Color, Option<f64>)> {
    let first = cursor.component()?;
    let before = weight(first);
    let color = if before.is_some() {
        if !cursor.ws() {
            return None;
        }
        parse(cursor.component()?, depth + 1)?
    } else {
        parse(first, depth + 1)?
    };
    let spaced = cursor.ws();
    let weight = if matches!(
        cursor.tokens.get(cursor.pos),
        Some(TokenKind::Percentage(_) | TokenKind::Function(_))
    ) {
        if !spaced {
            return None;
        }
        if before.is_some() {
            return None;
        }
        Some(weight(cursor.component()?)?)
    } else {
        before
    };
    Some((color, weight))
}

fn weight(tokens: &[TokenKind]) -> Option<f64> {
    let p = if let Some(p) = percentage(tokens) {
        p / 100.0
    } else {
        let expression = MathExpression::from_tokens(tokens, Some(Dimension::NUMBER))?;
        if expression.dimension != Dimension::NUMBER || !expression.has_percentage() {
            return None;
        }
        expression.evaluate64(&LengthContext::default(), 1.0)?
    };
    (0.0..=1.0).contains(&p).then_some(p)
}

#[cfg(test)]
mod tests;
