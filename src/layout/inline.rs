use crate::{
    css::values::{Edges, LineHeight},
    dom::NodeId,
    style::{ComputedStyle, Diagnostic, Direction, TextAlign},
};

use super::{
    BoxKind, InlineFragment, LayoutBox, LineBox, Rect, TextFragment,
    tree::{finite_rect, finite_used, resolve_edges, resolve_line_height, resolve_margins},
};

#[path = "line_break.rs"]
mod breaking;

#[derive(Clone, Copy)]
pub(super) struct FontMetrics {
    pub(super) ascent: f32,
    pub(super) descent: f32,
    pub(super) above: f32,
    pub(super) below: f32,
}

pub(super) fn font_metrics(
    font: &crate::text::FontContext,
    style: &ComputedStyle,
    node: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> FontMetrics {
    if style.writing_mode.vertical() {
        let height = if style.line_height == LineHeight::Normal {
            style.font_size
        } else {
            resolve_line_height(&style.line_height, style.font_size, node, diagnostics)
        };
        return FontMetrics {
            ascent: style.font_size * 0.5,
            descent: style.font_size * 0.5,
            above: height * 0.5,
            below: height * 0.5,
        };
    }
    let face = font.select(style);
    let metrics = face
        .horizontal_line_metrics(style.font_size)
        .expect("bundled horizontal face");
    let height = if style.line_height == LineHeight::Normal {
        metrics.new_line_size
    } else {
        resolve_line_height(&style.line_height, style.font_size, node, diagnostics)
    };
    let leading = (height as f64 - metrics.ascent as f64 + metrics.descent as f64) / 2.0;
    FontMetrics {
        ascent: metrics.ascent,
        descent: -metrics.descent,
        above: finite_used(
            metrics.ascent as f64 + leading,
            node,
            "line ascent",
            diagnostics,
        ),
        below: finite_used(
            -metrics.descent as f64 + leading,
            node,
            "line descent",
            diagnostics,
        ),
    }
}

// Identity is a preorder index within this inline run, not a DOM id: pseudo
// boxes and block-in-inline continuations can share a DOM identity.
pub(super) struct BoxData {
    parent: Option<usize>,
    node: Option<NodeId>,
    pub(super) style: ComputedStyle,
    metrics: FontMetrics,
    padding: Edges<f32>,
    margin: Edges<f32>,
    border: Edges<f32>,
    first: bool,
    last: bool,
    image_size: Option<(f32, f32)>,
    atomic: Option<Box<LayoutBox>>,
    atomic_baseline: f32,
    rect: Rect,
    content: Rect,
    text: Vec<TextFragment>,
    fragments: Vec<InlineFragment>,
    static_after_line: bool,
    inline_line: Option<usize>,
}

#[derive(Clone)]
pub(super) enum Token {
    Begin(usize),
    End(usize),
    Text(usize, String, f32, Option<crate::text::SourceSpan>),
    Glyphs(usize, String, std::sync::Arc<crate::text::ShapedText>),
    Space(usize, char, f32, bool),
    Control(usize, char),
    Omitted,
    Image(usize),
    Break(usize),
    Positioned(usize),
    Float(usize),
}

impl Token {
    fn width(&self, boxes: &[BoxData]) -> f64 {
        match self {
            Self::Begin(id) => {
                let b = &boxes[*id];
                b.margin.left as f64 + b.padding.left as f64 + b.border.left as f64
            }
            Self::End(id) => {
                let b = &boxes[*id];
                b.padding.right as f64 + b.border.right as f64 + b.margin.right as f64
            }
            Self::Text(_, _, width, _) | Self::Space(_, _, width, _) => *width as f64,
            Self::Glyphs(_, _, run) => run.width as f64,
            Self::Image(id) => {
                let b = &boxes[*id];
                b.margin.left as f64 + b.rect.width as f64 + b.margin.right as f64
            }
            Self::Break(_)
            | Self::Control(..)
            | Self::Omitted
            | Self::Positioned(_)
            | Self::Float(_) => 0.0,
        }
    }

    fn real(&self, boxes: &[BoxData]) -> bool {
        match self {
            Self::Text(..) | Self::Image(_) | Self::Break(_) => true,
            Self::Glyphs(_, text, _) => text.chars().any(|c| !nonprinting(c)),
            Self::Begin(id) => {
                let b = &boxes[*id];
                b.padding.left != 0.0
                    || b.border.left != 0.0
                    || b.margin.left != 0.0
                    || b.padding.top != 0.0
                    || b.padding.bottom != 0.0
                    || b.border.top != 0.0
                    || b.border.bottom != 0.0
            }
            Self::End(id) => {
                let b = &boxes[*id];
                b.padding.right != 0.0 || b.border.right != 0.0 || b.margin.right != 0.0
            }
            Self::Space(_, _, _, preserved) => *preserved,
            Self::Control(..) | Self::Omitted | Self::Positioned(_) | Self::Float(_) => false,
        }
    }
}

fn collect(
    layout: &LayoutBox,
    parent: Option<usize>,
    (width, reference_height, mode): (f32, Option<f32>, crate::style::WritingMode),
    font: &crate::text::FontContext,
    boxes: &mut Vec<BoxData>,
    tokens: &mut Vec<Token>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let id = boxes.len();
    let mut projected = layout.style.as_ref().clone();
    projected.writing_mode = mode;
    let mut flow_style = super::writing::inline_style(&projected);
    flow_style.writing_mode = layout.style.writing_mode;
    let style = &flow_style;
    let metrics = font_metrics(font, style, layout.node_id, diagnostics);
    let is_inline = layout.kind == BoxKind::Inline;
    if is_inline
        && !layout.floating()
        && style.vertical_align != crate::style::VerticalAlign::Baseline
    {
        diagnostics.push(Diagnostic { node_id: layout.node_id,
            message: "non-baseline inline vertical-align is not implemented; using baseline (table-cell alignment is supported)".into() });
    }
    let padding = if is_inline {
        layout.inline_edges(
            resolve_edges(
                &style.padding,
                style.font_size,
                width,
                layout.node_id,
                "padding",
                diagnostics,
            )
            .map(|value| value.max(0.0)),
        )
    } else {
        Edges::default()
    };
    let margin = if is_inline {
        layout.inline_edges(
            resolve_margins(style, width, layout.node_id, diagnostics)
                .map(|value| value.unwrap_or(0.0)),
        )
    } else {
        Edges::default()
    };
    let border = if is_inline {
        layout.inline_edges(style.border_width)
    } else {
        Edges::default()
    };
    let mut data = BoxData {
        parent,
        node: layout.node_id,
        style: style.clone(),
        metrics,
        padding,
        margin,
        border,
        first: layout.inline_start,
        last: layout.inline_end,
        image_size: None,
        atomic: None,
        atomic_baseline: 0.0,
        rect: Rect::default(),
        content: Rect::default(),
        text: Vec::new(),
        fragments: Vec::new(),
        static_after_line: false,
        inline_line: None,
    };
    if layout.floating() {
        let mut atomic = layout.clone();
        atomic.reset();
        if style.writing_mode.vertical() {
            let intrinsic =
                intrinsic_inline(&atomic.children, &atomic.style, width, font, diagnostics);
            let inline = style
                .width
                .as_ref()
                .map(|l| l.resolve(style.font_size, width))
                .unwrap_or(width.max(intrinsic.0).min(intrinsic.1));
            super::writing::layout(
                &mut atomic,
                0.0,
                reference_height.unwrap_or(width),
                Some(width),
                None,
                Some(inline),
                width,
                font,
                diagnostics,
            );
            let axes = super::writing::FlowAxes {
                mode: style.writing_mode,
                origin: (0.0, 0.0),
                block: 0.0,
                inline: width,
            };
            data.margin = axes.logical_edges(&atomic.margin);
            data.padding = axes.logical_edges(&atomic.padding);
            data.border = axes.logical_edges(&atomic.style.border_width);
            data.rect = Rect {
                x: 0.0,
                y: 0.0,
                width: atomic.rect.height,
                height: atomic.rect.width,
            };
            data.content = Rect {
                x: 0.0,
                y: 0.0,
                width: atomic.content_rect.height,
                height: atomic.content_rect.width,
            };
        } else {
            super::floats::layout_float(&mut atomic, width, reference_height, font, diagnostics);
            data.margin = atomic.margin;
            data.padding = atomic.padding;
            data.border = atomic.used_border.unwrap_or(atomic.style.border_width);
            data.rect = atomic.rect;
            data.content = atomic.content_rect;
        }
        data.atomic = Some(Box::new(atomic));
        boxes.push(data);
        tokens.push(Token::Float(id));
        return;
    }
    if let Some(image) = &layout.image {
        let horizontal = finite_used(
            padding.left as f64 + padding.right as f64 + border.left as f64 + border.right as f64,
            layout.node_id,
            "image horizontal decorations",
            diagnostics,
        );
        let vertical = finite_used(
            padding.top as f64 + padding.bottom as f64 + border.top as f64 + border.bottom as f64,
            layout.node_id,
            "image vertical decorations",
            diagnostics,
        );
        let explicit_width = if mode.vertical() {
            super::sizing::definite_height(
                layout,
                style.width.as_ref(),
                Some(width),
                horizontal,
                diagnostics,
            )
        } else {
            super::sizing::definite_size(
                layout,
                style.width.as_ref(),
                Some(width),
                horizontal,
                diagnostics,
            )
        };
        let explicit_height = if mode.vertical() {
            super::sizing::definite_size(
                layout,
                style.height.as_ref(),
                reference_height,
                vertical,
                diagnostics,
            )
        } else {
            super::sizing::definite_height(
                layout,
                style.height.as_ref(),
                reference_height,
                vertical,
                diagnostics,
            )
        };
        let mut size = image.size(
            &layout.style,
            if mode.vertical() {
                explicit_height
            } else {
                explicit_width
            },
            if mode.vertical() {
                explicit_width
            } else {
                explicit_height
            },
            metrics.above + metrics.below,
            font,
        );
        let (physical_width, physical_height, physical_horizontal, physical_vertical) =
            if mode.vertical() {
                (explicit_height, explicit_width, vertical, horizontal)
            } else {
                (explicit_width, explicit_height, horizontal, vertical)
            };
        if physical_height.is_none() {
            size.1 =
                super::sizing::ratio_height(layout, size.0, physical_horizontal, physical_vertical)
                    .unwrap_or(size.1);
        } else if physical_width.is_none() && physical_height.is_some() {
            size.0 =
                super::sizing::ratio_width(layout, size.1, physical_horizontal, physical_vertical)
                    .unwrap_or(size.0);
        }
        size = super::sizing::constrain_replaced(
            layout,
            size,
            if mode.vertical() {
                (explicit_height.is_some(), explicit_width.is_some())
            } else {
                (explicit_width.is_some(), explicit_height.is_some())
            },
            if mode.vertical() {
                (reference_height, Some(width))
            } else {
                (Some(width), reference_height)
            },
            if mode.vertical() {
                (vertical, horizontal)
            } else {
                (horizontal, vertical)
            },
            diagnostics,
        );
        if mode.vertical() {
            std::mem::swap(&mut size.0, &mut size.1);
        }
        data.image_size = Some(size);
        data.rect.width = finite_used(
            size.0 as f64 + horizontal as f64,
            layout.node_id,
            "image border width",
            diagnostics,
        );
        data.rect.height = finite_used(
            size.1 as f64 + vertical as f64,
            layout.node_id,
            "image border height",
            diagnostics,
        );
        data.atomic_baseline = if mode.vertical() {
            data.rect.height * 0.5
        } else {
            data.rect.height + data.margin.bottom
        };
        boxes.push(data);
        if layout.style.position.out_of_flow() {
            tokens.push(Token::Positioned(id));
            return;
        }
        tokens.push(Token::Image(id));
        return;
    }
    if layout.atomic_inline() {
        let mut atomic = layout.clone();
        atomic.reset();
        let physical_width = if mode.vertical() {
            reference_height.unwrap_or(width)
        } else {
            width
        };
        let physical_height = if mode.vertical() {
            Some(width)
        } else {
            reference_height
        };
        let physical_style = &layout.style;
        let decor = physical_style
            .padding
            .left
            .resolve(physical_style.font_size, physical_width)
            + physical_style
                .padding
                .right
                .resolve(physical_style.font_size, physical_width)
            + physical_style.border_width.left
            + physical_style.border_width.right;
        let vertical = physical_style
            .padding
            .top
            .resolve(physical_style.font_size, physical_width)
            + physical_style
                .padding
                .bottom
                .resolve(physical_style.font_size, physical_width)
            + physical_style.border_width.top
            + physical_style.border_width.bottom;
        let explicit_width = super::sizing::definite_size(
            layout,
            physical_style.width.as_ref(),
            Some(physical_width),
            decor,
            diagnostics,
        );
        let height = super::sizing::definite_height(
            layout,
            physical_style.height.as_ref(),
            physical_height,
            vertical,
            diagnostics,
        );
        let intrinsic =
            super::positioning::intrinsic_content(layout, physical_width, font, diagnostics);
        let available = (physical_width - decor).max(0.0);
        let used_width = explicit_width
            .or_else(|| height.and_then(|h| super::sizing::ratio_width(layout, h, decor, vertical)))
            .unwrap_or_else(|| available.max(intrinsic.0).min(intrinsic.1));
        let limits =
            super::sizing::bounds(layout, true, Some(physical_width), decor, 0.0, diagnostics);
        if physical_style.writing_mode.vertical() {
            let inline_available =
                physical_height.unwrap_or(crate::css::media::DEFAULT_VIEWPORT_HEIGHT - 48.0);
            let intrinsic = intrinsic_inline(
                &layout.children,
                physical_style,
                inline_available,
                font,
                diagnostics,
            );
            let inline = height.unwrap_or(inline_available.max(intrinsic.0).min(intrinsic.1));
            super::writing::layout(
                &mut atomic,
                0.0,
                physical_width,
                physical_height,
                explicit_width,
                Some(inline),
                width,
                font,
                diagnostics,
            );
        } else {
            super::block::layout_sized_in(
                &mut atomic,
                physical_width,
                used_width.clamp(limits.0, limits.1),
                height,
                (explicit_width.is_some(), true),
                width,
                font,
                diagnostics,
            );
        }
        if mode.vertical() {
            data.rect = Rect {
                x: 0.0,
                y: 0.0,
                width: atomic.rect.height,
                height: atomic.rect.width,
            };
            data.content = Rect {
                x: 0.0,
                y: 0.0,
                width: atomic.content_rect.height,
                height: atomic.content_rect.width,
            };
            data.image_size = Some((atomic.content_rect.height, atomic.content_rect.width));
            data.atomic_baseline = atomic.rect.width * 0.5;
        } else {
            data.rect = atomic.rect;
            data.content = atomic.content_rect;
            data.image_size = Some((atomic.content_rect.width, atomic.content_rect.height));
            data.atomic_baseline = if physical_style.writing_mode.vertical() {
                atomic.rect.height
            } else {
                atomic.first_local_baseline().unwrap_or(atomic.rect.height)
            };
        }
        data.atomic = Some(Box::new(atomic));
        boxes.push(data);
        tokens.push(Token::Image(id));
        return;
    }
    boxes.push(data);
    if layout.style.position.out_of_flow() {
        tokens.push(Token::Positioned(id));
        return;
    }
    if layout.tag_name == "<br>" {
        tokens.push(Token::Break(id));
    } else if layout.tag_name == "<wbr>" {
        tokens.push(Token::Control(id, '\u{200b}'));
    } else if layout.kind == BoxKind::Text {
        if let Some(text) = &layout.text {
            if text.len() > crate::text::MAX_TEXT_BYTES {
                diagnostics.push(Diagnostic {
                    node_id: layout.node_id,
                    message: format!(
                        "text exceeds {}-byte shaping limit; omitted",
                        crate::text::MAX_TEXT_BYTES
                    ),
                });
            } else {
                tokens.push(Token::Text(
                    id,
                    text.clone(),
                    0.0,
                    Some(crate::text::SourceSpan {
                        node_id: layout.node_id,
                        source: std::sync::Arc::from(text.as_str()),
                        start: 0,
                        end: text.len(),
                    }),
                ));
            }
        }
    } else {
        tokens.push(Token::Begin(id));
        for child in &layout.children {
            collect(
                child,
                Some(id),
                (width, reference_height, mode),
                font,
                boxes,
                tokens,
                diagnostics,
            );
        }
        tokens.push(Token::End(id));
    }
}

struct Segment {
    id: usize,
    border_start: f32,
    content_start: f32,
    content_end: Option<f32>,
    border_end: Option<f32>,
    first: bool,
    last: bool,
}

struct Assembly<'a> {
    boxes: &'a mut [BoxData],
    font: &'a crate::text::FontContext,
    diagnostics: &'a mut Vec<Diagnostic>,
    origin: f32,
    y: f32,
    x: f32,
    width: f32,
    align: TextAlign,
    last_align: crate::style::TextAlignLast,
    justify: crate::style::typography::Keyword,
    direction: Direction,
    strut: FontMetrics,
    above: f32,
    below: f32,
    real: bool,
    active: Vec<usize>,
    segments: Vec<Segment>,
    items: Vec<(Token, f32)>,
    lines: Vec<LineBox>,
    floats: Option<&'a mut super::floats::FloatContext>,
    full_origin: f32,
    full_width: f32,
    bidi: bool,
}

