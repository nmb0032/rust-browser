use std::sync::Arc;

use crate::{
    dom::ElementData, html::attributes::image_dimension, media::DecodedImage, style::ComputedStyle,
};

use super::{Rect, TextFragment, inline::measure_text};

const MAX_LAYOUT_DIMENSION: f32 = 1_000_000.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ImageContent {
    pub(super) data: Option<Arc<DecodedImage>>,
    height: Option<f32>,
    alt: String,
}

impl ImageContent {
    pub(super) fn new(element: &ElementData, data: Option<Arc<DecodedImage>>) -> Self {
        Self {
            data,
            height: element.attribute("height").and_then(image_dimension),
            alt: element.attribute("alt").unwrap_or_default().to_owned(),
        }
    }

    pub(super) fn size(
        &self,
        style: &ComputedStyle,
        containing_width: f32,
        font: &fontdue::Font,
    ) -> (f32, f32) {
        let natural_width = self.data.as_ref().map_or_else(
            || measure_text(font, &self.alt, style.font_size).max(16.0),
            |image| image.width() as f32,
        );
        let natural_height = self
            .data
            .as_ref()
            .map_or(style.font_size * 1.25, |image| image.height() as f32);
        let width = style
            .width
            .map(|length| length.resolve(style.font_size, containing_width));
        let ratio = natural_width / natural_height.max(1.0);
        let (width, height) = match (width, self.height) {
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

    pub(super) fn alt_fragment(&self, rect: Rect, style: &ComputedStyle) -> Option<TextFragment> {
        if self.data.is_some() || self.alt.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        Some(TextFragment {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            text: self.alt.clone(),
            font_size: style.font_size,
            color: style.color,
            bold: style.bold,
            italic: style.italic,
        })
    }
}
