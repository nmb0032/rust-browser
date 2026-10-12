use std::{collections::HashMap, sync::Arc};

use crate::{
    css::values::ResolvedColor,
    dom::{ElementData, Namespace, NodeId, NodeKind},
    layout::Rect,
    style::{ComputedStyle, Diagnostic, Display},
    text::FontContext,
};
use tiny_skia::{Path, PathBuilder, Transform};

use super::{Brush, Command, Scene, Source, Spread, Stroke, diagnose};

#[derive(Clone)]
struct Paint {
    fill: String,
    stroke: String,
    fill_opacity: f32,
    stroke_opacity: f32,
    width: f32,
    even_odd: bool,
    cap: tiny_skia::LineCap,
    join: tiny_skia::LineJoin,
    miter: f32,
    dash: Vec<f32>,
    dash_offset: f32,
    markers: [String; 3],
    anchor: String,
    font: ComputedStyle,
}

impl Paint {
    fn new(font: ComputedStyle) -> Self {
        Self {
            fill: "black".into(),
            stroke: "none".into(),
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            width: 1.0,
            even_odd: false,
            cap: tiny_skia::LineCap::Butt,
            join: tiny_skia::LineJoin::Miter,
            miter: 4.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            markers: Default::default(),
            anchor: "start".into(),
            font,
        }
    }
}

struct Builder<'a> {
    source: &'a Source,
    fonts: &'a FontContext,
    diagnostics: &'a mut Vec<Diagnostic>,
    ids: HashMap<String, NodeId>,
    references: Vec<NodeId>,
    commands: usize,
    visits: usize,
    segments: usize,
    text_bytes: usize,
    glyph_count: usize,
    viewport: (f32, f32),
}

pub(super) fn build(
    source: &Source,
    fonts: &FontContext,
    diagnostics: &mut Vec<Diagnostic>,
    used_viewport: Option<(f32, f32)>,
) -> Scene {
    let element = source
        .document
        .node(source.root)
        .and_then(|n| n.as_element())
        .expect("SVG root");
    let font = source.styles.get(source.root).cloned().unwrap_or_default();
    let vb = view_box(element);
    let width = used_viewport.map_or_else(
        || length(element.attribute("width"), 300.0, font.font_size).unwrap_or(300.0),
        |v| v.0,
    );
    let height = used_viewport.map_or_else(
        || length(element.attribute("height"), 150.0, font.font_size).unwrap_or(150.0),
        |v| v.1,
    );
    let mut builder = Builder {
        source,
        fonts,
        diagnostics,
        ids: HashMap::new(),
        references: Vec::new(),
        commands: 0,
        visits: 0,
        segments: 0,
        text_bytes: 0,
        glyph_count: 0,
        viewport: vb.map_or((width, height), |v| (v.width, v.height)),
    };
    for id in source.document.preorder() {
        if let Some(e) = source.document.node(id).and_then(|n| n.as_element())
            && *e.namespace() == Namespace::Svg
            && let Some(name) = e.attribute("id")
        {
            builder.ids.entry(name.to_owned()).or_insert(id);
        }
    }
    if element.attribute("viewBox").is_some() && vb.is_none() {
        builder.warn(source.root, "invalid viewBox; ignored");
    }
    let mut scene = Scene::empty(width, height);
    if width <= 0.0 || height <= 0.0 {
        return scene;
    }
    let mut body = Scene::empty(width, height);
    let paint = builder.paint(source.root, &Paint::new(font));
    builder.children(source.root, &paint, Transform::identity(), &mut body, 0);
    if ["width", "height"]
        .iter()
        .any(|name| element.attribute(name).is_some_and(|v| v.contains('%')))
    {
        builder.warn(source.root, "percentage SVG root dimensions use the default object viewport, not a containing-block percentage");
    }
    let mut viewport = Scene::empty(width, height);
    viewport.commands.push(Command::Group {
        scene: Arc::new(body),
        transform: element
            .attribute("transform")
            .and_then(transform_list)
            .unwrap_or_default()
            .pre_concat(vb.map_or(Transform::identity(), |v| {
                view_transform(v, width, height, element.attribute("preserveAspectRatio"))
            })),
        opacity: 1.0,
        clip: None,
    });
    scene.commands.push(Command::Group {
        scene: Arc::new(viewport),
        transform: Transform::identity(),
        opacity: number(element.attribute("opacity"), 1.0).clamp(0.0, 1.0),
        clip: Some(Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }),
    });
    scene
}