impl Assembly<'_> {
    fn opportunity(&mut self, needed: f32, height: f32) {
        let Some(floats) = self.floats.as_deref_mut() else {
            return;
        };
        loop {
            let band = floats.opportunity(
                self.full_origin,
                self.y + floats.offset,
                self.full_width,
                height,
            );
            if needed <= band.1 || floats.next_bottom(self.y + floats.offset).is_none() {
                self.origin = band.0;
                self.width = band.1;
                self.x = self.origin;
                // Continuing inline decorations begin at the new opportunity.
                for segment in &mut self.segments {
                    segment.border_start = self.origin;
                    segment.content_start = self.origin;
                }
                break;
            }
            self.y = floats.next_bottom(self.y + floats.offset).unwrap() - floats.offset;
        }
    }
    fn place_float(&mut self, id: usize, preceding: f32) {
        let Some(floats) = self.floats.as_deref_mut() else {
            return;
        };
        if self.boxes[id].style.writing_mode.vertical() {
            let mut proxy = self.boxes[id]
                .atomic
                .as_ref()
                .expect("float formatting context")
                .as_ref()
                .clone();
            proxy.children.clear();
            proxy.rect = self.boxes[id].rect;
            proxy.content_rect = self.boxes[id].content;
            proxy.margin = self.boxes[id].margin;
            floats.place(
                &mut proxy,
                self.full_origin,
                self.y,
                self.full_width,
                preceding,
                (self.strut.above + self.strut.below).max(0.0),
                self.diagnostics,
            );
            self.boxes[id].rect = proxy.rect;
            self.boxes[id].content = proxy.content_rect;
            self.opportunity(0.0, (self.strut.above + self.strut.below).max(0.0));
            return;
        }
        let b = self.boxes[id]
            .atomic
            .as_mut()
            .expect("float formatting context");
        floats.place(
            b,
            self.full_origin,
            self.y,
            self.full_width,
            preceding,
            (self.strut.above + self.strut.below).max(0.0),
            self.diagnostics,
        );
        let (rect, content) = (b.rect, b.content_rect);
        self.boxes[id].rect = rect;
        self.boxes[id].content = content;
        self.opportunity(0.0, (self.strut.above + self.strut.below).max(0.0));
    }
    fn advance(&mut self, width: f64, node: Option<NodeId>) {
        self.x = finite_used(
            self.x as f64 + width,
            node,
            "inline flow x",
            self.diagnostics,
        );
    }

    fn contribute(&mut self, metrics: FontMetrics) {
        self.above = self.above.max(metrics.above);
        self.below = self.below.max(metrics.below);
    }

    fn add(&mut self, token: Token) {
        if token.real(self.boxes) {
            self.real = true;
            for index in 0..self.active.len() {
                self.contribute(self.boxes[self.active[index]].metrics);
            }
        }
        match token {
            Token::Positioned(id) => {
                self.boxes[id].static_after_line = self.real;
                self.boxes[id].rect.x = if self.boxes[id].style.positioned_inline {
                    self.x
                } else {
                    self.origin
                };
                self.boxes[id].rect.y = self.y;
                self.boxes[id].content = self.boxes[id].rect;
                self.items.push((Token::Positioned(id), self.x));
            }
            Token::Begin(id) => {
                let b = &self.boxes[id];
                let node = b.node;
                let left_margin = b.margin.left as f64;
                let left = b.padding.left as f64 + b.border.left as f64;
                let metrics = b.metrics;
                let first = b.first;
                let real = Token::Begin(id).real(self.boxes);
                self.advance(left_margin, node);
                let start = self.x;
                self.boxes[id].rect.x = start;
                self.boxes[id].rect.y = self.y;
                self.boxes[id].content = self.boxes[id].rect;
                self.advance(left, node);
                self.segments.push(Segment {
                    id,
                    border_start: start,
                    content_start: self.x,
                    content_end: None,
                    border_end: None,
                    first,
                    last: false,
                });
                self.active.push(id);
                if real {
                    self.contribute(metrics);
                }
            }
            Token::End(id) => {
                let b = &self.boxes[id];
                let node = b.node;
                let right = b.padding.right as f64 + b.border.right as f64;
                let margin = b.margin.right as f64;
                let last = b.last;
                let metrics = b.metrics;
                if Token::End(id).real(self.boxes) {
                    self.contribute(metrics);
                }
                if !self.real
                    && let Some(fragment) = self.boxes[id].fragments.last_mut()
                {
                    fragment.last = last;
                }
                let end = self.x;
                self.advance(right, node);
                if let Some(segment) = self
                    .segments
                    .iter_mut()
                    .rev()
                    .find(|segment| segment.id == id)
                {
                    segment.content_end = Some(end);
                    segment.border_end = Some(self.x);
                    segment.last = last;
                }
                debug_assert_eq!(self.active.pop(), Some(id));
                self.advance(margin, node);
            }
            Token::Image(id) => {
                let b = &self.boxes[id];
                let above = finite_used(
                    b.margin.top as f64 + b.atomic_baseline as f64,
                    b.node,
                    "image margin height",
                    self.diagnostics,
                );
                self.contribute(FontMetrics {
                    above,
                    below: b.rect.height + b.margin.bottom - b.atomic_baseline,
                    ..b.metrics
                });
                let advance = Token::Image(id).width(self.boxes);
                self.items.push((Token::Image(id), self.x));
                self.advance(advance, self.boxes[id].node);
            }
            Token::Text(id, text, width, source) => {
                self.contribute(self.boxes[id].metrics);
                if let Some((
                    Token::Text(previous_id, previous_text, previous_width, _),
                    previous_x,
                )) = self.items.last_mut()
                    && !self.bidi
                    && *previous_id == id
                    && (*previous_x + *previous_width - self.x).abs() < 0.001
                {
                    previous_text.push_str(&text);
                    *previous_width += width;
                } else {
                    self.items
                        .push((Token::Text(id, text, width, source), self.x));
                }
                self.advance(width as f64, self.boxes[id].node);
            }
            Token::Glyphs(id, text, run) => {
                let metrics = self.boxes[id].metrics;
                let (above, below) = run.line_extents.unwrap_or((metrics.above, metrics.below));
                self.contribute(FontMetrics {
                    above,
                    below,
                    ..metrics
                });
                let width = run.width;
                if let Some((Token::Glyphs(previous_id, previous_text, previous_run), previous_x)) =
                    self.items.last_mut()
                    && !self.bidi
                    && *previous_id == id
                    && (*previous_x + previous_run.width - self.x).abs() < 0.001
                {
                    previous_text.push_str(&text);
                    std::sync::Arc::make_mut(previous_run).append(&run);
                } else {
                    self.items.push((Token::Glyphs(id, text, run), self.x));
                }
                self.advance(width as f64, self.boxes[id].node);
            }
            Token::Space(id, character, width, preserved) => {
                self.contribute(self.boxes[id].metrics);
                for index in 0..self.active.len() {
                    self.contribute(self.boxes[self.active[index]].metrics);
                }
                if preserved {
                    self.items
                        .push((Token::Space(id, character, width, true), self.x));
                }
                self.advance(width as f64, self.boxes[id].node);
            }
            Token::Break(id) => {
                self.contribute(self.boxes[id].metrics);
                self.items.push((Token::Break(id), self.x));
            }
            Token::Control(..) | Token::Omitted | Token::Float(_) => {}
        }
    }

    fn visualize(
        &mut self,
        slots: &[(Token, f32, f32, std::ops::Range<usize>)],
        levels: &[unicode_bidi::Level],
    ) {
        if !self.bidi {
            return;
        }
        let mut groups: Vec<std::ops::Range<usize>> = Vec::new();
        let mut start = 0;
        for i in 0..slots.len() {
            let protected = matches!(&slots[i].0, Token::Glyphs(_, _, run) if run.unsafe_after);
            let next_zero = matches!(slots.get(i + 1).map(|s| &s.0), Some(Token::Glyphs(_, _, run)) if run.width == 0.0);
            if !protected && !next_zero {
                groups.push(start..i + 1);
                start = i + 1;
            }
        }
        if start < slots.len() {
            groups.push(start..slots.len());
        }
        let group_levels: Vec<_> = groups.iter().map(|g| levels[g.start]).collect();
        let order = unicode_bidi::BidiInfo::reorder_visual(&group_levels);
        let mut visual = vec![0.0; slots.len()];
        let mut x = self.origin;
        for index in order {
            for i in groups[index].clone() {
                visual[i] = x;
                for item in slots[i].3.clone() {
                    self.items[item].1 += x - slots[i].1;
                }
                x += slots[i].2;
            }
        }
        let old = std::mem::take(&mut self.segments);
        for segment in old {
            let mut intervals = Vec::new();
            for (i, (token, _, width, _)) in slots.iter().enumerate() {
                let id = match token {
                    Token::Begin(id)
                    | Token::End(id)
                    | Token::Glyphs(id, ..)
                    | Token::Text(id, ..)
                    | Token::Space(id, ..)
                    | Token::Image(id)
                    | Token::Break(id) => *id,
                    _ => continue,
                };
                let mut ancestor = Some(id);
                while let Some(a) = ancestor {
                    if a == segment.id {
                        intervals.push((visual[i], visual[i] + width));
                        break;
                    }
                    ancestor = self.boxes[a].parent;
                }
            }
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut joined: Vec<(f32, f32)> = Vec::new();
            for (left, right) in intervals {
                if let Some(last) = joined.last_mut()
                    && left <= last.1 + 0.001
                {
                    last.1 = last.1.max(right);
                } else {
                    joined.push((left, right));
                }
            }
            for (left, right) in joined {
                self.segments.push(Segment {
                    id: segment.id,
                    border_start: left,
                    content_start: left,
                    content_end: Some(right),
                    border_end: Some(right),
                    first: segment.first,
                    last: segment.last,
                });
            }
        }
    }
    fn finish(&mut self, advance: f64) {
        if !self.real {
            return;
        }
        if self.bidi {
            let mut merged: Vec<(Token, f32)> = Vec::new();
            for (token, x) in std::mem::take(&mut self.items) {
                if let Token::Glyphs(id, text, run) = &token
                    && let Some((Token::Glyphs(old_id, old_text, old_run), old_x)) =
                        merged.last_mut()
                    && *old_id == *id
                    && (old_run.glyphs.first().map(|g| g.rtl) == run.glyphs.first().map(|g| g.rtl)
                        || run.glyphs.is_empty())
                    && ((*old_x + old_run.width - x).abs() < 0.001
                        || (x + run.width - *old_x).abs() < 0.001)
                {
                    let origin = (*old_x).min(x);
                    let old = std::sync::Arc::make_mut(old_run);
                    for g in &mut old.glyphs {
                        g.x += *old_x - origin;
                    }
                    old.glyphs.extend(run.glyphs.iter().cloned().map(|mut g| {
                        g.x += x - origin;
                        g
                    }));
                    old.width += run.width;
                    old_text.push_str(text);
                    *old_x = origin;
                } else {
                    merged.push((token, x));
                }
            }
            self.items = merged;
        }
        let remaining = self.width as f64 - advance;
        let align = match (self.align, self.direction) {
            (TextAlign::Start, Direction::Ltr) | (TextAlign::End, Direction::Rtl) => {
                TextAlign::Left
            }
            (TextAlign::Start, Direction::Rtl) | (TextAlign::End, Direction::Ltr) => {
                TextAlign::Right
            }
            (align, _) => align,
        };
        let shift = finite_used(
            match align {
                TextAlign::Right => remaining,
                TextAlign::Center => remaining / 2.0,
                _ => 0.0,
            },
            None,
            "text alignment offset",
            self.diagnostics,
        );
        let baseline = finite_used(
            self.y as f64 + self.above as f64,
            None,
            "line baseline",
            self.diagnostics,
        );
        let height = finite_used(
            (self.above as f64 + self.below as f64).max(0.0),
            None,
            "line height",
            self.diagnostics,
        );
        self.lines.push(LineBox {
            rect: Rect {
                x: self.origin,
                y: self.y,
                width: self.width,
                height,
            },
            content_rect: Rect {
                x: finite_used(
                    self.origin as f64 + shift as f64,
                    None,
                    "line x",
                    self.diagnostics,
                ),
                y: self.y,
                width: finite_used(advance.max(0.0), None, "line width", self.diagnostics),
                height,
            },
            baseline,
            ascent: self.above,
            descent: self.below,
        });
        for segment in self.segments.drain(..) {
            let b = &mut self.boxes[segment.id];
            let content_end = segment.content_end.unwrap_or(self.x);
            let border_end = segment.border_end.unwrap_or(self.x);
            let content = Rect {
                x: finite_used(
                    segment.content_start as f64 + shift as f64,
                    b.node,
                    "inline content x",
                    self.diagnostics,
                ),
                y: finite_used(
                    baseline as f64 - b.metrics.ascent as f64,
                    b.node,
                    "inline content y",
                    self.diagnostics,
                ),
                width: finite_used(
                    (content_end as f64 - segment.content_start as f64).max(0.0),
                    b.node,
                    "inline content width",
                    self.diagnostics,
                ),
                height: b.metrics.ascent + b.metrics.descent,
            };
            let rect = Rect {
                x: finite_used(
                    segment.border_start as f64 + shift as f64,
                    b.node,
                    "inline border x",
                    self.diagnostics,
                ),
                y: finite_used(
                    content.y as f64 - b.padding.top as f64 - b.border.top as f64,
                    b.node,
                    "inline border y",
                    self.diagnostics,
                ),
                width: finite_used(
                    (border_end as f64 - segment.border_start as f64).max(0.0),
                    b.node,
                    "inline border width",
                    self.diagnostics,
                ),
                height: finite_used(
                    content.height as f64
                        + b.padding.top as f64
                        + b.padding.bottom as f64
                        + b.border.top as f64
                        + b.border.bottom as f64,
                    b.node,
                    "inline border height",
                    self.diagnostics,
                ),
            };
            b.content = finite_rect(b.content.union(content), b.node, self.diagnostics);
            b.rect = finite_rect(b.rect.union(rect), b.node, self.diagnostics);
            b.fragments.push(InlineFragment {
                line_index: self.lines.len() - 1,
                rect,
                content_rect: content,
                baseline,
                first: segment.first,
                last: segment.last,
            });
        }
        for (token, x) in self.items.drain(..) {
            let whitespace = matches!(token, Token::Space(..));
            let token = match token {
                Token::Space(id, character, width, true) => {
                    let mut run = if character == '\t' {
                        crate::text::ShapedText::default()
                    } else {
                        (*self
                            .font
                            .shape(&self.boxes[id].style, &character.to_string()))
                        .clone()
                    };
                    run.width = width;
                    Token::Glyphs(id, character.to_string(), std::sync::Arc::new(run))
                }
                token => token,
            };
            match token {
                Token::Glyphs(id, text, run) => {
                    let b = &mut self.boxes[id];
                    let x =
                        finite_used(x as f64 + shift as f64, b.node, "text x", self.diagnostics);
                    let y = finite_used(
                        baseline as f64 - b.metrics.ascent as f64,
                        b.node,
                        "text y",
                        self.diagnostics,
                    );
                    let ink_rect = finite_rect(
                        run.ink_bounds(b.style.font_size, x, baseline),
                        b.node,
                        self.diagnostics,
                    );
                    let advance_rect = Rect {
                        x,
                        y,
                        width: run.width,
                        height: run.ascent.max(b.metrics.ascent)
                            + run.descent.max(b.metrics.descent),
                    };
                    b.rect = finite_rect(
                        b.rect.union(if whitespace || run.glyphs.is_empty() {
                            advance_rect
                        } else {
                            ink_rect
                        }),
                        b.node,
                        self.diagnostics,
                    );
                    b.content = b.rect;
                    b.text.push(TextFragment {
                        line_index: self.lines.len() - 1,
                        x,
                        y,
                        baseline,
                        ink_rect,
                        width: run.width,
                        height: advance_rect.height,
                        glyphs: run,
                        text,
                        font_size: b.style.font_size,
                        color: b.style.text_fill_color.legacy(),
                        text_color: b.style.text_fill_color,
                        bold: b.style.bold,
                        italic: b.style.italic,
                    });
                }
                Token::Image(id) => {
                    let b = &mut self.boxes[id];
                    b.inline_line = Some(self.lines.len() - 1);
                    b.rect.x = finite_used(
                        x as f64 + shift as f64 + b.margin.left as f64,
                        b.node,
                        "image x",
                        self.diagnostics,
                    );
                    b.rect.y = finite_used(
                        baseline as f64 - b.atomic_baseline as f64,
                        b.node,
                        "image y",
                        self.diagnostics,
                    );
                    let (width, height) = b.image_size.expect("replaced dimensions");
                    b.content = Rect {
                        x: finite_used(
                            b.rect.x as f64 + b.border.left as f64 + b.padding.left as f64,
                            b.node,
                            "image content x",
                            self.diagnostics,
                        ),
                        y: finite_used(
                            b.rect.y as f64 + b.border.top as f64 + b.padding.top as f64,
                            b.node,
                            "image content y",
                            self.diagnostics,
                        ),
                        width,
                        height,
                    };
                }
                Token::Break(id) => {
                    let b = &mut self.boxes[id];
                    b.rect = Rect {
                        x: finite_used(
                            x as f64 + shift as f64,
                            b.node,
                            "break x",
                            self.diagnostics,
                        ),
                        y: self.y,
                        width: 0.0,
                        height,
                    };
                    b.content = b.rect;
                }
                Token::Positioned(id) => {
                    let b = &mut self.boxes[id];
                    b.rect.x = if b.style.positioned_inline {
                        finite_used(
                            x as f64 + shift as f64,
                            b.node,
                            "static position x",
                            self.diagnostics,
                        )
                    } else {
                        self.origin
                    };
                    b.rect.y = if !b.style.positioned_inline && b.static_after_line {
                        self.y + height
                    } else {
                        self.y
                    };
                    b.content = b.rect;
                }
                _ => unreachable!(),
            }
        }
        self.y = finite_used(
            self.y as f64 + height as f64,
            None,
            "inline flow y",
            self.diagnostics,
        );
        self.x = self.origin;
        self.above = self.strut.above;
        self.below = self.strut.below;
        self.real = false;
        for &id in &self.active {
            self.segments.push(Segment {
                id,
                border_start: self.origin,
                content_start: self.origin,
                content_end: None,
                border_end: None,
                first: false,
                last: false,
            });
        }
        self.opportunity(0.0, (self.strut.above + self.strut.below).max(0.0));
    }
}

