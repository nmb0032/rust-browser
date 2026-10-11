use std::sync::Arc;

use crate::{
    dom::{ElementData, NodeId},
    media::DecodedImage,
    style::{ComputedStyle, Diagnostic},
};

use super::{
    Rect, TextFragment,
    inline::{glyph_bounds, measure_text},
    tree::{finite_rect, finite_used},
};

const MAX_LAYOUT_DIMENSION: f32 = 1_000_000.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ImageContent {
    pub(super) data: Option<Arc<DecodedImage>>,
    alt: String,
}

impl ImageContent {
    pub(super) fn new(element: &ElementData, data: Option<Arc<DecodedImage>>) -> Self {
        Self {
            data,
            alt: element.attribute("alt").unwrap_or_default().to_owned(),
        }
    }

    pub(super) fn size(
        &self,
        style: &ComputedStyle,
        width: Option<f32>,
        height: Option<f32>,
        line_height: f32,
        font: &fontdue::Font,
    ) -> (f32, f32) {
        let natural_width = self.data.as_ref().map_or_else(
            || measure_text(font, &self.alt, style.font_size).max(16.0),
            |image| image.width() as f32,
        );
        let natural_height = self
            .data
            .as_ref()
            .map_or(line_height, |image| image.height() as f32);
        let ratio = natural_width / natural_height.max(1.0);
        let (width, height) = match (width, height) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, width / ratio),
            (None, Some(height)) => (height * ratio, height),
            (None, None) => (natural_width, natural_height),
        };
        (
            width.clamp(0.0, MAX_LAYOUT_DIMENSION),
            height.clamp(0.0, MAX_LAYOUT_DIMENSION),
        )
    }

    pub(super) fn alt_fragment(
        &self,
        rect: Rect,
        style: &ComputedStyle,
        font: &fontdue::Font,
        node: Option<NodeId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<TextFragment> {
        if self.data.is_some() || self.alt.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let metrics = font
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
            font_size: style.font_size,
            color: style.text_fill_color.legacy(),
            text_color: style.text_fill_color,
            bold: style.bold,
            italic: style.italic,
        })
    }
}