impl Builder<'_> {
    fn element(&self, id: NodeId) -> &ElementData {
        self.source
            .document
            .node(id)
            .and_then(|n| n.as_element())
            .expect("SVG element")
    }
    fn warn(&mut self, id: NodeId, message: impl Into<String>) {
        diagnose(self.diagnostics, id, message);
    }
    fn paint(&mut self, id: NodeId, parent: &Paint) -> Paint {
        let e = self.element(id).clone();
        if id != self.source.root
            && self
                .source
                .styles
                .get(id)
                .is_some_and(|s| s.effects.context())
        {
            self.warn(id, "CSS transforms/effects on inner SVG nodes are unsupported; SVG transform/opacity attributes remain executable");
        }
        let mut paint = parent.clone();
        if let Some(style) = self.source.styles.get(id) {
            paint.font = style.clone();
            let original_parent = self
                .source
                .document
                .node(id)
                .and_then(|n| n.parent())
                .and_then(|id| self.source.styles.get(id));
            if original_parent.is_some_and(|p| p.font_family == style.font_family) {
                paint.font.font_family = parent.font.font_family.clone();
            }
            if original_parent.is_some_and(|p| p.font_style == style.font_style) {
                paint.font.font_style = parent.font.font_style;
                paint.font.italic = parent.font.italic;
            }
            if original_parent.is_some_and(|p| p.font_weight == style.font_weight) {
                paint.font.font_weight = parent.font.font_weight;
            }
        }
        // Presentation font attributes are inherited in SVG user units.
        paint.font.font_size = length(
            e.attribute("font-size"),
            self.viewport.1,
            parent.font.font_size,
        )
        .unwrap_or_else(|| {
            self.source
                .styles
                .get(id)
                .zip(
                    self.source
                        .document
                        .node(id)
                        .and_then(|n| n.parent())
                        .and_then(|id| self.source.styles.get(id)),
                )
                .filter(|(a, b)| a.font_size != b.font_size)
                .map_or(parent.font.font_size, |(a, _)| a.font_size)
        });
        if let Some(c) = e
            .attribute("color")
            .and_then(|v| super::color(v, parent.font.text_color))
        {
            paint.font.text_color = c;
            paint.font.text_fill_color = c;
        }
        if let Some(value) = e.attribute("font-family").filter(|v| v.len() <= 4096) {
            paint.font.font_family = crate::css::values::FontFamily(
                value
                    .split(',')
                    .take(16)
                    .map(|name| {
                        let name = name.trim().trim_matches(['\'', '"']);
                        if matches!(name, "serif" | "sans-serif" | "monospace" | "math") {
                            crate::css::values::FamilyName::Generic(name.into())
                        } else {
                            crate::css::values::FamilyName::Named(name.into())
                        }
                    })
                    .collect(),
            );
        }
        if e.attribute("font-style") == Some("italic") {
            paint.font.font_style = crate::css::values::FontStyle::Italic;
            paint.font.italic = true;
        }
        if let Some(weight) = e
            .attribute("font-weight")
            .and_then(|v| v.parse::<u16>().ok())
        {
            paint.font.font_weight = crate::css::values::FontWeight(weight as f32);
        }
        for (name, target) in [
            ("fill", &mut paint.fill),
            ("stroke", &mut paint.stroke),
            ("text-anchor", &mut paint.anchor),
            ("marker-start", &mut paint.markers[0]),
        ] {
            if let Some(value) = e.attribute(name) {
                *target = value.to_owned();
            }
        }
        for (i, name) in ["marker-mid", "marker-end"].iter().enumerate() {
            if let Some(value) = e.attribute(name) {
                paint.markers[i + 1] = value.into();
            }
        }
        paint.fill_opacity =
            number(e.attribute("fill-opacity"), parent.fill_opacity).clamp(0.0, 1.0);
        paint.stroke_opacity =
            number(e.attribute("stroke-opacity"), parent.stroke_opacity).clamp(0.0, 1.0);
        paint.width = length(
            e.attribute("stroke-width"),
            self.viewport.0,
            paint.font.font_size,
        )
        .unwrap_or(parent.width)
        .max(0.0);
        paint.even_odd = e
            .attribute("fill-rule")
            .map_or(parent.even_odd, |v| v == "evenodd");
        paint.cap = match e.attribute("stroke-linecap") {
            Some("round") => tiny_skia::LineCap::Round,
            Some("square") => tiny_skia::LineCap::Square,
            Some("butt") => tiny_skia::LineCap::Butt,
            _ => parent.cap,
        };
        paint.join = match e.attribute("stroke-linejoin") {
            Some("round") => tiny_skia::LineJoin::Round,
            Some("bevel") => tiny_skia::LineJoin::Bevel,
            Some("miter") => tiny_skia::LineJoin::Miter,
            _ => parent.join,
        };
        paint.miter = number(e.attribute("stroke-miterlimit"), parent.miter).max(1.0);
        if let Some(dash) = e.attribute("stroke-dasharray") {
            if dash == "none" {
                paint.dash.clear();
            } else if let Some(mut values) = numbers(dash)
                && values.len() <= 64
                && values.iter().all(|v| *v >= 0.0)
                && values.iter().sum::<f32>() > 0.0
            {
                if values.len() % 2 == 1 {
                    values.extend(values.clone());
                }
                paint.dash = values;
            } else {
                self.warn(id, "invalid or oversized stroke-dasharray");
            }
        }
        paint.dash_offset = length(
            e.attribute("stroke-dashoffset"),
            self.viewport.0,
            paint.font.font_size,
        )
        .unwrap_or(parent.dash_offset);
        for name in [
            "filter",
            "mask",
            "clip-path",
            "vector-effect",
            "paint-order",
        ] {
            if e.attribute(name)
                .is_some_and(|v| v != "none" && v != "normal")
            {
                self.warn(id, format!("SVG {name} is unsupported"));
            }
        }
        if e.attribute("style").is_some_and(|v| {
            v.split(';').any(|d| {
                d.split_once(':').is_some_and(|(p, _)| {
                    matches!(
                        p.trim(),
                        "fill"
                            | "stroke"
                            | "stop-color"
                            | "stop-opacity"
                            | "marker"
                            | "marker-start"
                            | "marker-end"
                            | "marker-mid"
                            | "text-anchor"
                    )
                })
            })
        }) {
            self.warn(
                id,
                "SVG paint CSS is unsupported; use presentation attributes",
            );
        }
        paint
    }
    fn children(
        &mut self,
        id: NodeId,
        paint: &Paint,
        transform: Transform,
        scene: &mut Scene,
        depth: usize,
    ) {
        let children = self
            .source
            .document
            .node(id)
            .expect("SVG node")
            .children()
            .to_vec();
        for child in children {
            self.node(child, paint, transform, scene, depth + 1, false);
        }
    }
    fn push(&mut self, id: NodeId, scene: &mut Scene, command: Command) {
        if self.commands >= super::MAX_COMMANDS {
            self.warn(id, "SVG command budget exceeded");
            return;
        }
        self.commands += 1;
        scene.commands.push(command);
    }
    fn node(
        &mut self,
        id: NodeId,
        parent: &Paint,
        transform: Transform,
        scene: &mut Scene,
        depth: usize,
        referenced: bool,
    ) {
        self.visits += 1;
        if self.visits > super::MAX_COMMANDS * 2 {
            self.warn(id, "SVG expanded-node budget exceeded");
            return;
        }
        if depth > super::MAX_DEPTH {
            self.warn(id, "SVG depth exceeds 32");
            return;
        }
        let Some(e) = self
            .source
            .document
            .node(id)
            .and_then(|n| n.as_element())
            .cloned()
        else {
            return;
        };
        if e.tag_name() == "script" {
            self.warn(id, "SVG scripts are unsupported and never executed");
            return;
        }
        if *e.namespace() != Namespace::Svg {
            self.warn(id, "foreign SVG content is unsupported");
            return;
        }
        if self
            .source
            .styles
            .get(id)
            .is_some_and(|s| s.display == Display::None)
            || e.attribute("display") == Some("none")
            || e.attribute("visibility") == Some("hidden")
        {
            return;
        }
        let paint = self.paint(id, parent);
        let local = e
            .attribute("transform")
            .and_then(|v| {
                transform_list(v).or_else(|| {
                    self.warn(id, "invalid SVG transform");
                    None
                })
            })
            .unwrap_or_default();
        let transform = transform.pre_concat(local);
        let opacity = number(e.attribute("opacity"), 1.0).clamp(0.0, 1.0);
        if opacity < 1.0 {
            let mut group = Scene::empty(scene.width, scene.height);
            self.content(id, &e, &paint, transform, &mut group, depth, referenced);
            self.push(
                id,
                scene,
                Command::Group {
                    scene: Arc::new(group),
                    transform: Transform::identity(),
                    opacity,
                    clip: None,
                },
            );
        } else {
            self.content(id, &e, &paint, transform, scene, depth, referenced);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn content(
        &mut self,
        id: NodeId,
        e: &ElementData,
        paint: &Paint,
        transform: Transform,
        scene: &mut Scene,
        depth: usize,
        referenced: bool,
    ) {
        match e.tag_name() {
            "defs" | "linearGradient" | "radialGradient" | "pattern" | "marker" | "title"
            | "desc" | "metadata" | "style" => {}
            "symbol" if !referenced => {}
            "g" | "a" | "symbol" => self.children(id, paint, transform, scene, depth),
            "svg" => {
                let width = length(e.attribute("width"), self.viewport.0, paint.font.font_size)
                    .unwrap_or(self.viewport.0);
                let height = length(e.attribute("height"), self.viewport.1, paint.font.font_size)
                    .unwrap_or(self.viewport.1);
                if width <= 0.0 || height <= 0.0 {
                    return;
                }
                let x =
                    length(e.attribute("x"), self.viewport.0, paint.font.font_size).unwrap_or(0.0);
                let y =
                    length(e.attribute("y"), self.viewport.1, paint.font.font_size).unwrap_or(0.0);
                let vb = view_box(e);
                let old = self.viewport;
                self.viewport = vb.map_or((width, height), |v| (v.width, v.height));
                let mut inner = Scene::empty(width, height);
                self.children(
                    id,
                    paint,
                    vb.map_or(Transform::identity(), |v| {
                        view_transform(v, width, height, e.attribute("preserveAspectRatio"))
                    }),
                    &mut inner,
                    depth,
                );
                self.viewport = old;
                self.push(
                    id,
                    scene,
                    Command::Group {
                        scene: Arc::new(inner),
                        transform: transform.pre_translate(x, y),
                        opacity: 1.0,
                        clip: Some(Rect {
                            x: 0.0,
                            y: 0.0,
                            width,
                            height,
                        }),
                    },
                );
            }
            "use" => self.use_node(id, e, paint, transform, scene, depth),
            "text" => {
                let mut cursor = (
                    length(e.attribute("x"), self.viewport.0, paint.font.font_size).unwrap_or(0.0),
                    length(e.attribute("y"), self.viewport.1, paint.font.font_size).unwrap_or(0.0),
                );
                self.text(id, paint, transform, scene, &mut cursor, depth);
            }
            "image" => self.image(id, e, paint, transform, scene),
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                if let Some((path, vertices)) = self.geometry(id, e, paint.font.font_size) {
                    let bounds = path.bounds();
                    let bbox = Rect {
                        x: bounds.x(),
                        y: bounds.y(),
                        width: bounds.width(),
                        height: bounds.height(),
                    };
                    let fill = self.brush(
                        id,
                        &paint.fill,
                        bbox,
                        paint.font.text_color,
                        paint.fill_opacity,
                        depth,
                    );
                    let stroke = self
                        .brush(
                            id,
                            &paint.stroke,
                            bbox,
                            paint.font.text_color,
                            paint.stroke_opacity,
                            depth,
                        )
                        .filter(|_| paint.width > 0.0)
                        .map(|brush| {
                            Box::new(Stroke {
                                brush,
                                width: paint.width,
                                cap: paint.cap,
                                join: paint.join,
                                miter: paint.miter,
                                dash: paint.dash.clone(),
                                dash_offset: paint.dash_offset,
                            })
                        });
                    self.push(
                        id,
                        scene,
                        Command::Path {
                            source: Some(id),
                            path,
                            transform,
                            fill,
                            even_odd: paint.even_odd,
                            stroke,
                        },
                    );
                    self.markers(id, paint, &vertices, transform, scene, depth);
                }
            }
            other => self.warn(id, format!("SVG element {other} is unsupported")),
        }
    }
    fn reference(&mut self, id: NodeId, value: &str) -> Option<NodeId> {
        let target = value
            .trim()
            .strip_prefix('#')
            .and_then(|key| self.ids.get(key))
            .copied();
        if target.is_none() {
            self.warn(id, "SVG reference must resolve within this document");
        }
        target
    }
    fn enter(&mut self, id: NodeId) -> bool {
        if self.references.len() >= super::MAX_DEPTH || self.references.contains(&id) {
            self.warn(id, "cyclic or over-depth SVG reference");
            false
        } else {
            self.references.push(id);
            true
        }
    }
    fn use_node(
        &mut self,
        id: NodeId,
        e: &ElementData,
        paint: &Paint,
        transform: Transform,
        scene: &mut Scene,
        depth: usize,
    ) {
        let Some(target) = super::href(e).and_then(|v| self.reference(id, v)) else {
            self.warn(id, "use requires a local href");
            return;
        };
        if !self.enter(target) {
            return;
        }
        let x = length(e.attribute("x"), self.viewport.0, paint.font.font_size).unwrap_or(0.0);
        let y = length(e.attribute("y"), self.viewport.1, paint.font.font_size).unwrap_or(0.0);
        let mut transform = transform.pre_translate(x, y);
        let target_element = self.element(target);
        if target_element.tag_name() == "symbol" {
            let width = length(e.attribute("width"), self.viewport.0, paint.font.font_size)
                .unwrap_or(self.viewport.0);
            let height = length(e.attribute("height"), self.viewport.1, paint.font.font_size)
                .unwrap_or(self.viewport.1);
            if let Some(vb) = view_box(target_element) {
                transform = transform.pre_concat(view_transform(
                    vb,
                    width,
                    height,
                    target_element.attribute("preserveAspectRatio"),
                ));
            }
        }
        self.node(target, paint, transform, scene, depth + 1, true);
        self.references.pop();
    }
    fn text(
        &mut self,
        id: NodeId,
        paint: &Paint,
        transform: Transform,
        scene: &mut Scene,
        cursor: &mut (f32, f32),
        depth: usize,
    ) {
        if depth > super::MAX_DEPTH {
            self.warn(id, "SVG text depth exceeds 32");
            return;
        }
        let e = self.element(id).clone();
        for (i, name) in ["x", "y"].iter().enumerate() {
            if let Some(value) = e.attribute(name) {
                if numbers(value).is_some_and(|v| v.len() > 1) {
                    self.warn(id, "per-character SVG text position lists are unsupported");
                }
                let coordinate = length(
                    Some(value),
                    if i == 0 {
                        self.viewport.0
                    } else {
                        self.viewport.1
                    },
                    paint.font.font_size,
                )
                .unwrap_or(0.0);
                if i == 0 {
                    cursor.0 = coordinate;
                } else {
                    cursor.1 = coordinate;
                }
            }
        }
        cursor.0 += length(e.attribute("dx"), self.viewport.0, paint.font.font_size).unwrap_or(0.0);
        cursor.1 += length(e.attribute("dy"), self.viewport.1, paint.font.font_size).unwrap_or(0.0);
        for name in ["rotate", "textLength", "lengthAdjust"] {
            if e.attribute(name).is_some() {
                self.warn(id, format!("SVG text {name} is unsupported"));
            }
        }
        let children = self
            .source
            .document
            .node(id)
            .expect("text node")
            .children()
            .to_vec();
        for child in children {
            match self.source.document.node(child).map(|n| n.kind()) {
                Some(NodeKind::Text(_)) => {
                    let (text, spans) = super::dom_text(&self.source.document, child);
                    if text.is_empty() {
                        continue;
                    }
                    self.text_bytes += text.len();
                    if text.len() >= crate::text::MAX_TEXT_BYTES || self.text_bytes > 262_144 {
                        self.warn(id, "SVG text budget exceeds 64 KiB/run or 256 KiB/scene");
                        continue;
                    }
                    let glyphs = super::shape_dom(self.fonts, &paint.font, &text, &spans, child);
                    self.glyph_count += glyphs.glyphs.len();
                    if self.glyph_count > 65_536 {
                        self.warn(id, "SVG glyph budget exceeds 65,536");
                        continue;
                    }
                    let x = cursor.0
                        - match paint.anchor.as_str() {
                            "middle" => glyphs.width * 0.5,
                            "end" => glyphs.width,
                            _ => 0.0,
                        };
                    if paint.fill != "none" {
                        if let Some(mut color) = super::color(&paint.fill, paint.font.text_color) {
                            color.alpha *= paint.fill_opacity as f64;
                            self.push(
                                id,
                                scene,
                                Command::Text {
                                    source: child,
                                    glyphs: glyphs.clone(),
                                    x,
                                    baseline: cursor.1,
                                    color,
                                    transform,
                                },
                            );
                        } else {
                            self.warn(id, "SVG text paint servers are unsupported");
                        }
                    }
                    if paint.stroke != "none" {
                        self.warn(id, "SVG text stroke is unsupported");
                    }
                    cursor.0 += glyphs.width;
                }
                Some(NodeKind::Element(child_e)) if child_e.tag_name() == "tspan" => {
                    let child_paint = self.paint(child, paint);
                    self.text(child, &child_paint, transform, scene, cursor, depth + 1);
                }
                Some(NodeKind::Element(_)) => {
                    self.warn(child, "SVG textPath/foreign text is unsupported")
                }
                _ => {}
            }
        }
    }
    fn image(
        &mut self,
        id: NodeId,
        e: &ElementData,
        paint: &Paint,
        transform: Transform,
        scene: &mut Scene,
    ) {
        let image = self.source.images.get(&id).cloned().or_else(|| {
            let value = super::href(e)?;
            if value.starts_with("data:") {
                match data_image(value) {
                    Ok(image) => Some(Arc::new(image)),
                    Err(error) => {
                        self.warn(id, format!("SVG data image: {error}"));
                        None
                    }
                }
            } else {
                None
            }
        });
        let Some(image) = image else {
            self.warn(id, "SVG image is missing or external image is not loaded");
            return;
        };
        let (natural_width, natural_height) = image.natural_size();
        let mut rect = Rect {
            x: length(e.attribute("x"), self.viewport.0, paint.font.font_size).unwrap_or(0.0),
            y: length(e.attribute("y"), self.viewport.1, paint.font.font_size).unwrap_or(0.0),
            width: length(e.attribute("width"), self.viewport.0, paint.font.font_size)
                .unwrap_or(natural_width),
            height: length(e.attribute("height"), self.viewport.1, paint.font.font_size)
                .unwrap_or(natural_height),
        };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let clip = rect;
        let natural = Rect {
            x: 0.0,
            y: 0.0,
            width: natural_width,
            height: natural_height,
        };
        let fit = view_transform(
            natural,
            rect.width,
            rect.height,
            e.attribute("preserveAspectRatio"),
        );
        rect.x += fit.tx;
        rect.y += fit.ty;
        rect.width = natural.width * fit.sx;
        rect.height = natural.height * fit.sy;
        let mut inner = Scene::empty(scene.width, scene.height);
        inner.commands.push(Command::Image {
            source: id,
            image,
            rect,
            transform: Transform::identity(),
        });
        self.push(
            id,
            scene,
            Command::Group {
                scene: Arc::new(inner),
                transform,
                opacity: 1.0,
                clip: Some(clip),
            },
        );
    }
    fn geometry(
        &mut self,
        id: NodeId,
        e: &ElementData,
        em: f32,
    ) -> Option<(Path, Vec<Vec<Vertex>>)> {
        let x = |name| length(e.attribute(name), self.viewport.0, em).unwrap_or(0.0);
        let y = |name| length(e.attribute(name), self.viewport.1, em).unwrap_or(0.0);
        let mut pb = PathBuilder::new();
        let mut vertices = Vec::new();
        match e.tag_name() {
            "path" => {
                let data = e.attribute("d")?;
                if data.len() > 262_144 {
                    self.warn(id, "SVG path exceeds 256 KiB");
                    return None;
                }
                let mut sub = Vec::new();
                let mut previous = (0.0, 0.0);
                for segment in svgtypes::SimplifyingPathParser::from(data) {
                    self.segments += 1;
                    if self.segments > 65_536 {
                        self.warn(id, "SVG path segment budget exceeded");
                        return None;
                    }
                    let Ok(segment) = segment else {
                        self.warn(id, "invalid SVG path");
                        return None;
                    };
                    use svgtypes::SimplePathSegment as S;
                    match segment {
                        S::MoveTo { x, y } => {
                            if !sub.is_empty() {
                                vertices.push(std::mem::take(&mut sub));
                            }
                            previous = (x as f32, y as f32);
                            pb.move_to(previous.0, previous.1);
                            sub.push(Vertex::new(previous));
                        }
                        S::LineTo { x, y } => {
                            let point = (x as f32, y as f32);
                            pb.line_to(point.0, point.1);
                            vertex(&mut sub, previous, point, point, point);
                            previous = point;
                        }
                        S::CurveTo {
                            x1,
                            y1,
                            x2,
                            y2,
                            x,
                            y,
                        } => {
                            let point = (x as f32, y as f32);
                            pb.cubic_to(
                                x1 as f32, y1 as f32, x2 as f32, y2 as f32, point.0, point.1,
                            );
                            vertex(
                                &mut sub,
                                previous,
                                (x1 as f32, y1 as f32),
                                (x2 as f32, y2 as f32),
                                point,
                            );
                            previous = point;
                        }
                        S::Quadratic { x1, y1, x, y } => {
                            let point = (x as f32, y as f32);
                            pb.quad_to(x1 as f32, y1 as f32, point.0, point.1);
                            vertex(
                                &mut sub,
                                previous,
                                (x1 as f32, y1 as f32),
                                (x1 as f32, y1 as f32),
                                point,
                            );
                            previous = point;
                        }
                        S::ClosePath => {
                            pb.close();
                            if let Some(first) = sub.first() {
                                let point = first.point;
                                vertex(&mut sub, previous, point, previous, point);
                                previous = point;
                            }
                        }
                    }
                }
                if !sub.is_empty() {
                    vertices.push(sub);
                }
            }
            "rect" => {
                let (width, height) = (x("width"), y("height"));
                if width <= 0.0 || height <= 0.0 {
                    return None;
                }
                let (rx, ry) = (x("rx").min(width * 0.5), y("ry").min(height * 0.5));
                if rx > 0.0 || ry > 0.0 {
                    let rx = if rx > 0.0 { rx } else { ry.min(width * 0.5) };
                    let ry = if ry > 0.0 { ry } else { rx.min(height * 0.5) };
                    let (left, top) = (x("x"), y("y"));
                    let (right, bottom) = (left + width, top + height);
                    const K: f32 = 0.5522848;
                    pb.move_to(left + rx, top);
                    pb.line_to(right - rx, top);
                    pb.cubic_to(
                        right - rx + rx * K,
                        top,
                        right,
                        top + ry - ry * K,
                        right,
                        top + ry,
                    );
                    pb.line_to(right, bottom - ry);
                    pb.cubic_to(
                        right,
                        bottom - ry + ry * K,
                        right - rx + rx * K,
                        bottom,
                        right - rx,
                        bottom,
                    );
                    pb.line_to(left + rx, bottom);
                    pb.cubic_to(
                        left + rx - rx * K,
                        bottom,
                        left,
                        bottom - ry + ry * K,
                        left,
                        bottom - ry,
                    );
                    pb.line_to(left, top + ry);
                    pb.cubic_to(
                        left,
                        top + ry - ry * K,
                        left + rx - rx * K,
                        top,
                        left + rx,
                        top,
                    );
                    pb.close();
                } else {
                    pb.push_rect(tiny_skia::Rect::from_xywh(x("x"), y("y"), width, height)?);
                }
            }
            "circle" | "ellipse" => {
                let rx = if e.tag_name() == "circle" {
                    x("r")
                } else {
                    x("rx")
                };
                let ry = if e.tag_name() == "circle" {
                    rx
                } else {
                    y("ry")
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return None;
                }
                pb.push_oval(tiny_skia::Rect::from_xywh(
                    x("cx") - rx,
                    y("cy") - ry,
                    rx * 2.0,
                    ry * 2.0,
                )?);
            }
            "line" => {
                let (a, b) = ((x("x1"), y("y1")), (x("x2"), y("y2")));
                pb.move_to(a.0, a.1);
                pb.line_to(b.0, b.1);
                let mut sub = vec![Vertex::new(a)];
                vertex(&mut sub, a, b, a, b);
                vertices.push(sub);
            }
            "polyline" | "polygon" => {
                let Some(points) = e.attribute("points").and_then(numbers) else {
                    self.warn(id, "invalid SVG points");
                    return None;
                };
                if points.len() > 32_768 || points.len() % 2 != 0 {
                    self.warn(id, "invalid or oversized SVG points");
                    return None;
                }
                let mut sub = Vec::new();
                for point in points.as_chunks::<2>().0 {
                    let point = (point[0], point[1]);
                    if let Some(last) = sub.last().map(|v: &Vertex| v.point) {
                        pb.line_to(point.0, point.1);
                        vertex(&mut sub, last, point, last, point);
                    } else {
                        pb.move_to(point.0, point.1);
                        sub.push(Vertex::new(point));
                    }
                }
                if e.tag_name() == "polygon"
                    && let (Some(first), Some(last)) = (sub.first(), sub.last())
                {
                    let (first, last) = (first.point, last.point);
                    pb.close();
                    vertex(&mut sub, last, first, last, first);
                }
                vertices.push(sub);
            }
            _ => {}
        }
        let path = pb.finish()?;
        if path.points().iter().any(|p| {
            !p.x.is_finite()
                || !p.y.is_finite()
                || p.x.abs() > 1_000_000.0
                || p.y.abs() > 1_000_000.0
        }) {
            self.warn(id, "SVG geometry exceeds finite coordinate budget");
            return None;
        }
        Some((path, vertices))
    }
    fn brush(
        &mut self,
        id: NodeId,
        value: &str,
        bbox: Rect,
        current: ResolvedColor,
        opacity: f32,
        depth: usize,
    ) -> Option<Brush> {
        if value.trim() == "none" {
            return None;
        }
        if let Some(url) = value
            .trim()
            .strip_prefix("url(")
            .and_then(|s| s.split_once(')'))
        {
            let Some(target) = self.reference(id, url.0.trim().trim_matches(['\'', '"'])) else {
                return super::color(url.1.trim(), current).map(Brush::Solid);
            };
            if !self.enter(target) {
                return None;
            }
            let e = self.element(target).clone();
            let brush = match e.tag_name() {
                "linearGradient" | "radialGradient" => {
                    self.gradient(target, bbox, current, opacity)
                }
                "pattern" => self.pattern(target, bbox, current, opacity, depth),
                _ => {
                    self.warn(id, "SVG paint reference is not a gradient or pattern");
                    None
                }
            };
            self.references.pop();
            brush
        } else if let Some(mut color) = super::color(value, current) {
            color.alpha *= opacity as f64;
            Some(Brush::Solid(color))
        } else {
            self.warn(id, format!("unsupported SVG paint {value:?}"));
            None
        }
    }
    fn gradient(
        &mut self,
        id: NodeId,
        bbox: Rect,
        current: ResolvedColor,
        opacity: f32,
    ) -> Option<Brush> {
        let mut chain = vec![id];
        let mut next = id;
        while let Some(reference) = super::href(self.element(next)).map(str::to_owned) {
            let target = self.reference(next, &reference)?;
            if chain.contains(&target) || chain.len() >= super::MAX_DEPTH {
                self.warn(id, "cyclic SVG gradient href");
                return None;
            }
            if !matches!(
                self.element(target).tag_name(),
                "linearGradient" | "radialGradient"
            ) {
                self.warn(id, "gradient href must refer to a gradient");
                return None;
            }
            chain.push(target);
            next = target;
        }
        let attr = |name: &str| {
            chain
                .iter()
                .find_map(|id| self.element(*id).attribute(name))
                .map(str::to_owned)
        };
        let user = attr("gradientUnits").as_deref() == Some("userSpaceOnUse");
        let transform = attr("gradientTransform")
            .and_then(|v| transform_list(&v))
            .unwrap_or_default();
        let transform = if user {
            transform
        } else {
            if bbox.width <= 0.0 || bbox.height <= 0.0 {
                return None;
            }
            Transform::from_translate(bbox.x, bbox.y)
                .pre_scale(bbox.width, bbox.height)
                .pre_concat(transform)
        };
        let coord = |name: &str, fallback: f32, horizontal: bool| {
            let reference = if user {
                if horizontal {
                    self.viewport.0
                } else {
                    self.viewport.1
                }
            } else {
                1.0
            };
            length(attr(name).as_deref(), reference, 16.0).unwrap_or(fallback * reference)
        };
        let spread = match attr("spreadMethod").as_deref() {
            Some("repeat") => Spread::Repeat,
            Some("reflect") => Spread::Reflect,
            _ => Spread::Pad,
        };
        let geometry = if self.element(id).tag_name() == "linearGradient" {
            (
                coord("x1", 0.0, true),
                coord("y1", 0.0, false),
                coord("x2", 1.0, true),
                coord("y2", 0.0, false),
                0.0,
            )
        } else {
            let (cx, cy) = (coord("cx", 0.5, true), coord("cy", 0.5, false));
            let r = length(
                attr("r").as_deref(),
                if user {
                    (self.viewport.0.powi(2) + self.viewport.1.powi(2)).sqrt() / 2.0_f32.sqrt()
                } else {
                    1.0
                },
                16.0,
            )
            .unwrap_or(if user { self.viewport.0 * 0.5 } else { 0.5 });
            (
                cx,
                cy,
                attr("fx").map_or(cx, |_| coord("fx", 0.5, true)),
                attr("fy").map_or(cy, |_| coord("fy", 0.5, false)),
                r,
            )
        };
        if attr("fr").is_some_and(|s| s != "0") {
            self.warn(id, "SVG radial gradient fr is unsupported");
        }
        let stop_parent = chain.iter().copied().find(|id| {
            self.source
                .document
                .node(*id)
                .expect("gradient")
                .children()
                .iter()
                .any(|c| {
                    self.source
                        .document
                        .node(*c)
                        .and_then(|n| n.as_element())
                        .is_some_and(|e| e.tag_name() == "stop")
                })
        });
        let mut stops = Vec::new();
        let mut previous = 0.0_f32;
        if let Some(parent) = stop_parent {
            for &child in self.source.document.node(parent).expect("stops").children() {
                if let Some(e) = self
                    .source
                    .document
                    .node(child)
                    .and_then(|n| n.as_element())
                    .filter(|e| e.tag_name() == "stop")
                {
                    if stops.len() >= 256 {
                        self.warn(id, "gradient stop budget exceeded");
                        break;
                    }
                    let offset = length(e.attribute("offset"), 1.0, 16.0)
                        .unwrap_or(0.0)
                        .clamp(previous, 1.0);
                    let mut c = super::color(e.attribute("stop-color").unwrap_or("black"), current)
                        .unwrap_or(ResolvedColor::BLACK);
                    c.alpha *=
                        (number(e.attribute("stop-opacity"), 1.0).clamp(0.0, 1.0) * opacity) as f64;
                    stops.push((offset, c));
                    previous = offset;
                }
            }
        }
        if stops.is_empty() {
            return None;
        }
        if stops.len() == 1 {
            return Some(Brush::Solid(stops[0].1));
        }
        let (a, b, c, d, r) = geometry;
        Some(if self.element(id).tag_name() == "linearGradient" {
            Brush::Linear {
                start: (a, b),
                end: (c, d),
                stops,
                spread,
                transform,
            }
        } else {
            Brush::Radial {
                center: (a, b),
                focus: (c, d),
                radius: r,
                stops,
                spread,
                transform,
            }
        })
    }
    fn pattern(
        &mut self,
        id: NodeId,
        bbox: Rect,
        current: ResolvedColor,
        opacity: f32,
        depth: usize,
    ) -> Option<Brush> {
        let e = self.element(id).clone();
        if super::href(&e).is_some() {
            self.warn(id, "SVG pattern href inheritance is unsupported");
        }
        let user = e.attribute("patternUnits") == Some("userSpaceOnUse");
        let coordinate = |name: &str, horizontal: bool| {
            length(
                e.attribute(name),
                if user {
                    if horizontal {
                        self.viewport.0
                    } else {
                        self.viewport.1
                    }
                } else {
                    1.0
                },
                16.0,
            )
            .unwrap_or(0.0)
        };
        let tile = if user {
            Rect {
                x: coordinate("x", true),
                y: coordinate("y", false),
                width: coordinate("width", true),
                height: coordinate("height", false),
            }
        } else {
            Rect {
                x: bbox.x + coordinate("x", true) * bbox.width,
                y: bbox.y + coordinate("y", false) * bbox.height,
                width: coordinate("width", true) * bbox.width,
                height: coordinate("height", false) * bbox.height,
            }
        };
        if tile.width <= 0.0 || tile.height <= 0.0 {
            return None;
        }
        let mut scene = Scene::empty(tile.width, tile.height);
        let content = if let Some(vb) = view_box(&e) {
            view_transform(
                vb,
                tile.width,
                tile.height,
                e.attribute("preserveAspectRatio"),
            )
        } else if e.attribute("patternContentUnits") == Some("objectBoundingBox") {
            Transform::from_translate(-tile.x + bbox.x, -tile.y + bbox.y)
                .pre_scale(bbox.width, bbox.height)
        } else {
            Transform::from_translate(-tile.x, -tile.y)
        };
        let mut font = self.source.styles.get(id).cloned().unwrap_or_default();
        font.text_color = current;
        let paint = self.paint(id, &Paint::new(font));
        self.children(id, &paint, content, &mut scene, depth + 1);
        Some(Brush::Pattern {
            scene: Arc::new(scene),
            tile,
            transform: e
                .attribute("patternTransform")
                .and_then(transform_list)
                .unwrap_or_default(),
            opacity,
        })
    }
    fn markers(
        &mut self,
        id: NodeId,
        paint: &Paint,
        paths: &[Vec<Vertex>],
        transform: Transform,
        scene: &mut Scene,
        depth: usize,
    ) {
        for path in paths {
            for (index, v) in path.iter().enumerate() {
                let which = if index == 0 {
                    0
                } else if index + 1 == path.len() {
                    2
                } else {
                    1
                };
                let value = &paint.markers[which];
                if value.is_empty() || value == "none" {
                    continue;
                }
                let Some(reference) = value.strip_prefix("url(").and_then(|s| s.strip_suffix(')'))
                else {
                    self.warn(id, "invalid SVG marker URL");
                    continue;
                };
                let Some(target) = self.reference(id, reference.trim()) else {
                    continue;
                };
                if !self.enter(target) {
                    continue;
                }
                let e = self.element(target).clone();
                if e.tag_name() != "marker" {
                    self.warn(id, "marker URL does not refer to a marker");
                    self.references.pop();
                    continue;
                }
                let (width, height) = (
                    number(e.attribute("markerWidth"), 3.0),
                    number(e.attribute("markerHeight"), 3.0),
                );
                if width <= 0.0 || height <= 0.0 {
                    self.references.pop();
                    continue;
                }
                let unit = if e.attribute("markerUnits") == Some("userSpaceOnUse") {
                    1.0
                } else {
                    paint.width
                };
                let angle = match e.attribute("orient").unwrap_or("0") {
                    "auto" => v.angle(which),
                    "auto-start-reverse" => v.angle(which) + if which == 0 { 180.0 } else { 0.0 },
                    value => number(Some(value.trim_end_matches("deg")), 0.0),
                };
                let vb = view_box(&e);
                let map = vb.map_or(Transform::identity(), |v| {
                    view_transform(v, width, height, e.attribute("preserveAspectRatio"))
                });
                let mut reference = tiny_skia::Point::from_xy(
                    number(e.attribute("refX"), 0.0),
                    number(e.attribute("refY"), 0.0),
                );
                map.map_point(&mut reference);
                let marker_transform = transform
                    .pre_translate(v.point.0, v.point.1)
                    .pre_rotate(angle)
                    .pre_scale(unit, unit)
                    .pre_translate(-reference.x, -reference.y);
                let mut marker = Scene::empty(width, height);
                let marker_paint = self.paint(target, &Paint::new(paint.font.clone()));
                self.children(target, &marker_paint, map, &mut marker, depth + 1);
                self.push(
                    id,
                    scene,
                    Command::Group {
                        scene: Arc::new(marker),
                        transform: marker_transform,
                        opacity: 1.0,
                        clip: Some(Rect {
                            x: 0.0,
                            y: 0.0,
                            width,
                            height,
                        }),
                    },
                );
                self.references.pop();
            }
        }
    }
}

