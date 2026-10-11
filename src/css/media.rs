//! Bounded Media Queries conditions shared with size container queries.
//! Unsupported and unavailable features stay unknown, including under negation.

use super::tokenizer::{TokenKind, Tokenizer};
use super::values::{Dimension, Length, LengthContext, MathExpression};

pub const DEFAULT_VIEWPORT_WIDTH: f32 = 800.0;
pub const DEFAULT_VIEWPORT_HEIGHT: f32 = 600.0;
pub const INITIAL_FONT_SIZE: f32 = 16.0;
const MAX_CONDITION_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaType {
    #[default]
    Screen,
    Print,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    NoPreference,
    Reduce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contrast {
    NoPreference,
    More,
    Less,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    None,
    Coarse,
    Fine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hover {
    None,
    Hover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerCapabilities {
    pub coarse: bool,
    pub fine: bool,
}

/// Dimensions are CSS pixels, not framebuffer pixels. Font-relative query
/// lengths use the initial font size, independently of the document's styles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaEnvironment {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub media_type: MediaType,
    pub initial_font_size: f32,
    /// Initial-face ch/ex/cap/ic/normal-lh metrics, supplied by the text context.
    pub initial_font_metrics: Option<[f32; 5]>,
    /// None means the desktop viewport, where browser-chrome sizes coincide.
    pub small_viewport: Option<(f32, f32)>,
    pub large_viewport: Option<(f32, f32)>,
    pub dynamic_viewport: Option<(f32, f32)>,
    /// Unknown platform capabilities/preferences are not assumed false.
    pub resolution_dppx: Option<f32>,
    pub color_scheme: Option<ColorScheme>,
    pub reduced_motion: Option<Preference>,
    pub reduced_transparency: Option<Preference>,
    pub reduced_data: Option<Preference>,
    pub contrast: Option<Contrast>,
    pub forced_colors: Option<bool>,
    pub inverted_colors: Option<bool>,
    pub pointer: Option<Pointer>,
    pub any_pointer: Option<PointerCapabilities>,
    pub hover: Option<Hover>,
    pub any_hover: Option<Hover>,
    pub color_bits: Option<u32>,
    pub monochrome_bits: Option<u32>,
    pub color_index: Option<u32>,
}

impl MediaEnvironment {
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            viewport_width,
            viewport_height,
            ..Self::default()
        }
    }

    fn valid(self) -> bool {
        self.viewport_width.is_finite()
            && self.viewport_width >= 0.0
            && self.viewport_height.is_finite()
            && self.viewport_height >= 0.0
            && self.initial_font_size.is_finite()
            && self.initial_font_size > 0.0
            && self
                .resolution_dppx
                .is_none_or(|v| v.is_finite() && v > 0.0)
            && [
                self.small_viewport,
                self.large_viewport,
                self.dynamic_viewport,
            ]
            .into_iter()
            .flatten()
            .all(|(w, h)| w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0)
    }
}

