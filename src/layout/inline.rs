use crate::{
    css::values::{Edges, LineHeight},
    dom::NodeId,
    style::{ComputedStyle, Diagnostic, Direction, TextAlign},
};

use super::{
    BoxKind, InlineFragment, LayoutBox, LineBox, Rect, TextFragment,
    tree::{
        finite_rect, finite_used, resolve_edges, resolve_length, resolve_line_height,
        resolve_margins,
    },
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
    font: &fontdue::Font,
    style: &ComputedStyle,
    node: Option<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> FontMetrics {
    let metrics = font
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
struct BoxData {
    parent: Option<usize>,
    node: Option<NodeId>,
    style: ComputedStyle,
    metrics: FontMetrics,
    padding: Edges<f32>,
    margin: Edges<f32>,
    border: Edges<f32>,
    first: bool,
    last: bool,
    image_size: Option<(f32, f32)>,
    rect: Rect,
    content: Rect,
    text: Vec<TextFragment>,
    fragments: Vec<InlineFragment>,
}

#[derive(Clone)]
enum Token {
    Begin(usize),
    End(usize),
    Text(usize, String, f32),
    Space(usize, char, f32, bool),
    Control(usize, char),
    Omitted,
    Image(usize),
    Break(usize),
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
            Self::Text(_, _, width) | Self::Space(_, _, width, _) => *width as f64,
            Self::Image(id) => {
                let b = &boxes[*id];
                b.margin.left as f64 + b.rect.width as f64 + b.margin.right as f64
            }
            Self::Break(_) | Self::Control(..) | Self::Omitted => 0.0,
        }
    }

    fn real(&self, boxes: &[BoxData]) -> bool {
        match self {
            Self::Text(..) | Self::Image(_) | Self::Break(_) => true,
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
            Self::Control(..) | Self::Omitted => false,
        }
    }
}

