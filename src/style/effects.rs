//! Typed, bounded CSS effects. Unsupported SVG/path/filter references are
//! rejected, never treated as successful no-ops.
use std::collections::BTreeMap;

use crate::css::{
    tokenizer::TokenKind,
    values::{
        Color, Length, LengthContext,
        components::{self, Component},
    },
};
use crate::layout::{Mat4, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Property {
    Transform,
    TransformOrigin,
    TransformBox,
    Translate,
    Rotate,
    Scale,
    Perspective,
    PerspectiveOrigin,
    TransformStyle,
    BackfaceVisibility,
    Opacity,
    Filter,
    BackdropFilter,
    Blend,
    Isolation,
    ClipPath,
    MaskImage,
    MaskMode,
    MaskRepeat,
    MaskSize,
    MaskPosition,
    MaskOrigin,
    MaskClip,
    MaskComposite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    None,
    Flat,
    Preserve3d,
    Visible,
    Hidden,
    Auto,
    Isolate,
    ContentBox,
    PaddingBox,
    BorderBox,
    FillBox,
    StrokeBox,
    ViewBox,
    NoClip,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Transform {
    Matrix(Mat4),
    Translate([Length; 3]),
    Scale([f64; 3]),
    Rotate([f64; 3], f64),
    Skew(f64, f64),
    Perspective(Length),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Origin(pub Length, pub Length, pub Length);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    Blur(Length),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    Sepia(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    HueRotate(f64),
    DropShadow {
        x: Length,
        y: Length,
        blur: Length,
        color: Color,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Radius {
    Closest,
    Farthest,
    Length(Length),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Inset([Length; 4]),
    Circle(Radius, Origin),
    Ellipse(Radius, Radius, Origin),
    Polygon {
        evenodd: bool,
        points: Vec<(Length, Length)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum MaskImage {
    None,
    Url(String),
    Gradient(Gradient),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub radial: bool,
    pub angle: f64,
    pub stops: Vec<(Color, Option<Length>)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskMode {
    MatchSource,
    Alpha,
    Luminance,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Repeat,
    NoRepeat,
    Space,
    Round,
}
#[derive(Clone, Debug, PartialEq)]
pub enum MaskSize {
    Auto,
    Contain,
    Cover,
    Size(Option<Length>, Option<Length>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Composite {
    Add,
    Subtract,
    Intersect,
    Exclude,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Keyword(Keyword),
    Transforms(Vec<Transform>),
    Origin(Origin),
    Number(f32),
    Length(Length),
    Filters(Vec<Filter>),
    Blend(Blend),
    Shape(Option<Shape>),
    Images(Vec<MaskImage>),
    Modes(Vec<MaskMode>),
    Repeats(Vec<(Repeat, Repeat)>),
    Sizes(Vec<MaskSize>),
    Positions(Vec<super::ObjectPosition>),
    Boxes(Vec<Keyword>),
    Composites(Vec<Composite>),
}

#[derive(Clone, Debug, Default)]
pub struct Effects(BTreeMap<Property, Value>);

impl PartialEq for Effects {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .keys()
            .chain(other.0.keys())
            .all(|&p| self.get(p) == other.get(p))
    }
}

impl Effects {
    pub(crate) fn non_transformable(&mut self) {
        for p in [
            Property::Transform,
            Property::Translate,
            Property::Rotate,
            Property::Scale,
            Property::Perspective,
            Property::TransformStyle,
            Property::BackfaceVisibility,
        ] {
            self.0.remove(&p);
        }
    }
    pub fn get(&self, p: Property) -> Value {
        self.0.get(&p).cloned().unwrap_or_else(|| initial(p))
    }
    pub fn set(&mut self, p: Property, v: Value) {
        self.0.insert(p, v);
    }
    pub fn keyword(&self, p: Property) -> Keyword {
        match self.get(p) {
            Value::Keyword(k) => k,
            _ => Keyword::None,
        }
    }
    pub fn number(&self, p: Property) -> f32 {
        match self.get(p) {
            Value::Number(n) => n,
            _ => 1.0,
        }
    }
    pub fn transforms(&self, p: Property) -> Vec<Transform> {
        match self.get(p) {
            Value::Transforms(v) => v,
            _ => Vec::new(),
        }
    }
    pub fn filters(&self, p: Property) -> Vec<Filter> {
        match self.get(p) {
            Value::Filters(v) => v,
            _ => Vec::new(),
        }
    }
    pub fn blend(&self) -> Blend {
        match self.get(Property::Blend) {
            Value::Blend(v) => v,
            _ => Blend::Normal,
        }
    }
    pub fn transformed(&self) -> bool {
        [
            Property::Transform,
            Property::Translate,
            Property::Rotate,
            Property::Scale,
        ]
        .into_iter()
        .any(|p| !self.transforms(p).is_empty())
    }
    pub fn containing_block(&self) -> bool {
        self.transformed()
            || matches!(self.get(Property::Perspective), Value::Length(_))
            || !self.filters(Property::Filter).is_empty()
            || !self.filters(Property::BackdropFilter).is_empty()
    }
    pub fn context(&self) -> bool {
        self.containing_block()
            || self.number(Property::Opacity) < 1.0
            || self.blend() != Blend::Normal
            || self.keyword(Property::Isolation) == Keyword::Isolate
            || matches!(self.get(Property::ClipPath), Value::Shape(Some(_)))
            || matches!(self.get(Property::MaskImage), Value::Images(v) if v.iter().any(|i| *i != MaskImage::None))
    }
    pub fn grouped(&self) -> bool {
        self.number(Property::Opacity) < 1.0
            || !self.filters(Property::Filter).is_empty()
            || !self.filters(Property::BackdropFilter).is_empty()
            || self.blend() != Blend::Normal
            || self.keyword(Property::Isolation) == Keyword::Isolate
            || matches!(self.get(Property::ClipPath), Value::Shape(Some(_)))
            || matches!(self.get(Property::MaskImage), Value::Images(v) if v.iter().any(|i| *i != MaskImage::None))
    }
    pub fn urls(&self) -> Vec<String> {
        match self.get(Property::MaskImage) {
            Value::Images(v) => v
                .into_iter()
                .filter_map(|i| {
                    if let MaskImage::Url(u) = i {
                        Some(u)
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    pub fn matrix(&self, border: Rect, content: Rect, size: f32) -> Mat4 {
        let r = if matches!(
            self.keyword(Property::TransformBox),
            Keyword::ContentBox | Keyword::FillBox
        ) {
            content
        } else {
            border
        };
        let origin = match self.get(Property::TransformOrigin) {
            Value::Origin(o) => o,
            _ => Origin(
                Length::Percent(50.0),
                Length::Percent(50.0),
                Length::Px(0.0),
            ),
        };
        let mut m = Mat4::IDENTITY;
        for p in [
            Property::Translate,
            Property::Rotate,
            Property::Scale,
            Property::Transform,
        ] {
            for t in self.transforms(p) {
                m = m.then(t.matrix(r, size));
            }
        }
        m.around(
            (r.x + origin.0.resolve(size, r.width)) as f64,
            (r.y + origin.1.resolve(size, r.height)) as f64,
            origin.2.resolve(size, 0.0) as f64,
        )
    }
    pub fn perspective(&self, r: Rect, size: f32) -> Mat4 {
        let Value::Length(l) = self.get(Property::Perspective) else {
            return Mat4::IDENTITY;
        };
        let o = match self.get(Property::PerspectiveOrigin) {
            Value::Origin(o) => o,
            _ => unreachable!(),
        };
        Mat4::perspective(l.resolve(size, 0.0) as f64).around(
            (r.x + o.0.resolve(size, r.width)) as f64,
            (r.y + o.1.resolve(size, r.height)) as f64,
            0.0,
        )
    }

    pub fn ink_bounds(&self, mut r: Rect, size: f32) -> Rect {
        for f in self.filters(Property::Filter) {
            let expand = |r: Rect, pad: f32| Rect {
                x: r.x - pad,
                y: r.y - pad,
                width: r.width + pad * 2.0,
                height: r.height + pad * 2.0,
            };
            match f {
                Filter::Blur(l) => r = expand(r, l.resolve(size, 0.0).clamp(0.0, 64.0) * 3.0),
                Filter::DropShadow { x, y, blur, .. } => {
                    let shadow = expand(
                        Rect {
                            x: r.x + x.resolve(size, 0.0).clamp(-1e6, 1e6),
                            y: r.y + y.resolve(size, 0.0).clamp(-1e6, 1e6),
                            ..r
                        },
                        blur.resolve(size, 0.0).clamp(0.0, 64.0) * 3.0,
                    );
                    r = r.union(shadow);
                }
                _ => {}
            }
        }
        r
    }
}

impl Transform {
    fn matrix(&self, r: Rect, size: f32) -> Mat4 {
        match self {
            Self::Matrix(m) => *m,
            Self::Translate(v) => Mat4::translation(
                v[0].resolve(size, r.width) as f64,
                v[1].resolve(size, r.height) as f64,
                v[2].resolve(size, 0.0) as f64,
            ),
            Self::Scale(v) => Mat4::scale(v[0], v[1], v[2]),
            Self::Rotate(v, a) => Mat4::rotation(*v, *a),
            Self::Skew(x, y) => {
                let mut m = Mat4::IDENTITY;
                m.0[0][1] = x.tan();
                m.0[1][0] = y.tan();
                m
            }
            Self::Perspective(l) => Mat4::perspective(l.resolve(size, 0.0) as f64),
        }
    }
}

fn initial(p: Property) -> Value {
    use Keyword as K;
    use Property as P;
    match p {
        P::Transform | P::Translate | P::Rotate | P::Scale => Value::Transforms(Vec::new()),
        P::TransformOrigin | P::PerspectiveOrigin => Value::Origin(Origin(
            Length::Percent(50.0),
            Length::Percent(50.0),
            Length::Px(0.0),
        )),
        P::TransformBox => Value::Keyword(K::ViewBox),
        P::Perspective => Value::Keyword(K::None),
        P::TransformStyle => Value::Keyword(K::Flat),
        P::BackfaceVisibility => Value::Keyword(K::Visible),
        P::Opacity => Value::Number(1.0),
        P::Filter | P::BackdropFilter => Value::Filters(Vec::new()),
        P::Blend => Value::Blend(Blend::Normal),
        P::Isolation => Value::Keyword(K::Auto),
        P::ClipPath => Value::Shape(None),
        P::MaskImage => Value::Images(vec![MaskImage::None]),
        P::MaskMode => Value::Modes(vec![MaskMode::MatchSource]),
        P::MaskRepeat => Value::Repeats(vec![(Repeat::Repeat, Repeat::Repeat)]),
        P::MaskSize => Value::Sizes(vec![MaskSize::Auto]),
        P::MaskPosition => Value::Positions(vec![super::ObjectPosition {
            x: super::ObjectAxis {
                from_end: false,
                offset: Length::Percent(0.0),
            },
            y: super::ObjectAxis {
                from_end: false,
                offset: Length::Percent(0.0),
            },
        }]),
        P::MaskOrigin | P::MaskClip => Value::Boxes(vec![K::BorderBox]),
        P::MaskComposite => Value::Composites(vec![Composite::Add]),
    }
}

fn number(c: &Component) -> Option<f64> {
    let n = c
        .number()
        .map(f64::from)
        .or_else(|| c.number_math()?.evaluate64(&LengthContext::default(), 0.0))?;
    n.is_finite().then_some(n.clamp(-1e6, 1e6))
}
fn amount(c: &Component) -> Option<f32> {
    if let Component::Token(TokenKind::Percentage(n)) = c {
        let n = n.value().ok()? as f32 / 100.0;
        return n.is_finite().then_some(n.clamp(-1e6, 1e6));
    }
    number(c).map(|n| n as f32)
}
fn angle(c: &Component) -> Option<f64> {
    if number(c) == Some(0.0) {
        return Some(0.0);
    }
    let Component::Token(TokenKind::Dimension { number, unit }) = c else {
        return None;
    };
    let v = number.value().ok()?;
    if !v.is_finite() {
        return None;
    }
    Some(match unit.to_ascii_lowercase().as_str() {
        "deg" => v.rem_euclid(360.0).to_radians(),
        "rad" => v.rem_euclid(std::f64::consts::TAU),
        "turn" => v.rem_euclid(1.0) * std::f64::consts::TAU,
        "grad" => v.rem_euclid(400.0) * std::f64::consts::PI / 200.0,
        _ => return None,
    })
}
fn length(c: &Component, percent: bool, positive: bool) -> Option<Length> {
    let l = c.length()?;
    if !percent && l.has_percentage() {
        return None;
    }
    if positive && !matches!(l, Length::Math(_)) && l.resolve(16.0, 100.0) < 0.0 {
        return None;
    }
    Some(l)
}
fn comma(c: &Component) -> bool {
    matches!(c, Component::Token(TokenKind::Comma))
}
fn slash(c: &Component) -> bool {
    matches!(c, Component::Token(TokenKind::Delim('/')))
}
fn parts(v: &[Component]) -> Option<Vec<&[Component]>> {
    let p: Vec<_> = v.split(comma).collect();
    (p.len() <= 16 && p.iter().all(|v| !v.is_empty())).then_some(p)
}
fn origin(v: &[Component], z: bool) -> Option<Origin> {
    if v.is_empty() || v.len() > if z { 3 } else { 2 } {
        return None;
    }
    let p = super::box_values::position(&v[..v.len().min(2)])?;
    let axis = |a: super::ObjectAxis| {
        if a.from_end {
            Length::Percent(100.0)
        } else {
            a.offset
        }
    };
    Some(Origin(
        axis(p.x),
        axis(p.y),
        if v.len() == 3 {
            length(&v[2], false, false)?
        } else {
            Length::Px(0.0)
        },
    ))
}
fn rotation(v: &[Component]) -> Option<Transform> {
    let (axis, a) = match v {
        [a] => ([0.0, 0.0, 1.0], angle(a)?),
        [axis, a] => (
            match axis.ident()?.to_ascii_lowercase().as_str() {
                "x" => [1.0, 0.0, 0.0],
                "y" => [0.0, 1.0, 0.0],
                "z" => [0.0, 0.0, 1.0],
                _ => return None,
            },
            angle(a)?,
        ),
        [x, y, z, a] => ([number(x)?, number(y)?, number(z)?], angle(a)?),
        _ => return None,
    };
    Some(Transform::Rotate(axis, a))
}

fn transform(c: &Component) -> Option<Transform> {
    let Component::Function(name, args) = c else {
        return None;
    };
    let v = components::from_tokens(args)?;
    let groups = v.split(comma).collect::<Vec<_>>();
    if groups.iter().any(|g| g.len() != 1) {
        return None;
    }
    let v: Vec<_> = groups.into_iter().map(|g| &g[0]).collect();
    let n = |i| number(v[i]);
    let l = |i, p| length(v[i], p, false);
    let a = |i| angle(v[i]);
    let zero = || Length::Px(0.0);
    Some(match (name.to_ascii_lowercase().as_str(), v.len()) {
        ("matrix", 6) => Transform::Matrix(Mat4([
            [n(0)?, n(2)?, 0.0, n(4)?],
            [n(1)?, n(3)?, 0.0, n(5)?],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])),
        ("matrix3d", 16) => {
            let mut m = Mat4::IDENTITY;
            for r in 0..4 {
                for c in 0..4 {
                    m.0[r][c] = n(c * 4 + r)?;
                }
            }
            Transform::Matrix(m)
        }
        ("translate", 1) => Transform::Translate([l(0, true)?, zero(), zero()]),
        ("translate", 2) => Transform::Translate([l(0, true)?, l(1, true)?, zero()]),
        ("translate3d", 3) => Transform::Translate([l(0, true)?, l(1, true)?, l(2, false)?]),
        ("translatex", 1) => Transform::Translate([l(0, true)?, zero(), zero()]),
        ("translatey", 1) => Transform::Translate([zero(), l(0, true)?, zero()]),
        ("translatez", 1) => Transform::Translate([zero(), zero(), l(0, false)?]),
        ("scale", 1) => Transform::Scale([n(0)?, n(0)?, 1.0]),
        ("scale", 2) => Transform::Scale([n(0)?, n(1)?, 1.0]),
        ("scale3d", 3) => Transform::Scale([n(0)?, n(1)?, n(2)?]),
        ("scalex", 1) => Transform::Scale([n(0)?, 1.0, 1.0]),
        ("scaley", 1) => Transform::Scale([1.0, n(0)?, 1.0]),
        ("scalez", 1) => Transform::Scale([1.0, 1.0, n(0)?]),
        ("rotate" | "rotatez", 1) => Transform::Rotate([0.0, 0.0, 1.0], a(0)?),
        ("rotatex", 1) => Transform::Rotate([1.0, 0.0, 0.0], a(0)?),
        ("rotatey", 1) => Transform::Rotate([0.0, 1.0, 0.0], a(0)?),
        ("rotate3d", 4) => Transform::Rotate([n(0)?, n(1)?, n(2)?], a(3)?),
        ("skew", 1) | ("skewx", 1) => Transform::Skew(a(0)?, 0.0),
        ("skew", 2) => Transform::Skew(a(0)?, a(1)?),
        ("skewy", 1) => Transform::Skew(0.0, a(0)?),
        ("perspective", 1) => Transform::Perspective(length(v[0], false, true)?),
        _ => return None,
    })
}

fn filters(v: &[Component]) -> Option<Vec<Filter>> {
    if v.len() == 1 && v[0].is_ident("none") {
        return Some(Vec::new());
    }
    if v.is_empty() || v.len() > 16 {
        return None;
    }
    v.iter()
        .map(|c| {
            let Component::Function(name, args) = c else {
                return None;
            };
            let v = components::from_tokens(args)?;
            if name.eq_ignore_ascii_case("drop-shadow") {
                let mut lengths = Vec::new();
                let mut color = None;
                for c in &v {
                    if let Some(l) = length(c, false, false) {
                        lengths.push(l);
                    } else if color.replace(c.color()?).is_some() {
                        return None;
                    }
                }
                if !(2..=3).contains(&lengths.len()) {
                    return None;
                }
                let blur = lengths.get(2).cloned().unwrap_or(Length::Px(0.0));
                if !matches!(blur, Length::Math(_)) && blur.resolve(16.0, 0.0) < 0.0 {
                    return None;
                }
                return Some(Filter::DropShadow {
                    x: lengths[0].clone(),
                    y: lengths[1].clone(),
                    blur,
                    color: color.unwrap_or(Color::CurrentColor),
                });
            }
            if v.len() > 1 {
                return None;
            }
            if name.eq_ignore_ascii_case("blur") {
                return Some(Filter::Blur(if v.is_empty() {
                    Length::Px(0.0)
                } else {
                    length(&v[0], false, true)?
                }));
            }
            if name.eq_ignore_ascii_case("hue-rotate") {
                return Some(Filter::HueRotate(if v.is_empty() {
                    0.0
                } else {
                    angle(&v[0])?
                }));
            }
            let n = if v.is_empty() { 1.0 } else { amount(&v[0])? };
            if n < 0.0 {
                return None;
            }
            Some(match name.to_ascii_lowercase().as_str() {
                "brightness" => Filter::Brightness(n),
                "contrast" => Filter::Contrast(n),
                "grayscale" => Filter::Grayscale(n.min(1.0)),
                "sepia" => Filter::Sepia(n.min(1.0)),
                "invert" => Filter::Invert(n.min(1.0)),
                "opacity" => Filter::Opacity(n.min(1.0)),
                "saturate" => Filter::Saturate(n),
                _ => return None,
            })
        })
        .collect()
}
fn radius(c: &Component) -> Option<Radius> {
    Some(if c.is_ident("closest-side") {
        Radius::Closest
    } else if c.is_ident("farthest-side") {
        Radius::Farthest
    } else {
        Radius::Length(length(c, true, true)?)
    })
}
fn shape(v: &[Component]) -> Option<Option<Shape>> {
    if v.len() == 1 && v[0].is_ident("none") {
        return Some(None);
    }
    let [Component::Function(name, args)] = v else {
        return None;
    };
    let v = components::from_tokens(args)?;
    Some(Some(match name.to_ascii_lowercase().as_str() {
        "inset" => {
            let lengths = v
                .iter()
                .map(|c| length(c, true, false))
                .collect::<Option<Vec<_>>>()?;
            Shape::Inset(crate::css::values::expand_four(&lengths)?)
        }
        "circle" | "ellipse" => {
            let at = v.iter().position(|c| c.is_ident("at")).unwrap_or(v.len());
            let o = if at < v.len() {
                origin(&v[at + 1..], false)?
            } else {
                Origin(
                    Length::Percent(50.0),
                    Length::Percent(50.0),
                    Length::Px(0.0),
                )
            };
            if name.eq_ignore_ascii_case("circle") {
                if at > 1 {
                    return None;
                }
                Shape::Circle(
                    if at == 0 {
                        Radius::Closest
                    } else {
                        radius(&v[0])?
                    },
                    o,
                )
            } else {
                if at != 0 && at != 2 {
                    return None;
                }
                Shape::Ellipse(
                    if at == 0 {
                        Radius::Closest
                    } else {
                        radius(&v[0])?
                    },
                    if at == 0 {
                        Radius::Closest
                    } else {
                        radius(&v[1])?
                    },
                    o,
                )
            }
        }
        "polygon" => {
            let mut p = v.split(comma).collect::<Vec<_>>();
            let mut evenodd = false;
            if let [c] = p[0]
                && (c.is_ident("evenodd") || c.is_ident("nonzero"))
            {
                evenodd = c.is_ident("evenodd");
                p.remove(0);
            }
            if p.len() < 3 || p.len() > 64 {
                return None;
            }
            Shape::Polygon {
                evenodd,
                points: p
                    .into_iter()
                    .map(|v| {
                        let [x, y] = v else { return None };
                        Some((length(x, true, false)?, length(y, true, false)?))
                    })
                    .collect::<Option<Vec<_>>>()?,
            }
        }
        _ => return None,
    }))
}
fn image(c: &Component) -> Option<MaskImage> {
    if c.is_ident("none") {
        return Some(MaskImage::None);
    }
    if let Component::Token(TokenKind::Url(u)) = c {
        return Some(MaskImage::Url(u.clone()));
    }
    let Component::Function(name, args) = c else {
        return None;
    };
    let v = components::from_tokens(args)?;
    if name.eq_ignore_ascii_case("url") {
        let [Component::Token(TokenKind::String(u))] = v.as_slice() else {
            return None;
        };
        return Some(MaskImage::Url(u.clone()));
    }
    if !name.eq_ignore_ascii_case("linear-gradient")
        && !name.eq_ignore_ascii_case("radial-gradient")
    {
        return None;
    }
    let mut p = parts(&v)?;
    let radial = name.eq_ignore_ascii_case("radial-gradient");
    let mut a = std::f64::consts::PI;
    if !radial {
        if let [c] = p[0]
            && let Some(angle) = angle(c)
        {
            a = angle;
            p.remove(0);
        } else if p[0].first().is_some_and(|c| c.is_ident("to")) {
            let d = &p.remove(0)[1..];
            a = match d {
                [c] if c.is_ident("right") => std::f64::consts::FRAC_PI_2,
                [c] if c.is_ident("left") => -std::f64::consts::FRAC_PI_2,
                [c] if c.is_ident("top") => 0.0,
                [c] if c.is_ident("bottom") => std::f64::consts::PI,
                _ => return None,
            };
        }
    } else if p[0]
        .iter()
        .any(|c| c.is_ident("ellipse") || c.is_ident("circle"))
    {
        if p[0].len() != 1 || !p[0][0].is_ident("ellipse") {
            return None;
        }
        p.remove(0);
    }
    if p.len() < 2 {
        return None;
    }
    let stops = p
        .into_iter()
        .map(|v| match v {
            [c] => Some((c.color()?, None)),
            [c, l] => Some((c.color()?, Some(length(l, true, false)?))),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(MaskImage::Gradient(Gradient {
        radial,
        angle: a,
        stops,
    }))
}
fn mask_box(c: &Component, clip: bool) -> Option<Keyword> {
    Some(match c.ident()?.to_ascii_lowercase().as_str() {
        "content-box" => Keyword::ContentBox,
        "padding-box" => Keyword::PaddingBox,
        "border-box" => Keyword::BorderBox,
        "no-clip" if clip => Keyword::NoClip,
        _ => return None,
    })
}
fn repeat(c: &Component) -> Option<Repeat> {
    Some(match c.ident()?.to_ascii_lowercase().as_str() {
        "repeat" => Repeat::Repeat,
        "no-repeat" => Repeat::NoRepeat,
        "space" => Repeat::Space,
        "round" => Repeat::Round,
        _ => return None,
    })
}

pub(crate) fn parse(p: Property, v: &[Component]) -> Option<Value> {
    use Property as P;
    if v.is_empty() {
        return None;
    }
    Some(match p {
        P::Transform => {
            if v.len() == 1 && v[0].is_ident("none") {
                Value::Transforms(Vec::new())
            } else if v.len() <= 64 {
                Value::Transforms(v.iter().map(transform).collect::<Option<Vec<_>>>()?)
            } else {
                return None;
            }
        }
        P::Translate | P::Scale | P::Rotate => {
            if v.len() == 1 && v[0].is_ident("none") {
                Value::Transforms(Vec::new())
            } else {
                Value::Transforms(vec![match p {
                    P::Translate if v.len() <= 3 => Transform::Translate([
                        length(&v[0], true, false)?,
                        v.get(1)
                            .map(|c| length(c, true, false))
                            .transpose_option()?
                            .unwrap_or(Length::Px(0.0)),
                        v.get(2)
                            .map(|c| length(c, false, false))
                            .transpose_option()?
                            .unwrap_or(Length::Px(0.0)),
                    ]),
                    P::Scale if v.len() <= 3 => Transform::Scale([
                        amount(&v[0])? as f64,
                        if v.len() > 1 {
                            amount(&v[1])? as f64
                        } else {
                            amount(&v[0])? as f64
                        },
                        if v.len() > 2 {
                            amount(&v[2])? as f64
                        } else {
                            1.0
                        },
                    ]),
                    P::Rotate => rotation(v)?,
                    _ => return None,
                }])
            }
        }
        P::TransformOrigin => Value::Origin(origin(v, true)?),
        P::PerspectiveOrigin => Value::Origin(origin(v, false)?),
        P::Opacity if v.len() == 1 => Value::Number(amount(&v[0])?.clamp(0.0, 1.0)),
        P::Perspective if v.len() == 1 => {
            if v[0].is_ident("none") {
                Value::Keyword(Keyword::None)
            } else {
                Value::Length(length(&v[0], false, true)?)
            }
        }
        P::Filter | P::BackdropFilter => Value::Filters(filters(v)?),
        P::ClipPath => Value::Shape(shape(v)?),
        P::Blend if v.len() == 1 => {
            Value::Blend(match v[0].ident()?.to_ascii_lowercase().as_str() {
                "normal" => Blend::Normal,
                "multiply" => Blend::Multiply,
                "screen" => Blend::Screen,
                "overlay" => Blend::Overlay,
                "darken" => Blend::Darken,
                "lighten" => Blend::Lighten,
                "color-dodge" => Blend::ColorDodge,
                "color-burn" => Blend::ColorBurn,
                "hard-light" => Blend::HardLight,
                "soft-light" => Blend::SoftLight,
                "difference" => Blend::Difference,
                "exclusion" => Blend::Exclusion,
                "hue" => Blend::Hue,
                "saturation" => Blend::Saturation,
                "color" => Blend::Color,
                "luminosity" => Blend::Luminosity,
                _ => return None,
            })
        }
        P::MaskImage => Value::Images(
            parts(v)?
                .iter()
                .map(|v| {
                    let [c] = *v else { return None };
                    image(c)
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        P::MaskMode => Value::Modes(
            parts(v)?
                .iter()
                .map(|v| {
                    let [c] = *v else { return None };
                    Some(match c.ident()?.to_ascii_lowercase().as_str() {
                        "match-source" => MaskMode::MatchSource,
                        "alpha" => MaskMode::Alpha,
                        "luminance" => MaskMode::Luminance,
                        _ => return None,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        P::MaskComposite => Value::Composites(
            parts(v)?
                .iter()
                .map(|v| {
                    let [c] = *v else { return None };
                    Some(match c.ident()?.to_ascii_lowercase().as_str() {
                        "add" => Composite::Add,
                        "subtract" => Composite::Subtract,
                        "intersect" => Composite::Intersect,
                        "exclude" => Composite::Exclude,
                        _ => return None,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        P::MaskOrigin | P::MaskClip => Value::Boxes(
            parts(v)?
                .iter()
                .map(|v| {
                    let [c] = *v else { return None };
                    mask_box(c, p == P::MaskClip)
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        P::MaskPosition => Value::Positions(
            parts(v)?
                .iter()
                .map(|v| super::box_values::position(v))
                .collect::<Option<Vec<_>>>()?,
        ),
        P::MaskSize => Value::Sizes(
            parts(v)?
                .iter()
                .map(|v| match *v {
                    [c] if c.is_ident("auto") => Some(MaskSize::Auto),
                    [c] if c.is_ident("contain") => Some(MaskSize::Contain),
                    [c] if c.is_ident("cover") => Some(MaskSize::Cover),
                    [x] => Some(MaskSize::Size(Some(length(x, true, true)?), None)),
                    [x, y] => Some(MaskSize::Size(
                        if x.is_ident("auto") {
                            None
                        } else {
                            Some(length(x, true, true)?)
                        },
                        if y.is_ident("auto") {
                            None
                        } else {
                            Some(length(y, true, true)?)
                        },
                    )),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        P::MaskRepeat => Value::Repeats(
            parts(v)?
                .iter()
                .map(|v| match *v {
                    [c] if c.is_ident("repeat-x") => Some((Repeat::Repeat, Repeat::NoRepeat)),
                    [c] if c.is_ident("repeat-y") => Some((Repeat::NoRepeat, Repeat::Repeat)),
                    [c] => Some((repeat(c)?, repeat(c)?)),
                    [x, y] => Some((repeat(x)?, repeat(y)?)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        P::TransformBox | P::TransformStyle | P::BackfaceVisibility | P::Isolation
            if v.len() == 1 =>
        {
            Value::Keyword(match (p, v[0].ident()?.to_ascii_lowercase().as_str()) {
                (P::TransformBox, "content-box") => Keyword::ContentBox,
                (P::TransformBox, "border-box") => Keyword::BorderBox,
                (P::TransformBox, "fill-box") => Keyword::FillBox,
                (P::TransformBox, "stroke-box") => Keyword::StrokeBox,
                (P::TransformBox, "view-box") => Keyword::ViewBox,
                (P::TransformStyle, "flat") => Keyword::Flat,
                (P::TransformStyle, "preserve-3d") => Keyword::Preserve3d,
                (P::BackfaceVisibility, "visible") => Keyword::Visible,
                (P::BackfaceVisibility, "hidden") => Keyword::Hidden,
                (P::Isolation, "auto") => Keyword::Auto,
                (P::Isolation, "isolate") => Keyword::Isolate,
                _ => return None,
            })
        }
        _ => return None,
    })
}

pub(crate) fn resolve_urls(v: &mut Value, base: &url::Url) {
    if let Value::Images(images) = v {
        for image in images {
            if let MaskImage::Url(reference) = image
                && let Ok(url) = base.join(reference)
            {
                *reference = url.into();
            }
        }
    }
}
pub(crate) fn mask(v: &[Component], properties: &[Property]) -> Option<Vec<Value>> {
    let mut layers = Vec::new();
    for v in parts(v)? {
        let mut values = BTreeMap::new();
        let mut position = Vec::new();
        let mut boxes = Vec::new();
        let mut i = 0;
        while i < v.len() {
            let c = &v[i];
            if slash(c) {
                if values.contains_key(&Property::MaskSize) || position.is_empty() {
                    return None;
                }
                i += 1;
                let start = i;
                while i < v.len()
                    && i - start < 2
                    && (v[i].is_ident("auto")
                        || v[i].is_ident("cover")
                        || v[i].is_ident("contain")
                        || length(&v[i], true, true).is_some())
                {
                    i += 1;
                }
                values.insert(Property::MaskSize, parse(Property::MaskSize, &v[start..i])?);
                continue;
            }
            let classified = [
                Property::MaskImage,
                Property::MaskMode,
                Property::MaskComposite,
            ]
            .into_iter()
            .find_map(|p| parse(p, std::slice::from_ref(c)).map(|v| (p, v)));
            if let Some((p, value)) = classified {
                if values.insert(p, value).is_some() {
                    return None;
                }
            } else if mask_box(c, true).is_some() {
                if boxes.len() == 2 {
                    return None;
                }
                boxes.push(c.clone());
            } else if repeat(c).is_some() || c.is_ident("repeat-x") || c.is_ident("repeat-y") {
                let start = i;
                if i + 1 < v.len() && repeat(&v[i + 1]).is_some() {
                    i += 1;
                }
                if values
                    .insert(
                        Property::MaskRepeat,
                        parse(Property::MaskRepeat, &v[start..=i])?,
                    )
                    .is_some()
                {
                    return None;
                }
            } else {
                if position.len() == 4 {
                    return None;
                }
                position.push(c.clone());
            }
            i += 1;
        }
        if !position.is_empty() {
            values.insert(
                Property::MaskPosition,
                parse(Property::MaskPosition, &position)?,
            );
        }
        if !boxes.is_empty() {
            let (origin, clipping) = if boxes[0].is_ident("no-clip") {
                if boxes.len() > 1 {
                    return None;
                }
                (
                    initial(Property::MaskOrigin),
                    parse(Property::MaskClip, &boxes)?,
                )
            } else {
                (
                    parse(Property::MaskOrigin, &boxes[..1])?,
                    parse(
                        Property::MaskClip,
                        if boxes.len() == 2 {
                            &boxes[1..]
                        } else {
                            &boxes[..1]
                        },
                    )?,
                )
            };
            values.insert(Property::MaskOrigin, origin);
            values.insert(Property::MaskClip, clipping);
        }
        layers.push(values);
    }
    properties
        .iter()
        .map(|&p| {
            let mut combined = initial(p);
            for (i, layer) in layers.iter().enumerate() {
                let value = layer.get(&p).cloned().unwrap_or_else(|| initial(p));
                if i == 0 {
                    combined = value;
                    continue;
                }
                match (&mut combined, value) {
                    (Value::Images(a), Value::Images(b)) => a.extend(b),
                    (Value::Modes(a), Value::Modes(b)) => a.extend(b),
                    (Value::Repeats(a), Value::Repeats(b)) => a.extend(b),
                    (Value::Sizes(a), Value::Sizes(b)) => a.extend(b),
                    (Value::Positions(a), Value::Positions(b)) => a.extend(b),
                    (Value::Boxes(a), Value::Boxes(b)) => a.extend(b),
                    (Value::Composites(a), Value::Composites(b)) => a.extend(b),
                    _ => return None,
                }
            }
            Some(combined)
        })
        .collect()
}

trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(v) => v.map(Some),
        }
    }
}

/// Compute selected-face/viewport/container units now, retaining box
/// percentages until final layout. Invalid calculations invalidate the whole
/// longhand (or pending shorthand), not just one function.
pub(crate) fn compute(mut v: Value, ctx: &LengthContext) -> Option<Value> {
    fn l(v: &mut Length, ctx: &LengthContext) -> Option<()> {
        *v = v.computed(ctx)?.0;
        Some(())
    }
    fn o(v: &mut Origin, ctx: &LengthContext) -> Option<()> {
        l(&mut v.0, ctx)?;
        l(&mut v.1, ctx)?;
        l(&mut v.2, ctx)
    }
    match &mut v {
        Value::Length(v) => l(v, ctx)?,
        Value::Origin(v) => o(v, ctx)?,
        Value::Transforms(v) => {
            for t in v {
                match t {
                    Transform::Translate(v) => {
                        for v in v {
                            l(v, ctx)?
                        }
                    }
                    Transform::Perspective(v) => l(v, ctx)?,
                    _ => {}
                }
            }
        }
        Value::Filters(v) => {
            for f in v {
                match f {
                    Filter::Blur(v) => l(v, ctx)?,
                    Filter::DropShadow { x, y, blur, .. } => {
                        l(x, ctx)?;
                        l(y, ctx)?;
                        l(blur, ctx)?
                    }
                    _ => {}
                }
            }
        }
        Value::Shape(Some(v)) => match v {
            Shape::Inset(v) => {
                for v in v {
                    l(v, ctx)?
                }
            }
            Shape::Polygon { points, .. } => {
                for (x, y) in points {
                    l(x, ctx)?;
                    l(y, ctx)?
                }
            }
            Shape::Circle(r, origin) => {
                if let Radius::Length(v) = r {
                    l(v, ctx)?
                }
                o(origin, ctx)?
            }
            Shape::Ellipse(x, y, origin) => {
                if let Radius::Length(v) = x {
                    l(v, ctx)?
                }
                if let Radius::Length(v) = y {
                    l(v, ctx)?
                }
                o(origin, ctx)?
            }
        },
        Value::Sizes(v) => {
            for s in v {
                if let MaskSize::Size(x, y) = s {
                    if let Some(x) = x {
                        l(x, ctx)?
                    }
                    if let Some(y) = y {
                        l(y, ctx)?
                    }
                }
            }
        }
        Value::Positions(v) => {
            for p in v {
                l(&mut p.x.offset, ctx)?;
                l(&mut p.y.offset, ctx)?
            }
        }
        Value::Images(v) => {
            for i in v {
                if let MaskImage::Gradient(g) = i {
                    for (_, s) in &mut g.stops {
                        if let Some(s) = s {
                            l(s, ctx)?
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Some(v)
}

pub(crate) fn compute_colors(
    mut v: Value,
    current: crate::css::values::ResolvedColor,
) -> Option<Value> {
    let color = |c: &mut Color| -> Option<()> {
        *c = Color::Absolute(c.computed(current)?.resolve(current)?);
        Some(())
    };
    match &mut v {
        Value::Filters(v) => {
            for f in v {
                if let Filter::DropShadow { color: c, .. } = f {
                    color(c)?;
                }
            }
        }
        Value::Images(v) => {
            for i in v {
                if let MaskImage::Gradient(g) = i {
                    for (c, _) in &mut g.stops {
                        color(c)?;
                    }
                }
            }
        }
        _ => {}
    }
    Some(v)
}

impl Shape {
    pub fn contains(&self, r: Rect, size: f32, x: f32, y: f32) -> bool {
        let (x, y) = (x - r.x, y - r.y);
        let radius = |v: &Radius, reference: f32, near: f32, far: f32| match v {
            Radius::Closest => near,
            Radius::Farthest => far,
            Radius::Length(l) => l.resolve(size, reference).max(0.0),
        };
        match self {
            Self::Inset(v) => {
                let [t, right, b, l] = std::array::from_fn::<_, 4, _>(|i| {
                    v[i].resolve(size, if i % 2 == 0 { r.height } else { r.width })
                });
                x >= l && x < r.width - right && y >= t && y < r.height - b
            }
            Self::Circle(rad, o) => {
                let (cx, cy) = (o.0.resolve(size, r.width), o.1.resolve(size, r.height));
                let rx = cx.abs().min((r.width - cx).abs());
                let ry = cy.abs().min((r.height - cy).abs());
                let far = cx
                    .abs()
                    .max((r.width - cx).abs())
                    .max(cy.abs().max((r.height - cy).abs()));
                let radius = radius(
                    rad,
                    (r.width * r.width + r.height * r.height).sqrt() / 2.0f32.sqrt(),
                    rx.min(ry),
                    far,
                );
                radius > 0.0 && (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
            }
            Self::Ellipse(rx, ry, o) => {
                let (cx, cy) = (o.0.resolve(size, r.width), o.1.resolve(size, r.height));
                let rx = radius(
                    rx,
                    r.width,
                    cx.abs().min((r.width - cx).abs()),
                    cx.abs().max((r.width - cx).abs()),
                );
                let ry = radius(
                    ry,
                    r.height,
                    cy.abs().min((r.height - cy).abs()),
                    cy.abs().max((r.height - cy).abs()),
                );
                rx > 0.0 && ry > 0.0 && ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
            }
            Self::Polygon { evenodd, points } => {
                let resolve = |(x, y): &(Length, Length)| {
                    (x.resolve(size, r.width), y.resolve(size, r.height))
                };
                let Some(mut a) = points.last().map(resolve) else {
                    return false;
                };
                let mut winding = 0i32;
                for b in points.iter().map(resolve) {
                    let cross = (b.0 - a.0) * (y - a.1) - (x - a.0) * (b.1 - a.1);
                    if a.1 <= y && b.1 > y && cross > 0.0 {
                        winding += 1;
                    }
                    if a.1 > y && b.1 <= y && cross < 0.0 {
                        winding -= 1;
                    }
                    a = b;
                }
                if *evenodd {
                    winding % 2 != 0
                } else {
                    winding != 0
                }
            }
        }
    }
}