impl Default for MediaEnvironment {
    fn default() -> Self {
        Self {
            viewport_width: DEFAULT_VIEWPORT_WIDTH,
            viewport_height: DEFAULT_VIEWPORT_HEIGHT,
            media_type: MediaType::Screen,
            initial_font_size: INITIAL_FONT_SIZE,
            initial_font_metrics: None,
            small_viewport: None,
            large_viewport: None,
            dynamic_viewport: None,
            resolution_dppx: None,
            color_scheme: None,
            reduced_motion: None,
            reduced_transparency: None,
            reduced_data: None,
            contrast: None,
            forced_colors: None,
            inverted_colors: None,
            pointer: None,
            any_pointer: None,
            hover: None,
            any_hover: None,
            color_bits: None,
            monochrome_bits: None,
            color_index: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Truth {
    False,
    True,
    Unknown,
}

impl Truth {
    fn not(self) -> Self {
        match self {
            Self::False => Self::True,
            Self::True => Self::False,
            Self::Unknown => Self::Unknown,
        }
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug)]
enum Condition {
    Constant(Truth),
    Type(String),
    Feature(Feature, FeatureTest),
    Not(Box<Self>),
    And(Vec<Self>),
    Or(Vec<Self>),
}

#[derive(Clone, Copy, Debug)]
enum Comparison {
    Equal,
    Min,
    Max,
    Less,
    Greater,
}

impl Comparison {
    fn reverse(self) -> Self {
        match self {
            Self::Equal => Self::Equal,
            Self::Min => Self::Max,
            Self::Max => Self::Min,
            Self::Less => Self::Greater,
            Self::Greater => Self::Less,
        }
    }

    fn compare(self, actual: f64, value: f64) -> bool {
        match self {
            Self::Equal => actual == value,
            Self::Min => actual >= value,
            Self::Max => actual <= value,
            Self::Less => actual < value,
            Self::Greater => actual > value,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Feature {
    Width,
    Height,
    InlineSize,
    BlockSize,
    AspectRatio,
    Orientation,
    Resolution,
    WebkitDevicePixelRatio,
    WebkitTransform3d,
    ColorScheme,
    ReducedMotion,
    ReducedTransparency,
    ReducedData,
    Contrast,
    ForcedColors,
    InvertedColors,
    Pointer,
    AnyPointer,
    Hover,
    AnyHover,
    Color,
    Monochrome,
    ColorIndex,
}

impl Feature {
    fn parse(name: &str, container: bool) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "width" => Self::Width,
            "height" => Self::Height,
            "inline-size" if container => Self::InlineSize,
            "block-size" if container => Self::BlockSize,
            "aspect-ratio" => Self::AspectRatio,
            "orientation" => Self::Orientation,
            _ if container => return None,
            "resolution" => Self::Resolution,
            "-webkit-device-pixel-ratio" => Self::WebkitDevicePixelRatio,
            "-webkit-transform-3d" => Self::WebkitTransform3d,
            "prefers-color-scheme" => Self::ColorScheme,
            "prefers-reduced-motion" => Self::ReducedMotion,
            "prefers-reduced-transparency" => Self::ReducedTransparency,
            "prefers-reduced-data" => Self::ReducedData,
            "prefers-contrast" => Self::Contrast,
            "forced-colors" => Self::ForcedColors,
            "inverted-colors" => Self::InvertedColors,
            "pointer" => Self::Pointer,
            "any-pointer" => Self::AnyPointer,
            "hover" => Self::Hover,
            "any-hover" => Self::AnyHover,
            "color" => Self::Color,
            "monochrome" => Self::Monochrome,
            "color-index" => Self::ColorIndex,
            _ => return None,
        })
    }

    fn discrete(self) -> bool {
        matches!(
            self,
            Self::Orientation
                | Self::WebkitTransform3d
                | Self::ColorScheme
                | Self::ReducedMotion
                | Self::ReducedTransparency
                | Self::ReducedData
                | Self::Contrast
                | Self::ForcedColors
                | Self::InvertedColors
                | Self::Pointer
                | Self::AnyPointer
                | Self::Hover
                | Self::AnyHover
        )
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Orientation => &["portrait", "landscape"],
            Self::ColorScheme => &["light", "dark"],
            Self::ReducedMotion | Self::ReducedTransparency | Self::ReducedData => {
                &["no-preference", "reduce"]
            }
            Self::Contrast => &["no-preference", "more", "less", "custom"],
            Self::ForcedColors => &["none", "active"],
            Self::InvertedColors => &["none", "inverted"],
            Self::Pointer | Self::AnyPointer => &["none", "coarse", "fine"],
            Self::Hover | Self::AnyHover => &["none", "hover"],
            _ => &[],
        }
    }

    fn numeric(self, context: &Evaluation) -> Option<f64> {
        let (w, h) = context.size;
        let env = context.environment;
        Some(match self {
            Self::Width | Self::InlineSize => f64::from(w),
            Self::Height | Self::BlockSize => f64::from(h),
            // A zero-height viewport has infinite aspect ratio.
            Self::AspectRatio if w == 0.0 && h == 0.0 => 1.0,
            Self::AspectRatio if h == 0.0 => f64::INFINITY,
            Self::AspectRatio => f64::from(w) / f64::from(h),
            Self::Resolution | Self::WebkitDevicePixelRatio => f64::from(env.resolution_dppx?),
            Self::Color => f64::from(env.color_bits?),
            Self::Monochrome => f64::from(env.monochrome_bits?),
            Self::ColorIndex => f64::from(env.color_index?),
            _ => return None,
        })
    }

    fn keyword_matches(self, keyword: &str, context: &Evaluation) -> Option<bool> {
        let env = context.environment;
        let preference = |value: Option<Preference>| {
            value.map(|v| match v {
                Preference::Reduce => "reduce",
                Preference::NoPreference => "no-preference",
            })
        };
        let actual = match self {
            Self::Orientation => {
                if context.size.0 > context.size.1 {
                    "landscape"
                } else {
                    "portrait"
                }
            }
            Self::ColorScheme => match env.color_scheme? {
                ColorScheme::Light => "light",
                ColorScheme::Dark => "dark",
            },
            Self::ReducedMotion => preference(env.reduced_motion)?,
            Self::ReducedTransparency => preference(env.reduced_transparency)?,
            Self::ReducedData => preference(env.reduced_data)?,
            Self::Contrast => match env.contrast? {
                Contrast::NoPreference => "no-preference",
                Contrast::More => "more",
                Contrast::Less => "less",
                Contrast::Custom => "custom",
            },
            Self::ForcedColors => {
                if env.forced_colors? {
                    "active"
                } else {
                    "none"
                }
            }
            Self::InvertedColors => {
                if env.inverted_colors? {
                    "inverted"
                } else {
                    "none"
                }
            }
            Self::Pointer => match env.pointer? {
                Pointer::None => "none",
                Pointer::Coarse => "coarse",
                Pointer::Fine => "fine",
            },
            Self::AnyPointer => {
                let pointers = env.any_pointer?;
                return Some(match keyword {
                    "none" => !pointers.coarse && !pointers.fine,
                    "coarse" => pointers.coarse,
                    "fine" => pointers.fine,
                    _ => false,
                });
            }
            Self::Hover => match env.hover? {
                Hover::None => "none",
                Hover::Hover => "hover",
            },
            Self::AnyHover => match env.any_hover? {
                Hover::None => "none",
                Hover::Hover => "hover",
            },
            _ => return None,
        };
        Some(actual == keyword)
    }
}

#[derive(Clone, Debug)]
enum QueryValue {
    Length(Length),
    Scalar(std::sync::Arc<MathExpression>),
    Ratio(f64),
}

impl QueryValue {
    fn evaluate(&self, lengths: &LengthContext) -> Option<f64> {
        match self {
            Self::Length(length) => Some(f64::from(length.used(lengths, 0.0)?.0)),
            Self::Scalar(expression) => {
                let value = expression.evaluate64(lengths, 0.0)?;
                if expression.dimension == Dimension::RESOLUTION {
                    let value = value as f32;
                    value.is_finite().then_some(f64::from(value))
                } else {
                    Some(value)
                }
            }
            Self::Ratio(value) => value.is_finite().then_some(*value),
        }
    }
}

#[derive(Clone, Debug)]
enum FeatureTest {
    Boolean,
    Keyword(String),
    Range(Vec<(Comparison, QueryValue)>),
}

struct Evaluation {
    environment: MediaEnvironment,
    size: (f32, f32),
    lengths: LengthContext,
}

fn media_context(environment: MediaEnvironment) -> Evaluation {
    let viewport = (environment.viewport_width, environment.viewport_height);
    let [ch, ex, cap, ic, lh] = environment.initial_font_metrics.unwrap_or([
        environment.initial_font_size * 0.5,
        environment.initial_font_size * 0.5,
        environment.initial_font_size * 0.7,
        environment.initial_font_size,
        environment.initial_font_size * 1.25,
    ]);
    Evaluation {
        environment,
        size: viewport,
        lengths: LengthContext {
            font_size: environment.initial_font_size,
            root_font_size: environment.initial_font_size,
            zero_advance: ch,
            x_height: ex,
            cap_height: cap,
            ideographic_advance: ic,
            root_zero_advance: ch,
            root_x_height: ex,
            root_cap_height: cap,
            root_ideographic_advance: ic,
            line_height: lh,
            root_line_height: lh,
            small_viewport: environment.small_viewport.unwrap_or(viewport),
            large_viewport: environment.large_viewport.unwrap_or(viewport),
            dynamic_viewport: environment.dynamic_viewport.unwrap_or(viewport),
            ..LengthContext::default()
        },
    }
}

impl Condition {
    fn evaluate(&self, context: &Evaluation) -> Truth {
        let truth = |value| if value { Truth::True } else { Truth::False };
        match self {
            Self::Constant(value) => *value,
            Self::Type(name) => truth(
                name == "all"
                    || matches!(
                        (name.as_str(), context.environment.media_type),
                        ("screen", MediaType::Screen) | ("print", MediaType::Print)
                    ),
            ),
            Self::Feature(feature, test) => match test {
                FeatureTest::Boolean if feature.discrete() => {
                    if matches!(feature, Feature::Orientation) {
                        Truth::True
                    } else if matches!(feature, Feature::ColorScheme) {
                        if context.environment.color_scheme.is_some() {
                            Truth::True
                        } else {
                            Truth::Unknown
                        }
                    } else {
                        feature
                            .keyword_matches(feature.keywords()[0], context)
                            .map(|value| truth(!value))
                            .unwrap_or(Truth::Unknown)
                    }
                }
                FeatureTest::Boolean => feature
                    .numeric(context)
                    .map(|v| truth(v != 0.0))
                    .unwrap_or(Truth::Unknown),
                FeatureTest::Keyword(keyword) => feature
                    .keyword_matches(keyword, context)
                    .map(truth)
                    .unwrap_or(Truth::Unknown),
                FeatureTest::Range(comparisons) => {
                    let Some(actual) = feature.numeric(context) else {
                        return Truth::Unknown;
                    };
                    comparisons
                        .iter()
                        .fold(Truth::True, |result, (comparison, value)| {
                            result.and(
                                value
                                    .evaluate(&context.lengths)
                                    .map(|value| truth(comparison.compare(actual, value)))
                                    .unwrap_or(Truth::Unknown),
                            )
                        })
                }
            },
            Self::Not(condition) => condition.evaluate(context).not(),
            Self::And(conditions) => conditions.iter().fold(Truth::True, |value, condition| {
                value.and(condition.evaluate(context))
            }),
            Self::Or(conditions) => conditions.iter().fold(Truth::False, |value, condition| {
                value.or(condition.evaluate(context))
            }),
        }
    }

    fn axes(&self) -> (bool, bool) {
        match self {
            Self::Feature(Feature::Width | Feature::InlineSize, _) => (true, false),
            Self::Feature(Feature::Height | Feature::BlockSize, _) => (false, true),
            Self::Feature(Feature::AspectRatio | Feature::Orientation, _) => (true, true),
            Self::Not(condition) => condition.axes(),
            Self::And(conditions) | Self::Or(conditions) => {
                conditions.iter().fold((false, false), |(w, h), c| {
                    let (cw, ch) = c.axes();
                    (w || cw, h || ch)
                })
            }

            _ => (false, false),
        }
    }

    fn invalid_math(&self, context: &Evaluation) -> bool {
        match self {
            Self::Feature(_, FeatureTest::Range(values)) => values
                .iter()
                .any(|(_, value)| value.evaluate(&context.lengths).is_none()),
            Self::Not(condition) => condition.invalid_math(context),
            Self::And(conditions) | Self::Or(conditions) => conditions
                .iter()
                .any(|condition| condition.invalid_math(context)),
            _ => false,
        }
    }
}

/// A parsed list can be evaluated again on resize. Invalid members are retained
/// as nonmatching queries; other comma-separated members are unaffected.
#[derive(Clone, Debug)]
pub struct MediaQueryList {
    queries: Vec<Condition>,
    diagnostics: Vec<String>,
    empty: bool,
    invalid: bool,
    unsupported_container_predicate: bool,
}

impl MediaQueryList {
    /// Empty lists match all (the HTML media-attribute default). An empty
    /// `@media` prelude is rejected separately by the rule compiler.
    pub fn parse(source: &str) -> Self {
        Self::parse_internal(source, false)
    }

    pub(crate) fn parse_container(source: &str) -> Self {
        Self::parse_internal(source, true)
    }

    fn parse_internal(source: &str, container: bool) -> Self {
        let mut tokenizer = Tokenizer::default();
        tokenizer.feed(source);
        tokenizer.finish();
        let tokens = std::iter::from_fn(|| tokenizer.next_token()).collect::<Vec<_>>();
        let mut result = Self {
            queries: Vec::new(),
            diagnostics: Vec::new(),
            invalid: false,
            unsupported_container_predicate: container && tokens.iter().any(|token|
                matches!(&token.kind, TokenKind::Function(name) if name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("scroll-state"))),
            empty: tokens
                .iter()
                .all(|token| matches!(token.kind, TokenKind::Whitespace | TokenKind::Eof))
                && tokenizer.errors().is_empty(),
        };
        let mut start = 0;
        let mut depth = 0usize;
        let mut too_deep = false;
        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                TokenKind::OpenParen | TokenKind::Function(_) | TokenKind::OpenSquare => {
                    depth += 1;
                    too_deep |= depth > MAX_CONDITION_DEPTH;
                }
                TokenKind::CloseParen | TokenKind::CloseSquare => depth = depth.saturating_sub(1),
                _ => {}
            }
            if matches!(token.kind, TokenKind::Eof)
                || (depth == 0 && token.kind == TokenKind::Comma)
            {
                let kinds = query_tokens(
                    &tokens[start..index]
                        .iter()
                        .map(|token| token.kind.clone())
                        .collect::<Vec<_>>(),
                );
                let lexical_error = tokenizer.errors().iter().any(|error| {
                    error.position.byte_offset >= tokens[start].span.start.byte_offset
                        && error.position.byte_offset <= token.span.start.byte_offset
                });
                let query = if container
                    && (token.kind == TokenKind::Comma || start != 0 || kinds.is_empty())
                {
                    Err("container queries require one nonempty condition, not a query list".into())
                } else if kinds.is_empty()
                    && start == 0
                    && token.kind == TokenKind::Eof
                    && result.queries.is_empty()
                    && !lexical_error
                {
                    Ok(Condition::Constant(Truth::True))
                } else if lexical_error || too_deep {
                    Err("invalid or excessively nested media query".into())
                } else {
                    Parser {
                        tokens: &kinds,
                        position: 0,
                        diagnostics: &mut result.diagnostics,
                        container,
                    }
                    .query()
                };
                match query {
                    Ok(query) => result.queries.push(query),
                    Err(message) => {
                        result.invalid = true;
                        result.diagnostics.push(message);
                        result.queries.push(Condition::Constant(Truth::False));
                    }
                }
                start = index + 1;
                too_deep = false;
            }
        }
        result
    }