pub(super) fn layout_inline_run(
    children: &mut [LayoutBox],
    style: &ComputedStyle,
    node: Option<NodeId>,
    (x, y, width, reference_height): (f32, f32, f32, Option<f32>),
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
    floats: Option<&mut super::floats::FloatContext>,
) -> (Vec<LineBox>, f64) {
    let strut = font_metrics(font, style, node, diagnostics);
    let mut boxes = Vec::new();
    let mut tokens = Vec::new();
    for child in children.iter() {
        collect(
            child,
            None,
            (width, reference_height, style.writing_mode),
            font,
            &mut boxes,
            &mut tokens,
            diagnostics,
        );
    }
    let mut assembly = Assembly {
        boxes: &mut boxes,
        font,
        diagnostics,
        origin: x,
        x,
        y,
        width,
        align: style.text_align,
        last_align: style.text_align_last,
        justify: style
            .typography
            .keyword(crate::style::typography::Property::TextJustify),
        direction: style.direction,
        strut,
        above: strut.above,
        below: strut.below,
        real: false,
        active: Vec::new(),
        segments: Vec::new(),
        items: Vec::new(),
        lines: Vec::new(),
        floats,
        full_origin: x,
        full_width: width,
        bidi: false,
    };
    assembly.opportunity(0.0, (strut.above + strut.below).max(0.0));
    breaking::layout(tokens, style, &mut assembly);
    let height = assembly.y as f64 - y as f64;
    let lines = std::mem::take(&mut assembly.lines);
    drop(assembly);
    let mut data = boxes.into_iter();
    for child in children {
        child.flow_width = width;
        install(child, &mut data, font, diagnostics);
    }
    (lines, height)
}