fn collect(
    layout: &LayoutBox,
    parent: Option<usize>,
    width: f32,
    font: &fontdue::Font,
    boxes: &mut Vec<BoxData>,
    tokens: &mut Vec<Token>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let id = boxes.len();
    let style = &layout.style;
    let metrics = font_metrics(font, style, layout.node_id, diagnostics);
    let is_inline = layout.kind == BoxKind::Inline;
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
        rect: Rect::default(),
        content: Rect::default(),
        text: Vec::new(),
        fragments: Vec::new(),
    };
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
        let explicit_width = style.width.as_ref().map(|length| {
            style.box_sizing.content_size(
                resolve_length(
                    length,
                    style.font_size,
                    width,
                    layout.node_id,
                    "width",
                    diagnostics,
                ),
                horizontal,
            )
        });
        let explicit_height = style
            .height
            .as_ref()
            .filter(|length| !length.has_percentage())
            .map(|length| {
                style.box_sizing.content_size(
                    resolve_length(
                        length,
                        style.font_size,
                        0.0,
                        layout.node_id,
                        "height",
                        diagnostics,
                    ),
                    vertical,
                )
            });
        let size = image.size(
            style,
            explicit_width,
            explicit_height,
            metrics.above + metrics.below,
            font,
        );
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
        boxes.push(data);
        tokens.push(Token::Image(id));
        return;
    }
    boxes.push(data);
    if layout.tag_name == "<br>" {
        tokens.push(Token::Break(id));
    } else if layout.tag_name == "<wbr>" {
        tokens.push(Token::Control(id, '\u{200b}'));
    } else if layout.kind == BoxKind::Text {
        tokens.push(Token::Text(
            id,
            layout.text.clone().unwrap_or_default(),
            0.0,
        ));
    } else {
        tokens.push(Token::Begin(id));
        for child in &layout.children {
            collect(child, Some(id), width, font, boxes, tokens, diagnostics);
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
    font: &'a fontdue::Font,
    diagnostics: &'a mut Vec<Diagnostic>,
    origin: f32,
    y: f32,
    x: f32,
    width: f32,
    align: TextAlign,
    direction: Direction,
    strut: FontMetrics,
    above: f32,
    below: f32,
    real: bool,
    active: Vec<usize>,
    segments: Vec<Segment>,
    items: Vec<(Token, f32)>,
    lines: Vec<LineBox>,
}

impl Assembly<'_> {
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
                    b.margin.top as f64 + b.rect.height as f64 + b.margin.bottom as f64,
                    b.node,
                    "image margin height",
                    self.diagnostics,
                );
                self.contribute(FontMetrics {
                    above,
                    below: 0.0,
                    ..b.metrics
                });
                let advance = Token::Image(id).width(self.boxes);
                self.items.push((Token::Image(id), self.x));
                self.advance(advance, self.boxes[id].node);
            }
            Token::Text(id, text, width) => {
                self.contribute(self.boxes[id].metrics);
                if let Some((Token::Text(previous_id, previous_text, previous_width), previous_x)) =
                    self.items.last_mut()
                    && *previous_id == id
                    && (*previous_x + *previous_width - self.x).abs() < 0.001
                {
                    previous_text.push_str(&text);
                    *previous_width += width;
                } else {
                    self.items.push((Token::Text(id, text, width), self.x));
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
            Token::Control(..) | Token::Omitted => {}
        }
    }

    fn finish(&mut self, advance: f64) {
        if !self.real {
            return;
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
                    Token::Text(id, character.to_string(), width)
                }
                token => token,
            };
            match token {
                Token::Text(id, text, width) => {
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
                        glyph_bounds(self.font, &text, &b.style, x, baseline),
                        b.node,
                        self.diagnostics,
                    );
                    let advance_rect = Rect {
                        x,
                        y,
                        width,
                        height: b.metrics.ascent + b.metrics.descent,
                    };
                    b.rect = finite_rect(
                        b.rect
                            .union(if whitespace { advance_rect } else { ink_rect }),
                        b.node,
                        self.diagnostics,
                    );
                    b.content = b.rect;
                    b.text.push(TextFragment {
                        x,
                        y,
                        baseline,
                        ink_rect,
                        width,
                        height: b.metrics.ascent + b.metrics.descent,
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
                    b.rect.x = finite_used(
                        x as f64 + shift as f64 + b.margin.left as f64,
                        b.node,
                        "image x",
                        self.diagnostics,
                    );
                    b.rect.y = finite_used(
                        baseline as f64 - b.margin.bottom as f64 - b.rect.height as f64,
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
    }
}

pub(super) fn layout_inline_run(
    children: &mut [LayoutBox],
    style: &ComputedStyle,
    node: Option<NodeId>,
    (x, y, width): (f32, f32, f32),
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<LineBox>, f64) {
    let strut = font_metrics(font, style, node, diagnostics);
    let mut boxes = Vec::new();
    let mut tokens = Vec::new();
    for child in children.iter() {
        collect(
            child,
            None,
            width,
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
        direction: style.direction,
        strut,
        above: strut.above,
        below: strut.below,
        real: false,
        active: Vec::new(),
        segments: Vec::new(),
        items: Vec::new(),
        lines: Vec::new(),
    };
    breaking::layout(tokens, style, &mut assembly);
    let height = assembly.y as f64 - y as f64;
    let lines = std::mem::take(&mut assembly.lines);
    drop(assembly);
    let mut data = boxes.into_iter();
    for child in children {
        install(child, &mut data, font, diagnostics);
    }
    (lines, height)
}

fn install(
    layout: &mut LayoutBox,
    boxes: &mut impl Iterator<Item = BoxData>,
    font: &fontdue::Font,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let data = boxes.next().expect("inline preorder geometry");
    layout.rect = data.rect;
    layout.content_rect = data.content;
    layout.padding = data.padding;
    layout.margin = data.margin;
    layout.text_fragments = data.text;
    layout.inline_fragments = data.fragments;
    if let Some(image) = &layout.image {
        if let Some(fragment) = image.alt_fragment(
            layout.content_rect,
            &layout.style,
            font,
            layout.node_id,
            diagnostics,
        ) {
            layout.text_fragments.push(fragment);
        }
        return;
    }
    for child in &mut layout.children {
        install(child, boxes, font, diagnostics);
    }
}

pub(super) fn glyph_bounds(
    font: &fontdue::Font,
    text: &str,
    style: &ComputedStyle,
    x: f32,
    baseline: f32,
) -> Rect {
    let mut pen = x;
    let mut bounds = Rect::default();
    for character in text.chars() {
        if nonprinting(character) || character == '\t' {
            continue;
        }
        let metrics = font.metrics(character, style.font_size);
        if metrics.width != 0 && metrics.height != 0 {
            bounds = bounds.union(Rect {
                x: pen + metrics.xmin as f32,
                y: baseline - (metrics.ymin as f32 + metrics.height as f32),
                width: metrics.width as f32
                    + if style.bold { 1.0 } else { 0.0 }
                    + if style.italic {
                        (metrics.height as f32 * 0.2).round()
                    } else {
                        0.0
                    },
                height: metrics.height as f32,
            });
        }
        pen += metrics.advance_width;
    }
    bounds
}

pub(super) fn measure_text(font: &fontdue::Font, text: &str, font_size: f32) -> f32 {
    text.chars()
        .filter(|&character| !nonprinting(character))
        .map(|character| font.metrics(character, font_size).advance_width as f64)
        .sum::<f64>() as f32
}

pub(crate) fn nonprinting(c: char) -> bool {
    matches!(c as u32, 0x00ad | 0x034f | 0x061c | 0x180e | 0x200b..=0x200f
        | 0x202a..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f | 0xfeff | 0xe0100..=0xe01ef)
}