    pub fn matches(&self, environment: MediaEnvironment) -> bool {
        let context = media_context(environment);
        environment.valid()
            && self.queries.iter().any(|query| {
                !query.invalid_math(&context) && query.evaluate(&context) == Truth::True
            })
    }

    pub(crate) fn evaluation_diagnostics(&self, environment: MediaEnvironment) -> Vec<String> {
        let context = media_context(environment);
        self.queries
            .iter()
            .filter(|query| query.invalid_math(&context))
            .map(|_| "non-finite query calculation; query does not match".to_string())
            .collect()
    }

    pub(crate) fn container_evaluation(
        &self,
        environment: MediaEnvironment,
        size: (f32, f32),
        lengths: LengthContext,
    ) -> (bool, bool) {
        let context = Evaluation {
            environment,
            size,
            lengths,
        };
        let invalid_math = self
            .queries
            .iter()
            .any(|query| query.invalid_math(&context));
        let matched = !invalid_math
            && !self.unsupported_container_predicate
            && environment.valid()
            && size.0.is_finite()
            && size.1.is_finite()
            && size.0 >= 0.0
            && size.1 >= 0.0
            && self.queries.len() == 1
            && self.queries[0].evaluate(&context) == Truth::True;
        (matched, invalid_math)
    }

    pub(crate) fn required_axes(&self) -> (bool, bool) {
        self.queries.iter().fold((false, false), |(w, h), query| {
            let (qw, qh) = query.axes();
            (w || qw, h || qh)
        })
    }