pub(super) fn intrinsic_inline(
    children: &[LayoutBox],
    style: &ComputedStyle,
    width: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let strut = font_metrics(font, style, None, diagnostics);
    let mut boxes = Vec::new();
    let mut tokens = Vec::new();
    for child in children {
        collect(
            child,
            None,
            (width, None, style.writing_mode),
            font,
            &mut boxes,
            &mut tokens,
            diagnostics,
        );
    }
    let mut assembly = Assembly {
        boxes: &mut boxes,
        font,
        diagnostics,
        origin: 0.0,
        y: 0.0,
        x: 0.0,
        width,
        align: style.text_align,
        last_align: style.text_align_last,
        justify: style
            .typography
            .keyword(crate::style::typography::Property::TextJustify),
        direction: style.direction,
        strut,
        above: strut.above,
        below: strut.below,
        real: false,
        active: Vec::new(),
        segments: Vec::new(),
        items: Vec::new(),
        lines: Vec::new(),
        floats: None,
        full_origin: 0.0,
        full_width: width,
        bidi: false,
    };
    breaking::intrinsic(tokens, style, &mut assembly)
}

fn install(
    layout: &mut LayoutBox,
    boxes: &mut impl Iterator<Item = BoxData>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let data = boxes.next().expect("inline preorder geometry");
    if let Some(mut atomic) = data.atomic {
        if layout.floating() {
            if layout.style.writing_mode.vertical() {
                let dx = data.rect.x - atomic.rect.x;
                super::positioning::translate(&mut atomic, dx, 0.0, diagnostics);
                let local_y = atomic.content_rect.y - atomic.rect.y;
                atomic.content_rect.y = local_y;
                super::block::place_block(&mut atomic, data.rect.y as f64, diagnostics);
            }
            *layout = *atomic;
            return;
        }
        let dx = data.rect.x - atomic.rect.x;
        super::positioning::translate(&mut atomic, dx, 0.0, diagnostics);
        super::block::place_block(&mut atomic, data.rect.y as f64, diagnostics);
        atomic.inline_line = data.inline_line;
        atomic.margin = data.margin;
        *layout = *atomic;
        return;
    }
    layout.rect = data.rect;
    layout.content_rect = data.content;
    layout.padding = data.padding;
    layout.margin = data.margin;
    layout.text_fragments = data.text;
    layout.inline_fragments = data.fragments;
    layout.inline_line = data.inline_line;
    if layout.style.position.out_of_flow() {
        return;
    }
    if let Some(image) = &layout.image {
        if let Some(mut fragment) = image.alt_fragment(
            layout.content_rect,
            &layout.style,
            font,
            layout.node_id,
            diagnostics,
        ) {
            fragment.line_index = layout.inline_line.unwrap_or(0);
            layout.text_fragments.push(fragment);
        }
        return;
    }
    for child in &mut layout.children {
        child.flow_width = layout.flow_width;
        install(child, boxes, font, diagnostics);
    }
    if layout.style.display == crate::style::Display::InlineListItem {
        let text = layout
            .marker_text
            .clone()
            .unwrap_or_else(|| "\u{2022}".into());
        super::block::assign_marker(layout, text, font, diagnostics);
        if layout.style.writing_mode.vertical() {
            diagnostics.push(Diagnostic { node_id: layout.node_id,
                message: "inline list-item marker uses horizontal outside placement; native vertical list marker placement remains unsupported".into() });
        }
    }
}

pub(super) fn glyph_bounds(
    font: &crate::text::FontContext,
    text: &str,
    style: &ComputedStyle,
    x: f32,
    baseline: f32,
) -> Rect {
    font.shape(style, text)
        .ink_bounds(style.font_size, x, baseline)
}

#[cfg(test)]
pub(super) fn measure_text(font: &crate::text::FontContext, text: &str, font_size: f32) -> f32 {
    font.shape(
        &ComputedStyle {
            font_size,
            ..ComputedStyle::default()
        },
        text,
    )
    .width
}

pub(crate) fn nonprinting(c: char) -> bool {
    matches!(c as u32, 0x00ad | 0x034f | 0x061c | 0x180e | 0x200b..=0x200f
        | 0x202a..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f | 0xfeff | 0xe0100..=0xe01ef)
}