#[derive(Clone)]
struct Vertex {
    point: (f32, f32),
    incoming: (f32, f32),
    outgoing: (f32, f32),
}
impl Vertex {
    fn new(point: (f32, f32)) -> Self {
        Self {
            point,
            incoming: (0.0, 0.0),
            outgoing: (0.0, 0.0),
        }
    }
    fn angle(&self, which: usize) -> f32 {
        let a = self.incoming.1.atan2(self.incoming.0);
        let b = self.outgoing.1.atan2(self.outgoing.0);
        match which {
            0 => b.to_degrees(),
            2 => a.to_degrees(),
            _ => {
                let delta = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                (a + delta * 0.5).to_degrees()
            }
        }
    }
}
fn vertex(
    vertices: &mut Vec<Vertex>,
    previous: (f32, f32),
    control1: (f32, f32),
    control2: (f32, f32),
    point: (f32, f32),
) {
    let outgoing = if control1 == previous {
        (point.0 - previous.0, point.1 - previous.1)
    } else {
        (control1.0 - previous.0, control1.1 - previous.1)
    };
    let incoming = if control2 == point {
        (point.0 - previous.0, point.1 - previous.1)
    } else {
        (point.0 - control2.0, point.1 - control2.1)
    };
    if let Some(last) = vertices.last_mut() {
        last.outgoing = outgoing;
    }
    vertices.push(Vertex {
        point,
        incoming,
        outgoing: (0.0, 0.0),
    });
}

