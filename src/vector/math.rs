//! Finite MathML Core box layout in horizontal mathematical coordinates.
//! OpenType MATH constants are used when the selected face provides them.
use std::sync::Arc;

use crate::{
    css::values::{FontStyle, ResolvedColor},
    dom::{Namespace, NodeId, NodeKind},
    layout::Rect,
    style::{ComputedStyle, Diagnostic, Display},
    text::FontContext,
};
use tiny_skia::{PathBuilder, Transform};

use super::{Brush, Command, Scene, Source, Stroke, diagnose};

#[derive(Default)]
struct MathBox {
    width: f32,
    ascent: f32,
    descent: f32,
    commands: Vec<Command>,
}
impl MathBox {
    fn height(&self) -> f32 {
        self.ascent + self.descent
    }
    fn place(&mut self, child: MathBox, x: f32, baseline: f32) {
        if child.commands.is_empty() {
            return;
        }
        self.commands.push(Command::Group {
            scene: Arc::new(Scene {
                width: child.width,
                height: child.height(),
                baseline: Some(child.ascent),
                commands: child.commands,
            }),
            transform: Transform::from_translate(x, baseline - child.ascent),
            opacity: 1.0,
            clip: None,
        });
    }
    fn rule(&mut self, node: NodeId, rect: Rect, color: ResolvedColor) {
        if let Some(path) = super::rect_path(rect) {
            self.commands.push(Command::Path {
                source: Some(node),
                path,
                transform: Transform::identity(),
                fill: Some(Brush::Solid(color)),
                even_odd: false,
                stroke: None,
            });
        }
    }
}

#[derive(Clone, Copy)]
struct Metrics {
    axis: f32,
    rule: f32,
    gap: f32,
    script: f32,
    sub: f32,
    sup: f32,
    script_gap: f32,
    radical_gap: f32,
}
impl Metrics {
    fn new(fonts: &FontContext, style: &ComputedStyle) -> Self {
        let em = style.font_size;
        let mut m = Self {
            axis: em * 0.25,
            rule: em * 0.06,
            gap: em * 0.15,
            script: 0.71,
            sub: em * 0.25,
            sup: em * 0.4,
            script_gap: em * 0.2,
            radical_gap: em * 0.1,
        };
        let font = fonts.select(style);
        let face = font.shaper();
        if let Some(constants) = face.tables().math.and_then(|m| m.constants) {
            let scale = em / face.units_per_em() as f32;
            m.axis = constants.axis_height().value as f32 * scale;
            m.rule = constants.fraction_rule_thickness().value as f32 * scale;
            m.gap = constants.fraction_numerator_gap_min().value as f32 * scale;
            m.script = (constants.script_percent_scale_down() as f32 / 100.0).clamp(0.1, 1.0);
            m.sub = constants.subscript_shift_down().value as f32 * scale;
            m.sup = constants.superscript_shift_up().value as f32 * scale;
            m.script_gap = constants.sub_superscript_gap_min().value as f32 * scale;
            m.radical_gap = constants.radical_vertical_gap().value as f32 * scale;
        }
        m.rule = m.rule.max(em * 0.02);
        m
    }
}

struct Builder<'a> {
    source: &'a Source,
    fonts: &'a FontContext,
    diagnostics: &'a mut Vec<Diagnostic>,
    nodes: usize,
    text_bytes: usize,
    glyph_count: usize,
}

pub(super) fn build(
    source: &Source,
    fonts: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) -> Scene {
    let display = source
        .document
        .node(source.root)
        .and_then(|n| n.as_element())
        .is_some_and(|e| e.attribute("display") == Some("block"));
    let mut builder = Builder {
        source,
        fonts,
        diagnostics,
        nodes: 0,
        text_bytes: 0,
        glyph_count: 0,
    };
    let b = builder.node(source.root, 1.0, display, 0);
    Scene {
        width: b.width,
        height: b.height(),
        baseline: Some(b.ascent),
        commands: b.commands,
    }
}

