use std::sync::Arc;

use crate::{
    dom::{ElementData, NodeId},
    media::DecodedImage,
    style::{ComputedStyle, Diagnostic},
};

use super::{
    Rect, TextFragment,
    inline::glyph_bounds,
    tree::{finite_rect, finite_used},
};

const MAX_LAYOUT_DIMENSION: f32 = 1_000_000.0;

#[derive(Clone, Debug)]
pub(super) struct ImageContent {
    pub(super) data: Option<Arc<DecodedImage>>,
    pub(super) scene: Option<Arc<crate::vector::Scene>>,
    source: Option<crate::vector::Source>,
    alt: String,
    natural: Option<(f32, f32)>,
    ratio: Option<f32>,
}

impl PartialEq for ImageContent {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
            && self.scene == other.scene
            && self.alt == other.alt
            && self.natural == other.natural
            && self.ratio == other.ratio
    }
}

impl ImageContent {
    pub(super) fn new(element: &ElementData, data: Option<Arc<DecodedImage>>) -> Self {
        let dimension = |name, fallback| {
            element
                .attribute(name)
                .and_then(crate::html::attributes::image_dimension)
                .unwrap_or(fallback)
        };
        let natural = if let Some(image) = &data {
            Some(image.natural_size())
        } else {
            match element.tag_name() {
                "canvas" => Some((dimension("width", 300.0), dimension("height", 150.0))),
                "video" | "iframe" => Some((300.0, 150.0)),
                _ => None,
            }
        };
        let ratio = natural
            .filter(|_| data.is_some() || element.tag_name() == "canvas")
            .filter(|&(w, h)| w > 0.0 && h > 0.0)
            .map(|(w, h)| w / h);
        Self {
            scene: data.as_ref().and_then(|image| image.vector().cloned()),
            source: None,
            data,
            alt: if element.tag_name() == "img" {
                element.attribute("alt").unwrap_or_default().to_owned()
            } else {
                String::new()
            },
            natural,
            ratio,
        }
    }

    pub(super) fn foreign(source: crate::vector::Source) -> Self {
        Self {
            data: None,
            scene: None,
            source: Some(source),
            alt: String::new(),
            natural: None,
            ratio: None,
        }
    }

    pub(super) fn prepare(
        &mut self,
        fonts: &crate::text::FontContext,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if let Some(source) = &self.source {
            let scene = source.compile(fonts, diagnostics);
            self.natural = Some((scene.width, scene.height));
            self.ratio =
                (scene.width > 0.0 && scene.height > 0.0).then_some(scene.width / scene.height);
            self.scene = Some(scene);
        }
    }

    pub(super) fn natural(&self) -> Option<(f32, f32)> {
        self.natural
    }

    pub(super) fn viewport(
        &mut self,
        fonts: &crate::text::FontContext,
        diagnostics: &mut Vec<Diagnostic>,
        width: f32,
        height: f32,
    ) {
        if let Some(source) = &self.source
            && let Some(scene) = source.svg_viewport(fonts, diagnostics, width, height)
        {
            self.scene = Some(scene);
        }
    }

    pub(super) fn baseline(&self, height: f32) -> Option<f32> {
        let scene = self.scene.as_ref()?;
        scene.baseline.map(|b| {
            if scene.height > 0.0 {
                b * height / scene.height
            } else {
                0.0
            }
        })
    }

    pub(super) fn size(
        &self,
        style: &ComputedStyle,
        width: Option<f32>,
        height: Option<f32>,
        line_height: f32,
        font: &crate::text::FontContext,
    ) -> (f32, f32) {
        let natural_width = self.natural.map_or_else(
            || font.shape(style, &self.alt).width.max(16.0),
            |size| size.0,
        );
        let natural_height = self.natural.map_or(line_height, |size| size.1);
        let ratio = self.preferred_ratio(style).or_else(|| {
            (self.natural.is_none() && natural_width > 0.0 && natural_height > 0.0)
                .then_some(natural_width / natural_height)
        });
        let (width, height) = match (width, height) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, ratio.map_or(natural_height, |r| width / r)),
            (None, Some(height)) => (ratio.map_or(natural_width, |r| height * r), height),
            (None, None) => (
                natural_width,
                ratio.map_or(natural_height, |r| natural_width / r),
            ),
        };
        (
            width.clamp(0.0, MAX_LAYOUT_DIMENSION),
            height.clamp(0.0, MAX_LAYOUT_DIMENSION),
        )
    }

    pub(super) fn preferred_ratio(&self, style: &ComputedStyle) -> Option<f32> {
        if style.aspect_ratio.auto {
            self.ratio.or(style.aspect_ratio.ratio)
        } else {
            style.aspect_ratio.ratio
        }
    }

    pub(super) fn preserves_ratio(&self, style: &ComputedStyle) -> bool {
        self.preferred_ratio(style).is_some() || self.natural.is_none()
    }

    pub(super) fn alt_fragment(
        &self,
        rect: Rect,
        style: &ComputedStyle,
        font: &crate::text::FontContext,
        node: Option<NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<TextFragment> {
        if self.data.is_some()
            || self.scene.is_some()
            || self.alt.is_empty()
            || rect.width <= 0.0
            || rect.height <= 0.0
        {
            return None;
        }
        let face = font.select(style);
        let metrics = face
            .horizontal_line_metrics(style.font_size)
            .expect("bundled horizontal face");
        // The fallback is text inside a replaced box, not its intrinsic baseline.
        // Anchor its font descent to the content bottom, including explicit sizes.
        let baseline = finite_used(
            rect.y as f64 + rect.height as f64 + metrics.descent as f64,
            node,
            "alt text baseline",
            diagnostics,
        );
        Some(TextFragment {
            line_index: 0,
            x: rect.x,
            y: finite_used(
                baseline as f64 - metrics.ascent as f64,
                node,
                "alt text y",
                diagnostics,
            ),
            baseline,
            ink_rect: finite_rect(
                glyph_bounds(font, &self.alt, style, rect.x, baseline),
                node,
                diagnostics,
            ),
            width: rect.width,
            height: rect.height,
            text: self.alt.clone(),
            glyphs: font.shape(style, &self.alt),
            font_size: style.font_size,
            color: style.text_fill_color.legacy(),
            text_color: style.text_fill_color,
            bold: style.bold,
            italic: style.italic,
        })
    }
}
