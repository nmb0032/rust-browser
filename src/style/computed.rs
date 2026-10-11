use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{
    css::selectors::PseudoElement,
    css::tokenizer::TokenKind,
    css::values::{
        BackgroundImage, BorderStyle, Color, ComputedLength, Edges, FontFamily, FontStyle,
        FontWeight, Length, LengthOrAuto, LineHeight, ResolvedColor, SpecifiedLength,
    },
    dom::{ElementData, NodeId},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Content {
    #[default]
    Normal,
    None,
    Items(Vec<ContentItem>),
}

/// Used by the horizontal width equation and logical text alignment.
/// Text reordering and logical box properties are not implemented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignLast {
    #[default]
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpaceCollapse {
    #[default]
    Collapse,
    Preserve,
    PreserveBreaks,
    BreakSpaces,
}

impl WhiteSpaceCollapse {
    pub(crate) fn preserves_spaces(self) -> bool {
        matches!(self, Self::Preserve | Self::BreakSpaces)
    }

    pub(crate) fn preserves_breaks(self) -> bool {
        self != Self::Collapse
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    #[default]
    Wrap,
    Nowrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaselineShift {
    #[default]
    Zero,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaselineSource {
    #[default]
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentItem {
    Text(String),
    Attribute(String),
}

pub(crate) const MAX_GENERATED_TEXT_BYTES: usize = 65_536;

impl Content {
    /// Bounded generated text. `normal` and `none` do not generate a box.
    /// Oversized content is omitted with a diagnostic; a missing attribute
    /// contributes empty text without a diagnostic.
    pub fn text(&self, element: &ElementData) -> Option<String> {
        self.text_with_diagnostic(element, |message| eprintln!("CSS: {message}"))
    }

    pub(crate) fn text_with_diagnostic(
        &self,
        element: &ElementData,
        report: impl FnOnce(String),
    ) -> Option<String> {
        let Self::Items(items) = self else {
            return None;
        };
        let mut text = String::new();
        for item in items {
            let part = match item {
                ContentItem::Text(text) => text.as_str(),
                ContentItem::Attribute(name) => element.attribute(name).unwrap_or(""),
            };
            if text
                .len()
                .checked_add(part.len())
                .is_none_or(|len| len > MAX_GENERATED_TEXT_BYTES)
            {
                report(format!(
                    "generated text exceeds the {MAX_GENERATED_TEXT_BYTES}-byte limit; content omitted"
                ));
                return None;
            }
            text.push_str(part);
        }
        Some(text)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    Block,
    #[default]
    Inline,
    ListItem,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

impl Position {
    pub fn out_of_flow(self) -> bool {
        matches!(self, Self::Absolute | Self::Fixed)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZIndex {
    #[default]
    Auto,
    Integer(i32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

impl Overflow {
    pub fn scroll_container(self) -> bool {
        matches!(self, Self::Hidden | Self::Scroll | Self::Auto)
    }

    pub fn user_scrollable(self) -> bool {
        matches!(self, Self::Scroll | Self::Auto)
    }

    pub fn clips(self) -> bool {
        self != Self::Visible
    }

    pub(crate) fn coupled(self, other: Self) -> Self {
        if other.scroll_container() {
            match self {
                Self::Visible => Self::Auto,
                Self::Clip => Self::Hidden,
                _ => self,
            }
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

impl BoxSizing {
    pub(crate) fn content_size(self, specified: f32, decorations: f32) -> f32 {
        match self {
            Self::ContentBox => specified.max(0.0),
            Self::BorderBox => (specified - decorations).max(0.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub container_type: crate::css::container::ContainerType,
    pub container_names: Vec<String>,
    pub content: Content,
    /// Winning, substituted length values before relative-unit computation.
    pub specified_lengths: BTreeMap<String, SpecifiedLength>,
    /// Substituted margin values before relative-unit computation, including auto.
    pub specified_margins: BTreeMap<String, LengthOrAuto>,
    pub specified_colors: BTreeMap<String, Color>,
    /// Case-sensitive names and computed component tokens. `None` is the
    /// guaranteed-invalid value; absence has the same substitution semantics.
    pub custom_properties: BTreeMap<String, Option<Arc<[TokenKind]>>>,
    pub display: Display,
    pub position: Position,
    pub inset: Edges<LengthOrAuto>,
    pub z_index: ZIndex,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// Pre-blockification display used by CSS2's hypothetical static position.
    pub positioned_inline: bool,
    pub direction: Direction,
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub white_space_collapse: WhiteSpaceCollapse,
    pub text_wrap_mode: TextWrapMode,
    pub vertical_align: VerticalAlign,
    pub baseline_shift: BaselineShift,
    pub baseline_source: BaselineSource,
    pub box_sizing: BoxSizing,
    pub color: Option<u32>,
    pub background_color: Option<u32>,
    /// Authoritative colors; the u32 fields are compatibility sRGB projections.
    pub text_color: ResolvedColor,
    /// Text foreground fill, independent of color/currentcolor for box edges.
    pub text_fill_color: ResolvedColor,
    pub background: ResolvedColor,
    pub borders: Edges<ResolvedColor>,
    pub background_image: BackgroundImage,
    pub font_size: f32,
    pub font_family: FontFamily,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    /// Selected primary face; inherited by text, pseudos and anonymous boxes.
    pub font_face: Arc<crate::text::FontFace>,
    /// Inherited HTML language, used by the OpenType shaper.
    pub font_language: Option<String>,
    pub line_height: LineHeight,
    /// Synthetic rendering flags; numeric weight and style remain typed above.
    pub bold: bool,
    pub italic: bool,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub margin: Edges<LengthOrAuto>,
    pub padding: Edges<Length>,
    pub border_width: Edges<f32>,
    pub border_style: Edges<BorderStyle>,
    pub border_color: Edges<Option<u32>>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            container_type: crate::css::container::ContainerType::Normal,
            container_names: Vec::new(),
            content: Content::Normal,
            specified_lengths: BTreeMap::new(),
            specified_margins: BTreeMap::new(),
            specified_colors: BTreeMap::new(),
            custom_properties: BTreeMap::new(),
            display: Display::Inline,
            position: Position::Static,
            inset: Edges::all(LengthOrAuto::Auto),
            z_index: ZIndex::Auto,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            positioned_inline: false,
            direction: Direction::Ltr,
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            text_wrap_mode: TextWrapMode::Wrap,
            vertical_align: VerticalAlign::Baseline,
            baseline_shift: BaselineShift::Zero,
            baseline_source: BaselineSource::Auto,
            box_sizing: BoxSizing::ContentBox,
            color: Some(0),
            background_color: None,
            text_color: ResolvedColor::BLACK,
            text_fill_color: ResolvedColor::BLACK,
            background: ResolvedColor::TRANSPARENT,
            borders: Edges::all(ResolvedColor::BLACK),
            background_image: BackgroundImage::None,
            font_size: 16.0,
            font_family: FontFamily::default(),
            font_weight: FontWeight::default(),
            font_style: FontStyle::Normal,
            font_face: crate::text::bundled_face(),
            font_language: None,
            line_height: LineHeight::Normal,
            bold: false,
            italic: false,
            width: None,
            height: None,
            margin: Edges::default(),
            padding: Edges::default(),
            border_width: Edges::default(),
            border_style: Edges::all(BorderStyle::None),
            border_color: Edges::all(Some(0)),
        }
    }
}

impl ComputedStyle {
    pub fn computed_width(&self) -> Option<ComputedLength> {
        self.width.clone().map(ComputedLength)
    }

    pub fn specified_width(&self) -> Option<&SpecifiedLength> {
        self.specified_lengths.get("Width")
    }

    pub(crate) fn inherit(parent: &Self) -> Self {
        Self {
            custom_properties: parent.custom_properties.clone(),
            direction: parent.direction,
            text_align: parent.text_align,
            text_align_last: parent.text_align_last,
            white_space_collapse: parent.white_space_collapse,
            text_wrap_mode: parent.text_wrap_mode,
            color: parent.color,
            text_color: parent.text_color,
            text_fill_color: parent.text_fill_color,
            font_size: parent.font_size,
            font_family: parent.font_family.clone(),
            font_weight: parent.font_weight,
            font_style: parent.font_style,
            font_face: parent.font_face.clone(),
            font_language: parent.font_language.clone(),
            line_height: parent.line_height.clone(),
            bold: parent.bold,
            italic: parent.italic,
            border_color: Edges::all(parent.color),
            borders: Edges::all(parent.text_color),
            ..Self::default()
        }
    }

    pub fn used_line_height(&self) -> f32 {
        if self.line_height == LineHeight::Normal {
            self.font_face
                .horizontal_line_metrics(self.font_size)
                .map_or(self.font_size * 1.2, |metrics| metrics.new_line_size)
        } else {
            self.line_height.used(self.font_size)
        }
    }
}

#[derive(Clone, Debug)]
pub struct ComputedStyles {
    pub(super) nodes: HashMap<NodeId, ComputedStyle>,
    pub(super) pseudos: HashMap<(NodeId, PseudoElement), ComputedStyle>,
}

impl ComputedStyles {
    pub fn get(&self, node_id: NodeId) -> Option<&ComputedStyle> {
        self.nodes.get(&node_id)
    }

    /// Pseudo declarations never participate in the originating element's
    /// cascade. Styles inherit from that element, not its DOM parent.
    pub fn pseudo(&self, node_id: NodeId, pseudo: PseudoElement) -> Option<&ComputedStyle> {
        self.pseudos.get(&(node_id, pseudo))
    }
}
