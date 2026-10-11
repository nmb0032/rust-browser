use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{
    css::selectors::PseudoElement,
    css::tokenizer::TokenKind,
    css::values::{
        BackgroundImage, BorderStyle, Color, ComputedLength, Edges, FontFamily, FontStyle,
        FontWeight, Length, LineHeight, ResolvedColor, SpecifiedLength,
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
    pub specified_colors: BTreeMap<String, Color>,
    /// Case-sensitive names and computed component tokens. `None` is the
    /// guaranteed-invalid value; absence has the same substitution semantics.
    pub custom_properties: BTreeMap<String, Option<Arc<[TokenKind]>>>,
    pub display: Display,
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
    pub line_height: LineHeight,
    /// Synthetic rendering flags; numeric weight and style remain typed above.
    pub bold: bool,
    pub italic: bool,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub margin: Edges<Length>,
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
            specified_colors: BTreeMap::new(),
            custom_properties: BTreeMap::new(),
            display: Display::Inline,
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
            color: parent.color,
            text_color: parent.text_color,
            text_fill_color: parent.text_fill_color,
            font_size: parent.font_size,
            font_family: parent.font_family.clone(),
            font_weight: parent.font_weight,
            font_style: parent.font_style,
            line_height: parent.line_height.clone(),
            bold: parent.bold,
            italic: parent.italic,
            border_color: Edges::all(parent.color),
            borders: Edges::all(parent.text_color),
            ..Self::default()
        }
    }

    pub fn used_line_height(&self) -> f32 {
        self.line_height.used(self.font_size)
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
