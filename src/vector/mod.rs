//! Native, immutable vector geometry. SVG and MathML share the existing shaped
//! glyph sources; rasterization is deferred until a display item is painted.
mod math;
pub(crate) mod raster;
mod svg;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::{
    css::values::ResolvedColor,
    dom::{Document, Namespace, NodeId},
    media::{DecodedImage, ImageMap},
    style::{ComputedStyles, Diagnostic},
    text::{FontContext, ShapedText, SourceSpan},
};
use tiny_skia::{Path, Transform};

pub const MAX_COMMANDS: usize = 16_384;
pub const MAX_DEPTH: usize = 32;
pub const MAX_VECTOR_PIXELS: usize = 4_194_304;

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    /// Math baseline, from the viewport top. SVG uses the replaced bottom edge.
    pub baseline: Option<f32>,
    pub commands: Vec<Command>,
}

impl Scene {
    pub(crate) fn empty(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            baseline: None,
            commands: Vec::new(),
        }
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        let mut bytes = self.commands.len() * std::mem::size_of::<Command>();
        for command in &self.commands {
            bytes += match command {
                Command::Path {
                    path, fill, stroke, ..
                } => {
                    let mut bytes = std::mem::size_of_val(path.points());
                    for brush in fill.iter().chain(stroke.iter().map(|s| &s.brush)) {
                        bytes += match brush {
                            Brush::Pattern { scene, .. } => scene.estimated_bytes(),
                            Brush::Linear { stops, .. } | Brush::Radial { stops, .. } => {
                                stops.len() * std::mem::size_of::<(f32, ResolvedColor)>()
                            }
                            _ => 0,
                        };
                    }
                    bytes
                }
                Command::Text { glyphs, .. } => {
                    glyphs.glyphs.len() * (std::mem::size_of::<crate::text::ShapedGlyph>() + 64)
                }
                Command::Group { scene, .. } => scene.estimated_bytes(),
                Command::Image { image, .. } => image.decoded_bytes(),
            };
        }
        bytes
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(ResolvedColor),
    Linear {
        start: (f32, f32),
        end: (f32, f32),
        stops: Vec<(f32, ResolvedColor)>,
        spread: Spread,
        transform: Transform,
    },
    Radial {
        center: (f32, f32),
        focus: (f32, f32),
        radius: f32,
        stops: Vec<(f32, ResolvedColor)>,
        spread: Spread,
        transform: Transform,
    },
    Pattern {
        scene: Arc<Scene>,
        tile: crate::layout::Rect,
        transform: Transform,
        opacity: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spread {
    Pad,
    Repeat,
    Reflect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub brush: Brush,
    pub width: f32,
    pub cap: tiny_skia::LineCap,
    pub join: tiny_skia::LineJoin,
    pub miter: f32,
    pub dash: Vec<f32>,
    pub dash_offset: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Path {
        source: Option<NodeId>,
        path: Path,
        transform: Transform,
        fill: Option<Brush>,
        even_odd: bool,
        stroke: Option<Box<Stroke>>,
    },
    Text {
        source: NodeId,
        glyphs: Arc<ShapedText>,
        x: f32,
        baseline: f32,
        color: ResolvedColor,
        transform: Transform,
    },
    Image {
        source: NodeId,
        image: Arc<DecodedImage>,
        rect: crate::layout::Rect,
        transform: Transform,
    },
    Group {
        scene: Arc<Scene>,
        transform: Transform,
        opacity: f32,
        clip: Option<crate::layout::Rect>,
    },
}

/// Retained DOM/styles/resource snapshot, compiled with the *used* document
/// font context before the common intrinsic-sizing pass.
#[derive(Clone, Debug)]
pub(crate) struct Source {
    pub document: Arc<Document>,
    pub styles: Arc<ComputedStyles>,
    pub images: ImageMap,
    pub root: NodeId,
}

pub(crate) fn root(element: &crate::dom::ElementData) -> bool {
    matches!(
        (element.namespace(), element.tag_name()),
        (Namespace::Svg, "svg") | (Namespace::MathMl, "math")
    )
}

impl Source {
    pub fn compile(&self, fonts: &FontContext, diagnostics: &mut Vec<Diagnostic>) -> Arc<Scene> {
        if self
            .document
            .node(self.root)
            .and_then(|n| n.as_element())
            .is_some_and(|e| *e.namespace() == Namespace::Svg)
        {
            Arc::new(svg::build(self, fonts, diagnostics, None))
        } else {
            Arc::new(math::build(self, fonts, diagnostics))
        }
    }

    pub(crate) fn svg_viewport(
        &self,
        fonts: &FontContext,
        diagnostics: &mut Vec<Diagnostic>,
        width: f32,
        height: f32,
    ) -> Option<Arc<Scene>> {
        self.document
            .node(self.root)
            .and_then(|n| n.as_element())
            .filter(|e| *e.namespace() == Namespace::Svg)
            .map(|_| Arc::new(svg::build(self, fonts, diagnostics, Some((width, height)))))
    }
}

pub(crate) fn diagnose(
    diagnostics: &mut Vec<Diagnostic>,
    node: NodeId,
    message: impl Into<String>,
) {
    let diagnostic = Diagnostic {
        node_id: Some(node),
        message: format!("vector: {}", message.into()),
    };
    if diagnostics.len() < 256 && !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
    }
}

pub(crate) fn shape(
    fonts: &FontContext,
    style: &crate::style::ComputedStyle,
    text: &str,
    node: NodeId,
) -> Arc<ShapedText> {
    let mut shaped = fonts.shape(style, text).as_ref().clone();
    for glyph in &mut shaped.glyphs {
        glyph.source_spans = Arc::from([SourceSpan {
            node_id: Some(node),
            source: Arc::from(text),
            start: glyph.cluster,
            end: glyph.cluster_end.min(text.len()),
        }]);
    }
    Arc::new(shaped)
}

pub(crate) fn dom_text(
    document: &Document,
    root: NodeId,
) -> (String, Vec<(usize, usize, SourceSpan)>) {
    fn collect(
        document: &Document,
        id: NodeId,
        depth: usize,
        text: &mut String,
        spans: &mut Vec<(usize, usize, SourceSpan)>,
        pending: &mut Option<SourceSpan>,
    ) {
        if depth > MAX_DEPTH || text.len() >= crate::text::MAX_TEXT_BYTES {
            return;
        }
        let Some(node) = document.node(id) else {
            return;
        };
        if let crate::dom::NodeKind::Text(raw) = node.kind() {
            let source: Arc<str> = Arc::from(raw.as_str());
            for (offset, ch) in raw.char_indices() {
                if text.len() >= crate::text::MAX_TEXT_BYTES {
                    break;
                }
                let span = SourceSpan {
                    node_id: Some(id),
                    source: source.clone(),
                    start: offset,
                    end: offset + ch.len_utf8(),
                };
                if ch.is_ascii_whitespace() {
                    if !text.is_empty() && pending.is_none() {
                        *pending = Some(span);
                    }
                    continue;
                }
                if let Some(space) = pending.take() {
                    let start = text.len();
                    text.push(' ');
                    spans.push((start, text.len(), space));
                }
                let start = text.len();
                text.push(ch);
                spans.push((start, text.len(), span));
            }
        }
        for &child in node.children() {
            collect(document, child, depth + 1, text, spans, pending);
        }
    }
    let (mut text, mut spans) = (String::new(), Vec::new());
    collect(document, root, 0, &mut text, &mut spans, &mut None);
    (text, spans)
}

pub(crate) fn shape_dom(
    fonts: &FontContext,
    style: &crate::style::ComputedStyle,
    text: &str,
    spans: &[(usize, usize, SourceSpan)],
    node: NodeId,
) -> Arc<ShapedText> {
    let mut shaped = shape(fonts, style, text, node).as_ref().clone();
    for glyph in &mut shaped.glyphs {
        let start = spans.partition_point(|(_, end, _)| *end <= glyph.cluster);
        let sources = spans[start..]
            .iter()
            .take_while(|(start, _, _)| *start < glyph.cluster_end)
            .map(|(_, _, s)| s.clone())
            .collect::<Vec<_>>();
        if !sources.is_empty() {
            glyph.source_spans = Arc::from(sources);
        }
    }
    Arc::new(shaped)
}

pub(crate) fn href(element: &crate::dom::ElementData) -> Option<&str> {
    element.attribute("href").or_else(|| {
        element
            .attributes()
            .iter()
            .find(|a| a.name() == "href" && a.namespace() == Some("http://www.w3.org/1999/xlink"))
            .map(|a| a.value())
    })
}
pub(crate) fn color(value: &str, current: ResolvedColor) -> Option<ResolvedColor> {
    crate::css::values::parse_color(value)?.resolve(current)
}

pub(crate) fn rect_path(rect: crate::layout::Rect) -> Option<Path> {
    let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)?;
    Some(tiny_skia::PathBuilder::from_rect(r))
}