impl Builder<'_> {
    fn warn(&mut self, id: NodeId, message: impl Into<String>) {
        diagnose(self.diagnostics, id, message);
    }
    fn text(&self, id: NodeId) -> String {
        fn collect(source: &Source, id: NodeId, text: &mut String, depth: usize) {
            if depth > super::MAX_DEPTH || text.len() > crate::text::MAX_TEXT_BYTES {
                return;
            }
            if let Some(node) = source.document.node(id) {
                if let NodeKind::Text(value) = node.kind() {
                    text.push_str(value);
                }
                for &child in node.children() {
                    collect(source, child, text, depth + 1);
                }
            }
        }
        let mut text = String::new();
        collect(self.source, id, &mut text, 0);
        text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
    }
    fn style(&self, id: NodeId, scale: f32) -> ComputedStyle {
        let mut style = self.source.styles.get(id).cloned().unwrap_or_default();
        // Mathematical axes stay local; CSS writing modes arrange the atomic
        // outer box, not the glyph offsets or numerator/denominator axes.
        style.writing_mode = crate::style::WritingMode::HorizontalTb;
        style.font_size = (style.font_size * scale).clamp(1.0, 4096.0);
        style
    }
    fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.source
            .document
            .node(id)
            .expect("MathML node")
            .children()
            .iter()
            .copied()
            .filter(|child| {
                self.source
                    .document
                    .node(*child)
                    .and_then(|n| n.as_element())
                    .is_some()
            })
            .collect()
    }
    fn node(&mut self, id: NodeId, mut scale: f32, mut display: bool, depth: usize) -> MathBox {
        self.nodes += 1;
        if depth > super::MAX_DEPTH || self.nodes > super::MAX_COMMANDS {
            self.warn(id, "MathML depth/node budget exceeded");
            return MathBox::default();
        }
        let Some(e) = self
            .source
            .document
            .node(id)
            .and_then(|n| n.as_element())
            .cloned()
        else {
            return MathBox::default();
        };
        if *e.namespace() != Namespace::MathMl {
            self.warn(id, "non-MathML child is unsupported in equations");
            return MathBox::default();
        }
        if self
            .source
            .styles
            .get(id)
            .is_some_and(|s| s.display == Display::None)
        {
            return MathBox::default();
        }
        if let Some(level) = e.attribute("scriptlevel") {
            if let Ok(level) = level.parse::<i32>() {
                scale *= 0.71_f32.powi(level.clamp(-8, 8));
            } else {
                self.warn(id, "invalid MathML scriptlevel");
            }
        }
        if let Some(value) = e.attribute("displaystyle") {
            display = value == "true";
        }
        let mut style = self.style(id, scale);
        if let Some(size) = e.attribute("mathsize") {
            if let Some(value) = super::svg::length(Some(size), style.font_size, style.font_size) {
                scale *= value / style.font_size;
                style.font_size = value.clamp(1.0, 4096.0);
            } else {
                self.warn(id, "unsupported MathML mathsize");
            }
        }
        if let Some(color) = e
            .attribute("mathcolor")
            .and_then(|v| super::color(v, style.text_color))
        {
            style.text_color = color;
            style.text_fill_color = color;
        }
        let children = self.children(id);
        if id != self.source.root
            && self
                .source
                .styles
                .get(id)
                .is_some_and(|s| s.effects.context())
        {
            self.warn(
                id,
                "CSS transforms/effects on inner MathML nodes are unsupported",
            );
        }
        let metrics = Metrics::new(self.fonts, &style);
        let mut result = match e.tag_name() {
            "mi" | "mn" | "mo" | "mtext" | "ms" => self.token(id, &e, &style, display),
            "math" | "mrow" | "mstyle" | "mtd" => {
                self.row(id, &children, scale, display, depth, &style)
            }
            "semantics" => children.first().map_or_else(MathBox::default, |&child| {
                self.node(child, scale, display, depth + 1)
            }),
            "annotation" | "annotation-xml" | "none" | "mprescripts" => MathBox::default(),
            "mspace" => {
                let dimension = |name| {
                    math_length(e.attribute(name), style.font_size)
                        .unwrap_or(0.0)
                        .max(0.0)
                };
                MathBox {
                    width: dimension("width"),
                    ascent: dimension("height"),
                    descent: dimension("depth"),
                    commands: Vec::new(),
                }
            }
            "mfrac" if children.len() == 2 => {
                let factor = if display { 1.0 } else { metrics.script };
                let numerator = self.node(children[0], scale * factor, false, depth + 1);
                let denominator = self.node(children[1], scale * factor, false, depth + 1);
                let thickness = e
                    .attribute("linethickness")
                    .and_then(|value| match value {
                        "thin" => Some(metrics.rule * 0.5),
                        "medium" => Some(metrics.rule),
                        "thick" => Some(metrics.rule * 2.0),
                        _ => math_length(Some(value), style.font_size),
                    })
                    .unwrap_or(metrics.rule)
                    .max(0.0);
                if e.attribute("bevelled") == Some("true") {
                    self.warn(id, "bevelled MathML fractions are unsupported");
                }
                let gap = metrics.gap * (if display { 2.0 } else { 1.0 });
                let width = numerator.width.max(denominator.width) + style.font_size * 0.2;
                let ascent = numerator.height() + gap + thickness * 0.5 + metrics.axis;
                let descent =
                    (denominator.height() + gap + thickness * 0.5 - metrics.axis).max(0.0);
                let line = ascent - metrics.axis;
                let nx = aligned(e.attribute("numalign"), width, numerator.width);
                let dx = aligned(e.attribute("denomalign"), width, denominator.width);
                let nbaseline = numerator.ascent;
                let dbaseline = line + thickness * 0.5 + gap + denominator.ascent;
                let mut b = MathBox {
                    width,
                    ascent,
                    descent,
                    commands: Vec::new(),
                };
                b.place(numerator, nx, nbaseline);
                b.place(denominator, dx, dbaseline);
                b.rule(
                    id,
                    Rect {
                        x: 0.0,
                        y: line - thickness * 0.5,
                        width,
                        height: thickness,
                    },
                    style.text_color,
                );
                b
            }
            "msqrt" | "mroot" => {
                let base = if e.tag_name() == "msqrt" {
                    self.row(id, &children, scale, display, depth, &style)
                } else if children.len() == 2 {
                    self.node(children[0], scale, display, depth + 1)
                } else {
                    self.warn(id, "mroot requires two children");
                    MathBox::default()
                };
                let degree = if e.tag_name() == "mroot" && children.len() == 2 {
                    Some(self.node(
                        children[1],
                        scale * metrics.script * metrics.script,
                        false,
                        depth + 1,
                    ))
                } else {
                    None
                };
                let prefix = degree.as_ref().map_or(0.0, |d| d.width * 0.7);
                let hook = style.font_size * 0.65;
                let ascent = base.ascent + metrics.radical_gap + metrics.rule * 2.0;
                let descent = base.descent;
                let width = prefix + hook + base.width + metrics.rule;
                let mut b = MathBox {
                    width,
                    ascent,
                    descent,
                    commands: Vec::new(),
                };
                b.place(base, prefix + hook, ascent);
                // A native stroked radical keeps the rule joined to its stem;
                // glyph assemblies are a documented finite future extension.
                let mut path = PathBuilder::new();
                let height = ascent + descent;
                path.move_to(prefix, ascent - height * 0.35);
                path.line_to(prefix + hook * 0.25, ascent - height * 0.4);
                path.line_to(prefix + hook * 0.5, height - metrics.rule);
                path.line_to(prefix + hook * 0.9, metrics.rule);
                path.line_to(width, metrics.rule);
                if let Some(path) = path.finish() {
                    b.commands.push(Command::Path {
                        source: Some(id),
                        path,
                        transform: Transform::identity(),
                        fill: None,
                        even_odd: false,
                        stroke: Some(Box::new(Stroke {
                            brush: Brush::Solid(style.text_color),
                            width: metrics.rule,
                            cap: tiny_skia::LineCap::Butt,
                            join: tiny_skia::LineJoin::Miter,
                            miter: 4.0,
                            dash: Vec::new(),
                            dash_offset: 0.0,
                        })),
                    });
                }
                if let Some(degree) = degree {
                    let baseline = (ascent + descent) * 0.35;
                    b.place(degree, 0.0, baseline);
                }
                b
            }
            "msub" | "msup" | "msubsup"
                if children.len() == if e.tag_name() == "msubsup" { 3 } else { 2 } =>
            {
                let base = self.node(children[0], scale, display, depth + 1);
                let sub = if e.tag_name() != "msup" {
                    Some(self.node(children[1], scale * metrics.script, false, depth + 1))
                } else {
                    None
                };
                let sup = if e.tag_name() != "msub" {
                    Some(self.node(
                        *children.last().expect("script"),
                        scale * metrics.script,
                        false,
                        depth + 1,
                    ))
                } else {
                    None
                };
                self.scripts(base, sub, sup, metrics, style.font_size)
            }
            "munder" | "mover" | "munderover"
                if children.len() == if e.tag_name() == "munderover" { 3 } else { 2 } =>
            {
                let base = self.node(children[0], scale, display, depth + 1);
                let under = if e.tag_name() != "mover" {
                    Some(self.node(children[1], scale * metrics.script, false, depth + 1))
                } else {
                    None
                };
                let over = if e.tag_name() != "munder" {
                    Some(self.node(
                        *children.last().expect("limit"),
                        scale * metrics.script,
                        false,
                        depth + 1,
                    ))
                } else {
                    None
                };
                let movable = self
                    .source
                    .document
                    .node(children[0])
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| {
                        e.attribute("movablelimits") == Some("true")
                            || matches!(self.text(children[0]).as_str(), "∑" | "∏" | "∫")
                    });
                if movable && !display {
                    self.scripts(base, under, over, metrics, style.font_size)
                } else {
                    let gap = metrics.gap;
                    let width = base
                        .width
                        .max(under.as_ref().map_or(0.0, |b| b.width))
                        .max(over.as_ref().map_or(0.0, |b| b.width));
                    let top = over.as_ref().map_or(0.0, |b| b.height() + gap);
                    let ascent = base.ascent + top;
                    let descent = base.descent + under.as_ref().map_or(0.0, |b| b.height() + gap);
                    let bottom = ascent + base.descent;
                    let mut b = MathBox {
                        width,
                        ascent,
                        descent,
                        commands: Vec::new(),
                    };
                    let x = (width - base.width) * 0.5;
                    b.place(base, x, ascent);
                    if let Some(over) = over {
                        let (x, baseline) = ((width - over.width) * 0.5, over.ascent);
                        b.place(over, x, baseline);
                    }
                    if let Some(under) = under {
                        let (x, baseline) =
                            ((width - under.width) * 0.5, bottom + gap + under.ascent);
                        b.place(under, x, baseline);
                    }
                    if e.attribute("accent") == Some("true")
                        || e.attribute("accentunder") == Some("true")
                    {
                        self.warn(id,"MathML accent attachment/assembly is unsupported; centered limits used");
                    }
                    b
                }
            }
            "mmultiscripts" => {
                self.multiscripts(id, &children, scale, display, depth, metrics, &style)
            }
            "mtable" => self.table(id, &children, scale, display, depth, &style),
            "mphantom" => {
                let mut b = self.row(id, &children, scale, display, depth, &style);
                b.commands.clear();
                b
            }
            "mpadded" => {
                let mut b = self.row(id, &children, scale, display, depth, &style);
                for name in ["width", "height", "depth", "lspace", "voffset"] {
                    if let Some(value) = e.attribute(name) {
                        let Some(length) = math_length(Some(value), style.font_size) else {
                            self.warn(
                                id,
                                "relative/pseudo-unit mpadded dimensions are unsupported",
                            );
                            continue;
                        };
                        match name {
                            "width" => b.width = length.max(0.0),
                            "height" => b.ascent = length.max(0.0),
                            "depth" => b.descent = length.max(0.0),
                            _ => {
                                let commands = std::mem::take(&mut b.commands);
                                b.commands.push(Command::Group {
                                    scene: Arc::new(Scene {
                                        width: b.width,
                                        height: b.height(),
                                        baseline: Some(b.ascent),
                                        commands,
                                    }),
                                    transform: Transform::from_translate(
                                        if name == "lspace" { length } else { 0.0 },
                                        if name == "voffset" { -length } else { 0.0 },
                                    ),
                                    opacity: 1.0,
                                    clip: None,
                                });
                            }
                        }
                    }
                }
                b
            }
            "merror" => {
                let mut b = self.row(id, &children, scale, display, depth, &style);
                let commands = std::mem::take(&mut b.commands);
                b.rule(
                    id,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: b.width,
                        height: b.height(),
                    },
                    super::color("#ffcccc", style.text_color).expect("color"),
                );
                b.commands.extend(commands);
                b
            }
            other => {
                self.warn(
                    id,
                    format!("unsupported or invalid MathML {other}; row fallback"),
                );
                self.row(id, &children, scale, display, depth, &style)
            }
        };
        if let Some(color) = e
            .attribute("mathbackground")
            .and_then(|v| super::color(v, style.text_color))
        {
            let commands = std::mem::take(&mut result.commands);
            result.rule(
                id,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: result.width,
                    height: result.height(),
                },
                color,
            );
            result.commands.extend(commands);
        }
        if !result.width.is_finite()
            || !result.height().is_finite()
            || result.width > 1_000_000.0
            || result.height() > 1_000_000.0
        {
            self.warn(id, "MathML geometry exceeds finite budget");
            return MathBox::default();
        }
        result
    }
    fn token(
        &mut self,
        id: NodeId,
        e: &crate::dom::ElementData,
        style: &ComputedStyle,
        display: bool,
    ) -> MathBox {
        let (mut text, mut spans) = super::dom_text(&self.source.document, id);
        if e.tag_name() == "ms" {
            let prefix = e.attribute("lquote").unwrap_or("\"");
            for (start, end, _) in &mut spans {
                *start += prefix.len();
                *end += prefix.len();
            }
            text = format!(
                "{}{}{}",
                prefix,
                text,
                e.attribute("rquote").unwrap_or("\"")
            );
        }
        self.text_bytes += text.len();
        if text.len() >= crate::text::MAX_TEXT_BYTES || self.text_bytes > 262_144 {
            self.warn(id, "MathML text budget exceeds 64 KiB/run or 256 KiB/scene");
            return MathBox::default();
        }
        let mut style = style.clone();
        let variant = e.attribute("mathvariant");
        if (e.tag_name() == "mi" && text.chars().count() == 1 && variant != Some("normal"))
            || variant == Some("italic")
        {
            style.font_style = FontStyle::Italic;
            style.italic = true;
        } else if variant == Some("normal") {
            style.font_style = FontStyle::Normal;
            style.italic = false;
        }
        if variant == Some("bold") {
            style.font_weight = crate::css::values::FontWeight(700.0);
            style.bold = true;
        }
        if variant.is_some_and(|v| !matches!(v, "normal" | "italic" | "bold")) {
            self.warn(id, "unsupported MathML mathvariant");
        }
        if e.tag_name() == "mo"
            && display
            && (e.attribute("largeop") == Some("true") || matches!(text.as_str(), "∑" | "∏" | "∫"))
        {
            style.font_size *= 1.4;
        }
        let shaped = super::shape_dom(self.fonts, &style, &text, &spans, id);
        self.glyph_count += shaped.glyphs.len();
        if self.glyph_count > 65_536 {
            self.warn(id, "MathML glyph budget exceeds 65,536");
            return MathBox::default();
        }
        // Tokens retain the actual DOM text owner, including across fragments.
        let text_owner = self
            .source
            .document
            .node(id)
            .expect("token")
            .children()
            .iter()
            .copied()
            .find(|child| {
                matches!(
                    self.source.document.node(*child).map(|n| n.kind()),
                    Some(NodeKind::Text(_))
                )
            })
            .unwrap_or(id);
        let ascent = shaped.ascent;
        let descent = shaped.descent;
        MathBox {
            width: shaped.width,
            ascent,
            descent,
            commands: vec![Command::Text {
                source: text_owner,
                glyphs: shaped,
                x: 0.0,
                baseline: ascent,
                color: style.text_color,
                transform: Transform::identity(),
            }],
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        id: NodeId,
        children: &[NodeId],
        scale: f32,
        display: bool,
        depth: usize,
        style: &ComputedStyle,
    ) -> MathBox {
        let mut boxes = children
            .iter()
            .map(|child| self.node(*child, scale, display, depth + 1))
            .collect::<Vec<_>>();
        let (ascent, descent) = boxes
            .iter()
            .zip(children)
            .filter(|(_, child)| !self.fence(**child))
            .fold((0.0_f32, 0.0_f32), |(a, d), (b, _)| {
                (a.max(b.ascent), d.max(b.descent))
            });
        if ascent + descent > 0.0 {
            for (b, child) in boxes.iter_mut().zip(children) {
                if self.fence(*child) && b.height() > 0.0 && b.height() < ascent + descent {
                    let sy = (ascent + descent) / b.height();
                    let commands = std::mem::take(&mut b.commands);
                    b.commands.push(Command::Group {
                        scene: Arc::new(Scene {
                            width: b.width,
                            height: b.height(),
                            baseline: Some(b.ascent),
                            commands,
                        }),
                        transform: Transform::from_scale(1.0, sy),
                        opacity: 1.0,
                        clip: None,
                    });
                    b.ascent = ascent;
                    b.descent = descent;
                }
            }
        }
        let mut row = MathBox::default();
        for b in &boxes {
            row.ascent = row.ascent.max(b.ascent);
            row.descent = row.descent.max(b.descent);
        }
        let rtl = style.direction == crate::style::Direction::Rtl;
        let mut participants = boxes
            .into_iter()
            .zip(children)
            .enumerate()
            .collect::<Vec<_>>();
        if rtl {
            participants.reverse();
        }
        for (index, (b, &child)) in participants {
            let (before, after) = self.spacing(child, index, children.len(), style.font_size);
            row.width += before;
            let x = row.width;
            row.width += b.width + after;
            row.place(b, x, row.ascent);
        }
        if self
            .source
            .document
            .node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|e| e.attribute("dir") == Some("rtl"))
            && !rtl
        {
            self.warn(
                id,
                "MathML dir attribute requires CSS direction:rtl in this finite layout",
            );
        }
        row
    }
    fn fence(&self, id: NodeId) -> bool {
        self.source
            .document
            .node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|e| {
                e.tag_name() == "mo"
                    && e.attribute("stretchy") != Some("false")
                    && (e.attribute("fence") == Some("true")
                        || matches!(
                            self.text(id).as_str(),
                            "(" | ")" | "[" | "]" | "{" | "}" | "|"
                        ))
            })
    }
    fn spacing(&self, id: NodeId, index: usize, count: usize, em: f32) -> (f32, f32) {
        let Some(e) = self
            .source
            .document
            .node(id)
            .and_then(|n| n.as_element())
            .filter(|e| e.tag_name() == "mo")
        else {
            return (0.0, 0.0);
        };
        let text = self.text(id);
        let form = e.attribute("form").unwrap_or(if index == 0 {
            "prefix"
        } else if index + 1 == count {
            "postfix"
        } else {
            "infix"
        });
        let default = if self.fence(id) {
            0.0
        } else if matches!(text.as_str(), "=" | "<" | ">" | "≤" | "≥" | "≠" | "≈") {
            em * 5.0 / 18.0
        } else if form == "infix"
            && matches!(text.as_str(), "+" | "-" | "−" | "×" | "÷" | "*" | "·" | "∈")
        {
            em * 4.0 / 18.0
        } else if text == "," || text == ";" {
            em * 3.0 / 18.0
        } else {
            0.0
        };
        (
            math_length(e.attribute("lspace"), em).unwrap_or(if text == "," {
                0.0
            } else {
                default
            }),
            math_length(e.attribute("rspace"), em).unwrap_or(default),
        )
    }
    fn scripts(
        &self,
        base: MathBox,
        sub: Option<MathBox>,
        sup: Option<MathBox>,
        metrics: Metrics,
        em: f32,
    ) -> MathBox {
        let mut up = metrics.sup.max(base.ascent * 0.6);
        let mut down = metrics.sub.max(base.descent * 0.8);
        if let Some(sup) = &sup {
            up = up.max(sup.descent + em * 0.1);
        }
        if let (Some(sub), Some(sup)) = (&sub, &sup) {
            let gap = up + down - sup.descent - sub.ascent;
            if gap < metrics.script_gap {
                down += metrics.script_gap - gap;
            }
        }
        let ascent = base.ascent.max(sup.as_ref().map_or(0.0, |b| up + b.ascent));
        let descent = base
            .descent
            .max(sub.as_ref().map_or(0.0, |b| down + b.descent));
        let x = base.width + em * 0.05;
        let width = x
            + sub
                .as_ref()
                .map_or(0.0, |b| b.width)
                .max(sup.as_ref().map_or(0.0, |b| b.width))
            + em * 0.05;
        let mut b = MathBox {
            width,
            ascent,
            descent,
            commands: Vec::new(),
        };
        b.place(base, 0.0, ascent);
        if let Some(sub) = sub {
            b.place(sub, x, ascent + down);
        }
        if let Some(sup) = sup {
            b.place(sup, x, ascent - up);
        }
        b
    }
    #[allow(clippy::too_many_arguments)]
    fn multiscripts(
        &mut self,
        id: NodeId,
        children: &[NodeId],
        scale: f32,
        display: bool,
        depth: usize,
        metrics: Metrics,
        style: &ComputedStyle,
    ) -> MathBox {
        let Some(&base) = children.first() else {
            return MathBox::default();
        };
        if children.len() > 33 {
            self.warn(id, "MathML multiscript budget exceeds 16 pairs");
            return MathBox::default();
        }
        let mut result = self.node(base, scale, display, depth + 1);
        let split = children.iter().position(|id| {
            self.source
                .document
                .node(*id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.tag_name() == "mprescripts")
        });
        let posts = &children[1..split.unwrap_or(children.len())];
        for pair in posts.chunks(2) {
            if pair.len() != 2 {
                self.warn(id, "multiscripts requires sub/sup pairs");
                break;
            }
            let sub = self.node(pair[0], scale * metrics.script, false, depth + 1);
            let sup = self.node(pair[1], scale * metrics.script, false, depth + 1);
            result = self.scripts(result, Some(sub), Some(sup), metrics, style.font_size);
        }
        if let Some(split) = split {
            for pair in children[split + 1..].chunks(2).rev() {
                if pair.len() != 2 {
                    self.warn(id, "prescripts requires sub/sup pairs");
                    continue;
                }
                let sub = self.node(pair[0], scale * metrics.script, false, depth + 1);
                let sup = self.node(pair[1], scale * metrics.script, false, depth + 1);
                let empty = MathBox {
                    ascent: result.ascent,
                    descent: result.descent,
                    ..Default::default()
                };
                let scripts = self.scripts(empty, Some(sub), Some(sup), metrics, style.font_size);
                let (width, ascent, descent) = (
                    scripts.width + result.width,
                    scripts.ascent.max(result.ascent),
                    scripts.descent.max(result.descent),
                );
                let mut b = MathBox {
                    width,
                    ascent,
                    descent,
                    commands: Vec::new(),
                };
                let x = scripts.width;
                b.place(scripts, 0.0, ascent);
                b.place(result, x, ascent);
                result = b;
            }
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn table(
        &mut self,
        id: NodeId,
        children: &[NodeId],
        scale: f32,
        display: bool,
        depth: usize,
        style: &ComputedStyle,
    ) -> MathBox {
        if children.len() > 64 {
            self.warn(id, "MathML table exceeds 64 rows");
            return MathBox::default();
        }
        let mut rows = Vec::new();
        let mut widths = Vec::<f32>::new();
        for &row in children {
            if !self
                .source
                .document
                .node(row)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.tag_name() == "mtr")
            {
                self.warn(row, "MathML table only supports mtr rows");
                continue;
            }
            let cells = self.children(row);
            if cells.len() > 64 {
                self.warn(row, "MathML table exceeds 64 columns");
                continue;
            }
            let mut boxes = Vec::new();
            for (index, cell) in cells.into_iter().enumerate() {
                let e = self
                    .source
                    .document
                    .node(cell)
                    .and_then(|n| n.as_element())
                    .expect("cell");
                if e.attribute("rowspan").is_some() || e.attribute("columnspan").is_some() {
                    self.warn(cell, "MathML table spans are unsupported");
                }
                let b = self.node(cell, scale, display, depth + 1);
                if index >= widths.len() {
                    widths.push(b.width);
                } else {
                    widths[index] = widths[index].max(b.width);
                }
                boxes.push(b);
            }
            rows.push(boxes);
        }
        let gap = style.font_size * 0.5;
        let width = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
        let height = rows
            .iter()
            .map(|r| {
                r.iter().map(|b| b.ascent).fold(0.0, f32::max)
                    + r.iter().map(|b| b.descent).fold(0.0, f32::max)
            })
            .sum::<f32>()
            + gap * rows.len().saturating_sub(1) as f32;
        let axis = Metrics::new(self.fonts, style).axis;
        let ascent = height * 0.5 + axis;
        let mut b = MathBox {
            width,
            ascent,
            descent: (height - ascent).max(0.0),
            commands: Vec::new(),
        };
        let mut y = 0.0;
        for row in rows {
            let a = row.iter().map(|b| b.ascent).fold(0.0, f32::max);
            let d = row.iter().map(|b| b.descent).fold(0.0, f32::max);
            let mut x = 0.0;
            for (cell, &width) in row.into_iter().zip(&widths) {
                let center = x + (width - cell.width) * 0.5;
                b.place(cell, center, y + a);
                x += width + gap;
            }
            y += a + d + gap;
        }
        b
    }
}

fn math_length(value: Option<&str>, em: f32) -> Option<f32> {
    let value = value?;
    let mu = match value {
        "veryverythinmathspace" => 1.0,
        "verythinmathspace" => 2.0,
        "thinmathspace" => 3.0,
        "mediummathspace" => 4.0,
        "thickmathspace" => 5.0,
        "verythickmathspace" => 6.0,
        "veryverythickmathspace" => 7.0,
        _ => return super::svg::length(Some(value), em, em),
    };
    Some(em * mu / 18.0)
}
fn aligned(value: Option<&str>, width: f32, child: f32) -> f32 {
    match value {
        Some("left") => 0.0,
        Some("right") => width - child,
        _ => (width - child) * 0.5,
    }
}