    pub(crate) fn valid_condition(&self) -> bool {
        !self.invalid && !self.empty && self.queries.len() == 1
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }
}

struct Parser<'a> {
    tokens: &'a [TokenKind],
    position: usize,
    diagnostics: &'a mut Vec<String>,
    container: bool,
}

impl Parser<'_> {
    fn keyword(&mut self, name: &str) -> bool {
        if matches!(self.tokens.get(self.position), Some(TokenKind::Ident(value)) if value.eq_ignore_ascii_case(name))
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn query(mut self) -> Result<Condition, String> {
        if self.container {
            let condition = self.condition(true, 0)?;
            if self.position != self.tokens.len() {
                return Err("invalid container condition chain".into());
            }
            return Ok(condition);
        }
        let modifier = if self.keyword("not") {
            Some(true)
        } else if self.keyword("only") {
            Some(false)
        } else {
            None
        };
        let result = if let Some(TokenKind::Ident(name)) = self.tokens.get(self.position) {
            if ["and", "or", "not", "only"]
                .iter()
                .any(|keyword| name.eq_ignore_ascii_case(keyword))
            {
                return Err("invalid media type".into());
            }
            let mut conditions = vec![Condition::Type(name.to_ascii_lowercase())];
            self.position += 1;
            if self.keyword("and") {
                conditions.push(self.condition(false, 0)?);
            }
            let query = Condition::And(conditions);
            if modifier == Some(true) {
                Condition::Not(Box::new(query))
            } else {
                query
            }
        } else {
            if modifier == Some(false) {
                return Err("'only' requires a media type".into());
            }
            // A leading 'not' without a type negates one media-in-parens,
            // not an unparenthesized chain.
            if modifier == Some(true) {
                Condition::Not(Box::new(self.in_parens(0)?))
            } else {
                self.condition(true, 0)?
            }
        };
        if self.position != self.tokens.len() {
            return Err("invalid or unsupported media query syntax".into());
        }
        Ok(result)
    }

    fn condition(&mut self, allow_or: bool, depth: usize) -> Result<Condition, String> {
        if self.keyword("not") {
            return Ok(Condition::Not(Box::new(self.in_parens(depth)?)));
        }
        let first = self.in_parens(depth)?;
        let operator = if self.keyword("and") {
            Some(false)
        } else if allow_or && self.keyword("or") {
            Some(true)
        } else {
            None
        };
        let Some(or) = operator else { return Ok(first) };
        let mut conditions = vec![first, self.in_parens(depth)?];
        while self.keyword(if or { "or" } else { "and" }) {
            conditions.push(self.in_parens(depth)?);
        }
        Ok(if or {
            Condition::Or(conditions)
        } else {
            Condition::And(conditions)
        })
    }

    fn in_parens(&mut self, depth: usize) -> Result<Condition, String> {
        if depth >= MAX_CONDITION_DEPTH {
            return Err("media condition nesting limit exceeded".into());
        }
        let start = self.position;
        if !matches!(
            self.tokens.get(start),
            Some(TokenKind::OpenParen | TokenKind::Function(_))
        ) {
            return Err("expected a parenthesized media condition".into());
        }
        let mut stack = vec![TokenKind::CloseParen];
        let mut end = start + 1;
        while end < self.tokens.len() {
            match &self.tokens[end] {
                TokenKind::OpenParen | TokenKind::Function(_) => stack.push(TokenKind::CloseParen),
                TokenKind::OpenSquare => stack.push(TokenKind::CloseSquare),
                TokenKind::CloseParen | TokenKind::CloseSquare => {
                    if stack.pop().as_ref() != Some(&self.tokens[end]) {
                        return Err("unbalanced media condition".into());
                    }
                    if stack.is_empty() {
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        if !stack.is_empty() {
            return Err("unclosed media condition".into());
        }
        self.position = end + 1;
        if self.container
            && matches!(&self.tokens[start], TokenKind::Function(name)
            if name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("scroll-state"))
        {
            self.diagnostics
                .push("unsupported container style()/scroll-state() predicate".into());
            return Ok(Condition::Constant(Truth::Unknown));
        }
        let inner = &self.tokens[start + 1..end];
        if self.tokens[start] == TokenKind::OpenParen
            && (matches!(inner.first(), Some(TokenKind::Ident(name)) if !name.eq_ignore_ascii_case("not"))
                || !range_operators(inner).is_empty())
        {
            return self.feature(inner);
        }
        if self.tokens[start] == TokenKind::OpenParen
            && (matches!(
                inner.first(),
                Some(TokenKind::OpenParen | TokenKind::Function(_))
            ) || matches!(inner.first(), Some(TokenKind::Ident(name)) if name.eq_ignore_ascii_case("not")))
        {
            let mut nested = Parser {
                tokens: inner,
                position: 0,
                diagnostics: self.diagnostics,
                container: self.container,
            };
            let condition = nested.condition(true, depth + 1)?;
            if nested.position != inner.len() {
                return Err("invalid media condition chain".into());
            }
            return Ok(condition);
        }
        if inner.is_empty() {
            return Err("empty media condition".into());
        }
        self.diagnostics.push("unsupported media condition".into());
        Ok(Condition::Constant(Truth::Unknown))
    }

    fn feature(&mut self, inner: &[TokenKind]) -> Result<Condition, String> {
        if !self.container
            && matches!(inner.first(), Some(TokenKind::Ident(name)) if name.eq_ignore_ascii_case("-webkit-transform-3d"))
        {
            // The renderer has no 3D transforms. This is a known false
            // capability, not an unknown device capability.
            let matched = match inner {
                [_] => false,
                [_, TokenKind::Colon, TokenKind::Number(number)]
                    if number.kind == super::tokenizer::NumericKind::Integer =>
                {
                    match number.value().ok() {
                        Some(0.0) => true,
                        Some(1.0) => false,
                        _ => return Err("invalid -webkit-transform-3d boolean".into()),
                    }
                }
                _ => return Err("invalid -webkit-transform-3d syntax".into()),
            };
            return Ok(Condition::Constant(if matched {
                Truth::True
            } else {
                Truth::False
            }));
        }
        let operators = range_operators(inner);
        if !operators.is_empty() {
            if operators.len() > 2 {
                return Err("too many media range comparisons".into());
            }
            let mut operands = Vec::new();
            let mut start = 0;
            for &(index, count, _) in &operators {
                operands.push(&inner[start..index]);
                start = index + count;
            }
            operands.push(&inner[start..]);
            let feature_index = if operators.len() == 2 {
                1
            } else if matches!(operands[0], [TokenKind::Ident(_)]) {
                0
            } else {
                1
            };
            let [TokenKind::Ident(name)] = operands[feature_index] else {
                return Err("media range requires one feature name".into());
            };
            let Some(feature) = Feature::parse(name, self.container) else {
                self.diagnostics
                    .push(format!("unsupported media feature: {name}"));
                return Ok(Condition::Constant(Truth::Unknown));
            };
            if feature.discrete() {
                return Err("range comparison is not allowed on a discrete feature".into());
            }
            if operators.len() == 2 {
                let direction = |op| match op {
                    Comparison::Less | Comparison::Max => -1,
                    Comparison::Greater | Comparison::Min => 1,
                    Comparison::Equal => 0,
                };
                if direction(operators[0].2) == 0
                    || direction(operators[0].2) != direction(operators[1].2)
                {
                    return Err("chained range comparisons must point in the same direction".into());
                }
            }
            let mut comparisons = Vec::new();
            for (index, &(_, _, comparison)) in operators.iter().enumerate() {
                let value_index = if feature_index == index {
                    index + 1
                } else {
                    index
                };
                let comparison = if feature_index == index {
                    comparison
                } else {
                    comparison.reverse()
                };
                comparisons.push((
                    comparison,
                    query_value(feature, operands[value_index], self.container)?,
                ));
            }
            return Ok(Condition::Feature(feature, FeatureTest::Range(comparisons)));
        }
        let Some(TokenKind::Ident(name)) = inner.first() else {
            return Err("invalid media feature".into());
        };
        let lower = name.to_ascii_lowercase();
        let (name, comparison, prefixed) =
            if !self.container && lower == "-webkit-min-device-pixel-ratio" {
                ("-webkit-device-pixel-ratio", Comparison::Min, true)
            } else if !self.container && lower == "-webkit-max-device-pixel-ratio" {
                ("-webkit-device-pixel-ratio", Comparison::Max, true)
            } else if let Some(name) = lower.strip_prefix("min-") {
                if name.starts_with("-webkit-") {
                    return Err("min- does not apply to -webkit- media features".into());
                }
                (name, Comparison::Min, true)
            } else if let Some(name) = lower.strip_prefix("max-") {
                if name.starts_with("-webkit-") {
                    return Err("max- does not apply to -webkit- media features".into());
                }
                (name, Comparison::Max, true)
            } else {
                (lower.as_str(), Comparison::Equal, false)
            };
        let Some(feature) = Feature::parse(name, self.container) else {
            self.diagnostics
                .push(format!("unsupported media feature: {name}"));
            return Ok(Condition::Constant(Truth::Unknown));
        };
        if inner.len() == 1 && !prefixed {
            return Ok(Condition::Feature(feature, FeatureTest::Boolean));
        }
        if inner.get(1) != Some(&TokenKind::Colon)
            || inner.len() <= 2
            || prefixed && feature.discrete()
        {
            return Err("invalid media feature value".into());
        }
        let test = if feature.discrete() {
            let [TokenKind::Ident(keyword)] = &inner[2..] else {
                return Err("invalid discrete media feature value".into());
            };
            let keyword = keyword.to_ascii_lowercase();
            if !feature.keywords().contains(&keyword.as_str()) {
                return Err("invalid discrete media feature keyword".into());
            }
            FeatureTest::Keyword(keyword)
        } else {
            FeatureTest::Range(vec![(
                comparison,
                query_value(feature, &inner[2..], self.container)?,
            )])
        };
        Ok(Condition::Feature(feature, test))
    }
}

fn query_value(
    feature: Feature,
    tokens: &[TokenKind],
    container: bool,
) -> Result<QueryValue, String> {
    let invalid = || {
        format!(
            "invalid {} media-query length or typed feature value",
            match feature {
                Feature::Width | Feature::InlineSize => "width",
                Feature::Height | Feature::BlockSize => "height",
                Feature::AspectRatio => "aspect-ratio",
                Feature::Resolution => "resolution",
                Feature::WebkitDevicePixelRatio => "-webkit-device-pixel-ratio",
                _ => "numeric",
            }
        )
    };
    if matches!(
        feature,
        Feature::Width | Feature::Height | Feature::InlineSize | Feature::BlockSize
    ) {
        if !container && tokens.iter().any(|t| matches!(t, TokenKind::Dimension { unit, .. } if unit.to_ascii_lowercase().starts_with("cq"))) {
            return Err("container units are not allowed in media queries".into());
        }
        let length = Length::from_tokens(tokens)
            .filter(|l| !l.has_percentage())
            .ok_or_else(invalid)?;
        return Ok(QueryValue::Length(length));
    }
    if matches!(feature, Feature::AspectRatio)
        && let [
            TokenKind::Number(a),
            TokenKind::Delim('/'),
            TokenKind::Number(b),
        ] = tokens
    {
        let (a, b) = (
            a.value().map_err(|_| invalid())?,
            b.value().map_err(|_| invalid())?,
        );
        if a.is_finite() && b.is_finite() && a >= 0.0 && b > 0.0 && (a / b).is_finite() {
            return Ok(QueryValue::Ratio(a / b));
        }
        return Err(invalid());
    }
    let dimension = if matches!(feature, Feature::Resolution) {
        Dimension::RESOLUTION
    } else {
        Dimension::NUMBER
    };
    let expression = MathExpression::from_tokens(tokens, None)
        .filter(|e| e.dimension == dimension)
        .ok_or_else(invalid)?;
    if tokens
        .iter()
        .any(|t| matches!(t, TokenKind::Dimension { .. } | TokenKind::Percentage(_)))
        && dimension == Dimension::NUMBER
    {
        // Ratios and integer capabilities do not accept dimension cancellation.
        return Err(invalid());
    }
    if matches!(
        feature,
        Feature::Color | Feature::Monochrome | Feature::ColorIndex | Feature::AspectRatio
    ) {
        let value = expression
            .evaluate64(&LengthContext::default(), 0.0)
            .ok_or_else(invalid)?;
        if value < 0.0 || !matches!(feature, Feature::AspectRatio) && value.fract() != 0.0 {
            return Err(invalid());
        }
    }
    Ok(QueryValue::Scalar(expression))
}

fn range_operators(tokens: &[TokenKind]) -> Vec<(usize, usize, Comparison)> {
    let mut depth = 0usize;
    let mut result = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            TokenKind::OpenParen | TokenKind::Function(_) => depth += 1,
            TokenKind::CloseParen => depth = depth.saturating_sub(1),
            TokenKind::Delim(c @ ('<' | '>' | '=')) if depth == 0 => {
                let equal = c != '=' && tokens.get(index + 1) == Some(&TokenKind::Delim('='));
                let comparison = match (c, equal) {
                    ('<', false) => Comparison::Less,
                    ('<', true) => Comparison::Max,
                    ('>', false) => Comparison::Greater,
                    ('>', true) => Comparison::Min,
                    _ => Comparison::Equal,
                };
                let count = if equal { 2 } else { 1 };
                result.push((index, count, comparison));
                index += count;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    result
}

fn query_tokens(tokens: &[TokenKind]) -> Vec<TokenKind> {
    let mut depth = 0usize;
    let mut function = None;
    let mut result = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if let TokenKind::Function(_) = token {
            depth += 1;
            function.get_or_insert(depth);
        } else if *token == TokenKind::OpenParen {
            depth += 1;
        } else if *token == TokenKind::CloseParen {
            depth = depth.saturating_sub(1);
            if function.is_some_and(|start| depth < start) {
                function = None;
            }
        }
        let split_operator = matches!(result.last(), Some(TokenKind::Delim('<' | '>')))
            && tokens.get(index + 1) == Some(&TokenKind::Delim('='));
        if *token != TokenKind::Whitespace || function.is_some() || split_operator {
            result.push(token.clone());
        }
    }
    result
}

#[cfg(test)]
#[path = "media_compat_tests.rs"]
mod compat_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(source: &str, width: f32) -> bool {
        let query = MediaQueryList::parse(source);
        assert!(
            query.diagnostics().is_empty(),
            "{source}: {:?}",
            query.diagnostics()
        );
        query.matches(MediaEnvironment::new(width, 600.0))
    }

    #[test]
    fn width_boundaries_are_inclusive_and_boolean_width_checks_zero() {
        for (source, expected) in [
            ("(width: 600px)", [false, true, false]),
            ("(min-width: 600px)", [false, true, true]),
            ("(max-width: 600px)", [true, true, false]),
        ] {
            for (width, expected) in [599.5, 600.0, 600.5].into_iter().zip(expected) {
                assert_eq!(matches(source, width), expected, "{source} at {width}");
            }
        }
        assert!(!matches("(width)", 0.0));
        assert!(matches("(width)", 1.0));
        assert!(matches("(width:0)", 0.0));
        assert!(matches("(min-width: -1px)", 0.0));
        assert!(!matches("(max-width: -1px)", 0.0));
    }

    #[test]
    fn media_types_modifiers_and_query_lists_follow_boolean_semantics() {
        for (query, expected) in [
            ("", true),
            ("/* comment */", true),
            ("screen", true),
            ("ALL", true),
            ("print", false),
            ("speech", false),
            ("unknown-type", false),
            ("not unknown-type", true),
            ("not screen", false),
            ("only SCREEN and (min-width:600px)", true),
            ("not screen and (min-width:601px)", true),
            ("print, (min-width:600px)", true),
            ("(min-width:601px), (max-width:599px)", false),
            ("screen and (min-width:500px) and (max-width:700px)", true),
            ("screen and ((min-width:700px) or (width:600px))", true),
            ("(width:601px) or (width:600px)", true),
            ("not (width:601px)", true),
            ("not ((width:600px) and (min-width:500px))", false),
            ("((width:600px) and (not (width:601px)))", true),
        ] {
            assert_eq!(matches(query, 600.0), expected, "{query}");
        }
        let print = MediaEnvironment {
            media_type: MediaType::Print,
            ..MediaEnvironment::new(600.0, 600.0)
        };
        assert!(MediaQueryList::parse("only print and (width:600px)").matches(print));
        assert!(!MediaQueryList::parse("screen").matches(print));
    }

    #[test]
    fn font_relative_and_absolute_lengths_use_the_environment_not_document_styles() {
        for query in [
            "(width:37.5em)",
            "(width:37.5rem)",
            r"(W\69 DTH:6e2PX)",
            "(width/**/:/**/600px)",
        ] {
            assert!(matches(query, 600.0), "{query}");
        }
        let environment = MediaEnvironment {
            initial_font_size: 20.0,
            ..MediaEnvironment::new(600.0, 600.0)
        };
        assert!(MediaQueryList::parse("(width:30em)").matches(environment));
        for query in [
            "(width:1in)",
            "(width:2.54cm)",
            "(width:25.4mm)",
            "(width:101.6q)",
            "(width:72pt)",
            "(width:6pc)",
        ] {
            assert!(matches(query, 96.0), "{query}");
        }
    }

    #[test]
    fn invalid_queries_cannot_match_even_under_not_and_other_list_members_survive() {
        for source in [
            "only (width:600px)",
            "only",
            "not",
            "and (width:600px)",
            "screen or (width:600px)",
            "not (width:601px) and (width:600px)",
            "(width:600px) and (width) or (width)",
            "(min-width)",
            "(width:)",
            "(width:600)",
            "(width:600 px)",
            "(width:-)",
            "(width:1e999px)",
            "not (width:100%)",
            "()",
            "(width:600px",
            "(width:600px))",
            "screen and(width:600px)",
            "screen,",
            ",",
        ] {
            let query = MediaQueryList::parse(source);
            assert!(!query.diagnostics().is_empty(), "{source}");
            // "screen," keeps its valid first member.
            assert_eq!(
                query.matches(MediaEnvironment::default()),
                source == "screen,",
                "{source}"
            );
        }
        let query = MediaQueryList::parse("(width:), screen and (width:600px), only");
        assert_eq!(query.diagnostics().len(), 2);
        assert!(query.matches(MediaEnvironment::new(600.0, 600.0)));
        let query = MediaQueryList::parse("screen, \"broken\n");
        assert!(query.matches(MediaEnvironment::default()));
        assert!(!query.diagnostics().is_empty());
    }

    #[test]
    fn unsupported_features_and_general_enclosed_stay_unknown_under_negation() {
        for source in [
            "(future-height:600px)",
            "not (future-height:600px)",
            "not future(feature)",
            "(600px < future-width)",
            "not (600px < future-width)",
            "(future-width >= 600px)",
            "not (future-width >= 600px)",
            "not ((future-height:600px) or (width:601px))",
        ] {
            let query = MediaQueryList::parse(source);
            assert!(!query.diagnostics().is_empty(), "{source}");
            assert!(
                !query.matches(MediaEnvironment::new(600.0, 600.0)),
                "{source}"
            );
        }
        for source in [
            "(height:600px), (width:600px)",
            "(height:600px) or (width:600px)",
            "((height:600px) or (width:600px))",
            "(future(feature) or (width:600px))",
        ] {
            assert!(
                MediaQueryList::parse(source).matches(MediaEnvironment::new(600.0, 600.0)),
                "{source}"
            );
        }
    }

    #[test]
    fn parsing_retains_queries_for_reevaluation_and_bounds_deep_input() {
        let query = MediaQueryList::parse("(max-width:600px)");
        assert!(query.matches(MediaEnvironment::new(400.0, 600.0)));
        assert!(!query.matches(MediaEnvironment::new(800.0, 600.0)));
        let source = format!("{}width:600px{}", "(".repeat(200), ")".repeat(200));
        let query = MediaQueryList::parse(&source);
        assert!(!query.matches(MediaEnvironment::default()));
        assert_eq!(query.diagnostics().len(), 1);
        assert!(!MediaQueryList::parse("screen").matches(MediaEnvironment::new(f32::NAN, 600.0)));
        assert!(!MediaQueryList::parse("screen").matches(MediaEnvironment::new(-1.0, 600.0)));
    }

    #[test]
    fn ranges_chains_case_math_and_derived_features() {
        let environment = MediaEnvironment {
            resolution_dppx: Some(2.0),
            ..MediaEnvironment::new(600.0, 400.0)
        };
        for source in [
            "(width >= 600px)",
            "(600px <= WIDTH)",
            "(601px > width >= 600px)",
            "(599px < width <= 600px)",
            "(600px <= width < 601px)",
            "(600px = width)",
            "(HEIGHT:25em)",
            "(height > 399px)",
            "(orientation:landscape)",
            "(aspect-ratio:3/2)",
            "(1 < aspect-ratio < 2)",
            "(width = calc(50vw + 300px))",
            "(width = min(600px, 700px))",
            "(height = clamp(300px, 25rem, 500px))",
            "(resolution:192dpi)",
            "(resolution:75.5905511811dpcm)",
            "(1dppx < resolution <= 2x)",
            "(resolution = calc(1dppx * 2))",
            "(width >= -1px)",
        ] {
            let query = MediaQueryList::parse(source);
            assert!(
                query.diagnostics().is_empty(),
                "{source}: {:?}",
                query.diagnostics()
            );
            assert!(query.matches(environment), "{source}");
        }
        for source in [
            "(width > 600px)",
            "(600px > width)",
            "(height >= 401px)",
            "(orientation:portrait)",
            "(aspect-ratio > 1.5)",
            "(resolution < 2dppx)",
        ] {
            assert!(
                !MediaQueryList::parse(source).matches(environment),
                "{source}"
            );
        }
        assert!(
            MediaQueryList::parse("(orientation:portrait)")
                .matches(MediaEnvironment::new(600.0, 600.0))
        );
        for source in [
            "not (600px < width > 800px)",
            "not (width > = 500px)",
            "not (width == 600px)",
            "not (600px = width = 600px)",
            "not (width < height)",
            "not (orientation > portrait)",
            "not (aspect-ratio:1/0)",
            "not (aspect-ratio:-1)",
            "not (aspect-ratio:1e300/1e-300)",
            "not (prefers-color-scheme:sepia)",
            "not (width:calc(100% + 1px))",
            "not (width:calc(1px+2px))",
            "not (width:var(--width))",
        ] {
            let query = MediaQueryList::parse(source);
            assert!(!query.diagnostics().is_empty(), "{source}");
            assert!(!query.matches(environment), "{source}");
        }
        assert!(!MediaQueryList::parse("not (width:calc(1px / 0))").matches(environment));
        assert!(
            !MediaQueryList::parse("not ((width:calc(1px / 0)) and (width:0px))")
                .matches(environment)
        );
    }

    #[test]
    fn explicit_preferences_input_and_unavailable_features() {
        let environment = MediaEnvironment {
            color_scheme: Some(ColorScheme::Dark),
            reduced_motion: Some(Preference::Reduce),
            reduced_transparency: Some(Preference::NoPreference),
            reduced_data: Some(Preference::Reduce),
            contrast: Some(Contrast::More),
            forced_colors: Some(true),
            inverted_colors: Some(false),
            pointer: Some(Pointer::Fine),
            any_pointer: Some(PointerCapabilities {
                coarse: true,
                fine: true,
            }),
            hover: Some(Hover::Hover),
            any_hover: Some(Hover::Hover),
            color_bits: Some(8),
            monochrome_bits: Some(0),
            color_index: Some(0),
            ..MediaEnvironment::default()
        };
        for source in [
            "(prefers-color-scheme:dark)",
            "(prefers-color-scheme)",
            "(prefers-reduced-motion:reduce)",
            "(prefers-reduced-motion)",
            "not (prefers-reduced-transparency)",
            "(prefers-reduced-data:reduce)",
            "(prefers-contrast:more)",
            "(forced-colors:active)",
            "not (inverted-colors)",
            "(pointer:fine)",
            "(any-pointer:coarse) and (any-pointer:fine)",
            "(hover:hover)",
            "(any-hover)",
            "(color >= 8)",
            "not (monochrome)",
            "(color-index:0)",
        ] {
            let query = MediaQueryList::parse(source);
            assert!(
                query.diagnostics().is_empty(),
                "{source}: {:?}",
                query.diagnostics()
            );
            assert!(query.matches(environment), "{source}");
        }
        for source in [
            "not (prefers-color-scheme:dark)",
            "(prefers-color-scheme)",
            "not (prefers-reduced-motion:reduce)",
            "not (forced-colors:active)",
            "not (pointer:coarse)",
            "not (hover)",
            "not (resolution > 1dppx)",
            "not (color:8)",
        ] {
            assert!(
                !MediaQueryList::parse(source).matches(MediaEnvironment::default()),
                "{source}"
            );
        }
    }
}
