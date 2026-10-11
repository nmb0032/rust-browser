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

/// The inline base direction, used by paragraph bidi and logical geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnicodeBidi {
    #[default]
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WritingMode {
    #[default]
    HorizontalTb,
    VerticalRl,
    VerticalLr,
    SidewaysRl,
    SidewaysLr,
}

impl WritingMode {
    pub fn vertical(self) -> bool {
        self != Self::HorizontalTb
    }
    pub(crate) fn block_reverse(self) -> bool {
        matches!(self, Self::VerticalRl | Self::SidewaysRl)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOrientation {
    #[default]
    Mixed,
    Upright,
    Sideways,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignLast {
    #[default]
    Auto,
    Align(TextAlign),
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
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
    InlineStart,
    InlineEnd,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
    InlineStart,
    InlineEnd,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableLayout {
    #[default]
    Auto,
    Fixed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderCollapse {
    #[default]
    Separate,
    Collapse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptionSide {
    #[default]
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmptyCells {
    #[default]
    Show,
    Hide,
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
    Counter {
        name: String,
        style: String,
        separator: Option<String>,
    },
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
                ContentItem::Counter { .. } => {
                    report("counter content requires document-order counter resolution".into());
                    return None;
                }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOutside {
    Block,
    Inline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayInside {
    Flow,
    FlowRoot,
    Flex,
    Grid,
    Table,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    Block,
    FlowRoot,
    InlineBlock,
    Contents,
    InlineListItem,
    FlowRootListItem,
    InlineFlowRootListItem,
    #[default]
    Inline,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    Table,
    InlineTable,
    TableCaption,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableColumn,
    TableColumnGroup,
    ListItem,
    None,
}

impl Display {
    pub fn outside(self) -> Option<DisplayOutside> {
        if self.table_internal() || matches!(self, Self::None | Self::Contents) {
            return None;
        }
        Some(if self.inline_outer() {
            DisplayOutside::Inline
        } else {
            DisplayOutside::Block
        })
    }
    pub fn inside(self) -> Option<DisplayInside> {
        self.outside()?;
        Some(if self.is_flow_root() {
            DisplayInside::FlowRoot
        } else if self.is_flex() {
            DisplayInside::Flex
        } else if self.is_grid() {
            DisplayInside::Grid
        } else if self.is_table() {
            DisplayInside::Table
        } else {
            DisplayInside::Flow
        })
    }
    pub fn is_list_item(self) -> bool {
        matches!(
            self,
            Self::ListItem
                | Self::InlineListItem
                | Self::FlowRootListItem
                | Self::InlineFlowRootListItem
        )
    }
    pub fn is_flow_root(self) -> bool {
        matches!(
            self,
            Self::FlowRoot
                | Self::InlineBlock
                | Self::FlowRootListItem
                | Self::InlineFlowRootListItem
        )
    }
    pub fn inline_outer(self) -> bool {
        matches!(
            self,
            Self::Inline
                | Self::InlineBlock
                | Self::InlineListItem
                | Self::InlineFlowRootListItem
                | Self::InlineFlex
                | Self::InlineGrid
                | Self::InlineTable
        )
    }
    pub fn is_table(self) -> bool {
        matches!(self, Self::Table | Self::InlineTable)
    }
    pub(crate) fn table_internal(self) -> bool {
        matches!(
            self,
            Self::TableRowGroup
                | Self::TableHeaderGroup
                | Self::TableFooterGroup
                | Self::TableRow
                | Self::TableCell
                | Self::TableColumn
                | Self::TableColumnGroup
        )
    }
    pub(crate) fn row_group(self) -> bool {
        matches!(
            self,
            Self::TableRowGroup | Self::TableHeaderGroup | Self::TableFooterGroup
        )
    }
    pub fn is_grid(self) -> bool {
        matches!(self, Self::Grid | Self::InlineGrid)
    }
    pub(crate) fn is_item_container(self) -> bool {
        self.is_flex() || self.is_grid()
    }
    pub fn is_flex(self) -> bool {
        matches!(self, Self::Flex | Self::InlineFlex)
    }

    pub(crate) fn blockified(self) -> Self {
        match self {
            Self::Inline => Self::Block,
            Self::InlineBlock => Self::FlowRoot,
            Self::InlineListItem => Self::ListItem,
            Self::InlineFlowRootListItem => Self::FlowRootListItem,
            Self::InlineFlex => Self::Flex,
            Self::InlineGrid => Self::Grid,
            Self::InlineTable => Self::Table,
            Self::TableCaption
            | Self::TableRowGroup
            | Self::TableHeaderGroup
            | Self::TableFooterGroup
            | Self::TableRow
            | Self::TableCell
            | Self::TableColumn
            | Self::TableColumnGroup => Self::Block,
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirection {
    pub(crate) fn row(self) -> bool {
        matches!(self, Self::Row | Self::RowReverse)
    }
    pub(crate) fn reverse(self) -> bool {
        matches!(self, Self::RowReverse | Self::ColumnReverse)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexWrap {
    #[default]
    Nowrap,
    Wrap,
    WrapReverse,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum FlexBasis {
    #[default]
    Auto,
    Content,
    Length(Length),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignKeyword {
    Auto,
    #[default]
    Normal,
    Stretch,
    FlexStart,
    FlexEnd,
    Start,
    End,
    SelfStart,
    SelfEnd,
    Center,
    Baseline,
    LastBaseline,
    Left,
    Right,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AspectRatio {
    pub auto: bool,
    pub ratio: Option<f32>,
}
impl Default for AspectRatio {
    fn default() -> Self {
        Self {
            auto: true,
            ratio: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectAxis {
    pub from_end: bool,
    pub offset: Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectPosition {
    pub x: ObjectAxis,
    pub y: ObjectAxis,
}

impl Default for ObjectPosition {
    fn default() -> Self {
        let axis = ObjectAxis {
            from_end: false,
            offset: Length::Percent(50.0),
        };
        Self {
            x: axis.clone(),
            y: axis,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Alignment {
    pub keyword: AlignKeyword,
    pub safe: bool,
}

impl Alignment {
    pub const fn new(keyword: AlignKeyword) -> Self {
        Self {
            keyword,
            safe: false,
        }
    }
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
    pub motion: super::animation::Motion,
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
    pub float: Float,
    pub clear: Clear,
    pub table_layout: TableLayout,
    pub border_collapse: BorderCollapse,
    pub border_spacing: (Length, Length),
    pub caption_side: CaptionSide,
    pub empty_cells: EmptyCells,
    pub inset: Edges<LengthOrAuto>,
    pub z_index: ZIndex,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// Pre-blockification display used by CSS2's hypothetical static position.
    pub positioned_inline: bool,
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    pub writing_mode: WritingMode,
    pub text_orientation: TextOrientation,
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub typography: super::typography::Typography,
    pub effects: super::effects::Effects,
    pub decoration_sources: Vec<super::typography::Decoration>,
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
    pub aspect_ratio: AspectRatio,
    pub object_fit: ObjectFit,
    pub object_position: ObjectPosition,
    pub height: Option<Length>,
    /// Physical min/max prerequisites. Flex/grid determine content-based auto
    /// minima from item overflow, spanned tracks and preferred sizes.
    pub min_width: Option<Length>,
    pub min_height: Option<Length>,
    pub max_width: Option<Length>,
    pub max_height: Option<Length>,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: FlexBasis,
    pub grid_template_columns: Vec<super::GridTrack>,
    pub grid_template_rows: Vec<super::GridTrack>,
    pub grid_template_areas: Vec<Vec<Option<String>>>,
    pub grid_auto_columns: Vec<super::GridTrack>,
    pub grid_auto_rows: Vec<super::GridTrack>,
    pub grid_auto_flow: super::GridAutoFlow,
    pub grid_column_start: super::GridLine,
    pub grid_column_end: super::GridLine,
    pub grid_row_start: super::GridLine,
    pub grid_row_end: super::GridLine,
    pub order: i32,
    pub justify_content: Alignment,
    pub justify_items: Alignment,
    pub justify_self: Alignment,
    pub align_items: Alignment,
    pub align_self: Alignment,
    pub align_content: Alignment,
    /// `None` represents normal (zero in flex/grid layout).
    pub row_gap: Option<Length>,
    pub column_gap: Option<Length>,
    pub margin: Edges<LengthOrAuto>,
    pub padding: Edges<Length>,
    pub border_width: Edges<f32>,
    pub border_style: Edges<BorderStyle>,
    pub border_color: Edges<Option<u32>>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            motion: super::animation::Motion::default(),
            container_type: crate::css::container::ContainerType::Normal,
            container_names: Vec::new(),
            content: Content::Normal,
            specified_lengths: BTreeMap::new(),
            specified_margins: BTreeMap::new(),
            specified_colors: BTreeMap::new(),
            custom_properties: BTreeMap::new(),
            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            table_layout: TableLayout::Auto,
            border_collapse: BorderCollapse::Separate,
            border_spacing: (Length::Px(0.0), Length::Px(0.0)),
            caption_side: CaptionSide::Top,
            empty_cells: EmptyCells::Show,
            inset: Edges::all(LengthOrAuto::Auto),
            z_index: ZIndex::Auto,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            positioned_inline: false,
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            writing_mode: WritingMode::HorizontalTb,
            text_orientation: TextOrientation::Mixed,
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            typography: super::typography::Typography::default(),
            effects: super::effects::Effects::default(),
            decoration_sources: Vec::new(),
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
            aspect_ratio: AspectRatio::default(),
            object_fit: ObjectFit::default(),
            object_position: ObjectPosition::default(),
            height: None,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: FlexBasis::Auto,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_template_areas: Vec::new(),
            grid_auto_columns: vec![super::GridTrack::Size(super::TrackSize::default())],
            grid_auto_rows: vec![super::GridTrack::Size(super::TrackSize::default())],
            grid_auto_flow: super::GridAutoFlow::default(),
            grid_column_start: super::GridLine::Auto,
            grid_column_end: super::GridLine::Auto,
            grid_row_start: super::GridLine::Auto,
            grid_row_end: super::GridLine::Auto,
            order: 0,
            justify_content: Alignment::default(),
            justify_items: Alignment::default(),
            justify_self: Alignment::new(AlignKeyword::Auto),
            align_items: Alignment::default(),
            align_self: Alignment::new(AlignKeyword::Auto),
            align_content: Alignment::default(),
            row_gap: None,
            column_gap: None,
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
            writing_mode: parent.writing_mode,
            text_orientation: parent.text_orientation,
            border_collapse: parent.border_collapse,
            border_spacing: parent.border_spacing.clone(),
            caption_side: parent.caption_side,
            empty_cells: parent.empty_cells,
            object_position: parent.object_position.clone(),
            text_align: parent.text_align,
            text_align_last: parent.text_align_last,
            typography: parent.typography.inherit(),
            decoration_sources: parent.decoration_sources.clone(),
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
    pub(crate) counter_styles: Arc<crate::css::counter_style::CounterStyles>,
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
