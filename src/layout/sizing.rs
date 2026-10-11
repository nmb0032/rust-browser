//! Physical min/max prerequisites shared by block, positioned, flex and grid.
use crate::{css::values::Length, style::Diagnostic};

use super::{LayoutBox, tree::resolve_length};

pub(super) fn definite_size(
    b: &LayoutBox,
    length: Option<&Length>,
    reference: Option<f32>,
    decorations: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    let length = length?;
    if length.has_percentage() && reference.is_none() {
        return None;
    }
    Some(b.style.box_sizing.content_size(
        resolve_length(
            length,
            b.style.font_size,
            reference.unwrap_or(0.0),
            b.node_id,
            "size",
            diagnostics,
        ),
        decorations,
    ))
}

pub(super) fn bounds(
    b: &LayoutBox,
    horizontal: bool,
    reference: Option<f32>,
    decorations: f32,
    automatic_minimum: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let (min, max) = if horizontal {
        (&b.style.min_width, &b.style.max_width)
    } else {
        (&b.style.min_height, &b.style.max_height)
    };
    let min = definite_size(b, min.as_ref(), reference, decorations, diagnostics)
        .unwrap_or(if min.is_none() {
            automatic_minimum
        } else {
            0.0
        })
        .max(0.0);
    let max = definite_size(b, max.as_ref(), reference, decorations, diagnostics)
        .unwrap_or(f32::INFINITY)
        .max(min);
    (min, max)
}