pub(crate) fn length(value: Option<&str>, reference: f32, em: f32) -> Option<f32> {
    let v = value?.trim().parse::<svgtypes::Length>().ok()?;
    use svgtypes::LengthUnit as U;
    let value = v.number as f32
        * match v.unit {
            U::None | U::Px => 1.0,
            U::Percent => reference / 100.0,
            U::Em => em,
            U::Ex => em * 0.5,
            U::In => 96.0,
            U::Cm => 96.0 / 2.54,
            U::Mm => 96.0 / 25.4,
            U::Pt => 96.0 / 72.0,
            U::Pc => 16.0,
        };
    (value.is_finite() && value.abs() <= 1_000_000.0).then_some(value)
}
fn number(value: Option<&str>, fallback: f32) -> f32 {
    value
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && v.abs() <= 1_000_000.0)
        .unwrap_or(fallback)
}
fn numbers(value: &str) -> Option<Vec<f32>> {
    if value.len() > 262_144 {
        return None;
    }
    let values = svgtypes::NumberListParser::from(value)
        .map(|n| n.ok().map(|n| n as f32))
        .collect::<Option<Vec<_>>>()?;
    values
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
        .then_some(values)
}
fn view_box(e: &ElementData) -> Option<Rect> {
    let v = numbers(e.attribute("viewBox")?)?;
    (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| Rect {
        x: v[0],
        y: v[1],
        width: v[2],
        height: v[3],
    })
}
fn view_transform(v: Rect, width: f32, height: f32, aspect: Option<&str>) -> Transform {
    let value = aspect.unwrap_or("xMidYMid meet");
    let (sx, sy) = (width / v.width, height / v.height);
    if value.trim() == "none" {
        return Transform::from_scale(sx, sy).pre_translate(-v.x, -v.y);
    }
    let scale = if value.contains("slice") {
        sx.max(sy)
    } else {
        sx.min(sy)
    };
    let ax = if value.contains("xMin") {
        0.0
    } else if value.contains("xMax") {
        1.0
    } else {
        0.5
    };
    let ay = if value.contains("YMin") {
        0.0
    } else if value.contains("YMax") {
        1.0
    } else {
        0.5
    };
    Transform::from_translate(
        (width - v.width * scale) * ax,
        (height - v.height * scale) * ay,
    )
    .pre_scale(scale, scale)
    .pre_translate(-v.x, -v.y)
}
fn transform_list(value: &str) -> Option<Transform> {
    if value.len() > 4096 {
        return None;
    }
    let mut result = Transform::identity();
    for token in svgtypes::TransformListParser::from(value) {
        use svgtypes::TransformListToken as T;
        let t = match token.ok()? {
            T::Matrix { a, b, c, d, e, f } => {
                Transform::from_row(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32)
            }
            T::Translate { tx, ty } => Transform::from_translate(tx as f32, ty as f32),
            T::Scale { sx, sy } => Transform::from_scale(sx as f32, sy as f32),
            T::Rotate { angle } => Transform::from_rotate(angle as f32),
            T::SkewX { angle } => {
                Transform::from_row(1.0, 0.0, (angle as f32).to_radians().tan(), 1.0, 0.0, 0.0)
            }
            T::SkewY { angle } => {
                Transform::from_row(1.0, (angle as f32).to_radians().tan(), 0.0, 1.0, 0.0, 0.0)
            }
        };
        result = result.pre_concat(t);
    }
    result.is_finite().then_some(result)
}

fn data_image(value: &str) -> Result<crate::media::DecodedImage, Box<dyn std::error::Error>> {
    use base64::Engine;
    if value.len() > 6 * 1024 * 1024 {
        return Err("data image exceeds 6 MiB".into());
    }
    let (header, payload) = value.split_once(',').ok_or("invalid data image")?;
    let bytes = if header.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD.decode(payload)?
    } else {
        percent_encoding::percent_decode_str(payload).collect::<Vec<_>>()
    };
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("data image exceeds 4 MiB".into());
    }
    // Recursive SVG data images could bypass graph/decode limits.
    if header.starts_with("data:image/svg") {
        return Err("nested SVG data images are unsupported".into());
    }
    image::guess_format(&bytes)?;
    crate::media::DecodedImage::decode(&bytes)
}