/// SVG image decoding never performs nested I/O or system font discovery.
/// Inline SVG images use the Frame's normal resource loader instead.
pub(crate) fn decode(bytes: &[u8]) -> Result<Arc<Scene>, Box<dyn std::error::Error>> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("SVG image exceeds 4 MiB".into());
    }
    let document = crate::xml::parse_bytes(bytes, Some("image/svg+xml"))?;
    let root = document
        .preorder()
        .find(|id| {
            document
                .node(*id)
                .and_then(|n| n.as_element())
                .is_some_and(|e| *e.namespace() == Namespace::Svg && e.tag_name() == "svg")
        })
        .ok_or("SVG image requires a namespaced svg root")?;
    let resolution = crate::style::resolve(&document);
    let mut diagnostics = resolution.stylesheets.diagnostics;
    for id in document.preorder() {
        if document
            .node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|e| e.tag_name() == "style")
            && dom_text(&document, id)
                .0
                .to_ascii_lowercase()
                .contains("@font-face")
        {
            diagnose(
                &mut diagnostics,
                id,
                "SVG image font downloads are unsupported; no nested font I/O is performed",
            );
        }
    }
    let source = Source {
        document: Arc::new(document),
        styles: Arc::new(resolution.styles),
        images: ImageMap::new(),
        root,
    };
    let scene = source.compile(&FontContext::bundled(), &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
            .into());
    }
    if scene.width <= 0.0 || scene.height <= 0.0 || scene.width > 4096.0 || scene.height > 4096.0 {
        return Err("SVG image dimensions must be positive and at most 4096".into());
    }
    Ok(scene)
}
