//! Shared content-box sizing and bounded production intrinsic measurements.
use crate::{css::values::Length, style::Diagnostic};

use super::{LayoutBox, tree::resolve_length};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct IntrinsicSizes {
    width: (f32, f32),
    height: (f32, f32),
}
impl IntrinsicSizes {
    pub(super) fn swapped(self) -> Self {
        Self {
            width: self.height,
            height: self.width,
        }
    }
}

pub(super) fn prepare_intrinsic(
    b: &mut LayoutBox,
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    b.intrinsic_sizes = None;
    let child_reference = b
        .style
        .width
        .as_ref()
        .filter(|l| !l.intrinsic() && !l.has_percentage())
        .map_or(reference, |l| l.resolve(b.style.font_size, reference));
    for child in &mut b.children {
        prepare_intrinsic(child, child_reference, font, diagnostics);
    }
    if ![
        &b.style.width,
        &b.style.height,
        &b.style.min_width,
        &b.style.min_height,
        &b.style.max_width,
        &b.style.max_height,
    ]
    .iter()
    .any(|l| l.as_ref().is_some_and(Length::intrinsic))
        && !matches!(&b.style.flex_basis, crate::style::FlexBasis::Length(l) if l.intrinsic())
    {
        return;
    }
    measure_intrinsic(b, reference, font, diagnostics);
}

fn measure_intrinsic(
    b: &mut LayoutBox,
    reference: f32,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let widths = super::positioning::intrinsic_content(b, reference, font, diagnostics);
    b.intrinsic_sizes = Some(IntrinsicSizes {
        width: widths,
        height: (0.0, 0.0),
    });
    let mut probe = b.clone();
    probe.reset();
    for l in [
        &mut probe.style.width,
        &mut probe.style.height,
        &mut probe.style.min_width,
        &mut probe.style.min_height,
        &mut probe.style.max_width,
        &mut probe.style.max_height,
    ] {
        if l.as_ref().is_some_and(Length::intrinsic) {
            *l = None;
        }
    }
    probe.style.aspect_ratio = crate::style::AspectRatio::default();
    let horizontal = b.style.padding.left.resolve(b.style.font_size, reference)
        + b.style.padding.right.resolve(b.style.font_size, reference)
        + b.style.border_width.left
        + b.style.border_width.right;
    let width = definite_size(
        b,
        b.style.width.as_ref(),
        Some(reference),
        horizontal,
        diagnostics,
    )
    .unwrap_or((reference - horizontal).max(0.0));
    let limits = bounds(b, true, Some(reference), horizontal, 0.0, diagnostics);
    let width = width.clamp(limits.0, limits.1);
    super::block::layout_sized(
        &mut probe,
        reference,
        width,
        None,
        (false, false),
        font,
        diagnostics,
    );
    let height = probe.content_rect.height;
    let mut sizes = IntrinsicSizes {
        width: widths,
        height: (height, height),
    };
    if b.style.writing_mode.vertical() {
        let inline =
            super::inline::intrinsic_inline(&b.children, &b.style, reference, font, diagnostics);
        sizes.height = inline;
        sizes.width = (probe.content_rect.width, probe.content_rect.width);
    }
    b.intrinsic_sizes = Some(sizes);
}

fn resolve_size(
    b: &LayoutBox,
    horizontal: bool,
    length: Option<&Length>,
    reference: Option<f32>,
    decorations: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    let length = length?;
    if length.intrinsic() {
        let sizes = b.intrinsic_sizes?;
        let (min, max) = if horizontal {
            sizes.width
        } else {
            sizes.height
        };
        return Some(match length {
            Length::MinContent => min,
            Length::MaxContent => max,
            Length::FitContent(limit) => {
                if reference.is_none() && limit.as_ref().is_some_and(|l| l.has_percentage()) {
                    diagnose_once(b,"cyclic fit-content percentage limit contributes zero before min-content clamping",diagnostics);
                }
                let margins = super::tree::resolve_margins(&b.style,reference.unwrap_or(0.0),b.node_id,diagnostics);
                let margin = if horizontal { margins.left.unwrap_or(0.0) + margins.right.unwrap_or(0.0) }
                    else { margins.top.unwrap_or(0.0) + margins.bottom.unwrap_or(0.0) };
                let available = limit.as_ref().map_or(
                    reference.unwrap_or(max + decorations + margin) - decorations - margin,
                    |l| resolve_length(l, b.style.font_size, reference.unwrap_or(0.0), b.node_id, "fit-content limit", diagnostics) - decorations - margin,
                );
                min.max(available).min(max)
            }
            _ => unreachable!(),
        }.max(0.0));
    }
    if length.has_percentage() && reference.is_none() {
        return None;
    }
    Some(b.style.box_sizing.content_size(
        resolve_length(
            length,
            b.style.font_size,
            reference.unwrap_or(0.0),
            b.node_id,
            if horizontal { "width" } else { "height" },
            diagnostics,
        ),
        decorations,
    ))
}

pub(super) fn definite_size(
    b: &LayoutBox,
    length: Option<&Length>,
    reference: Option<f32>,
    decorations: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    resolve_size(b, true, length, reference, decorations, diagnostics)
}

pub(super) fn definite_height(
    b: &LayoutBox,
    length: Option<&Length>,
    reference: Option<f32>,
    decorations: f32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f32> {
    resolve_size(b, false, length, reference, decorations, diagnostics)
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
    if reference.is_none()
        && [min, max]
            .iter()
            .any(|l| l.as_ref().is_some_and(Length::has_percentage))
    {
        diagnose_once(
            b,
            if horizontal {
                "percentage min/max width has no definite containing size; using intrinsic automatic constraints"
            } else {
                "percentage min/max height has no definite containing size; using intrinsic automatic constraints"
            },
            diagnostics,
        );
    }
    let max = resolve_size(
        b,
        horizontal,
        max.as_ref(),
        reference,
        decorations,
        diagnostics,
    )
    .unwrap_or(f32::INFINITY);
    let min = resolve_size(
        b,
        horizontal,
        min.as_ref(),
        reference,
        decorations,
        diagnostics,
    )
    .unwrap_or(if min.is_none() {
        automatic_minimum.min(max)
    } else {
        0.0
    })
    .max(0.0);
    (min, max.max(min))
}

fn diagnose_once(b: &LayoutBox, message: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !diagnostics
        .iter()
        .any(|d| d.node_id == b.node_id && d.message == message)
    {
        diagnostics.push(Diagnostic {
            node_id: b.node_id,
            message: message.into(),
        });
    }
}

pub(super) fn ratio_height(
    b: &LayoutBox,
    width: f32,
    horizontal: f32,
    vertical: f32,
) -> Option<f32> {
    let ratio = b
        .image
        .as_ref()
        .and_then(|i| i.preferred_ratio(&b.style))
        .or(b.style.aspect_ratio.ratio)?;
    Some(
        if b.style.box_sizing == crate::style::BoxSizing::BorderBox && !b.style.aspect_ratio.auto {
            ((width + horizontal) / ratio - vertical).max(0.0)
        } else {
            width / ratio
        },
    )
}

pub(super) fn ratio_width(
    b: &LayoutBox,
    height: f32,
    horizontal: f32,
    vertical: f32,
) -> Option<f32> {
    let ratio = b
        .image
        .as_ref()
        .and_then(|i| i.preferred_ratio(&b.style))
        .or(b.style.aspect_ratio.ratio)?;
    Some(
        if b.style.box_sizing == crate::style::BoxSizing::BorderBox && !b.style.aspect_ratio.auto {
            ((height + vertical) * ratio - horizontal).max(0.0)
        } else {
            height * ratio
        },
    )
}

pub(super) fn constrain_replaced(
    b: &LayoutBox,
    mut size: (f32, f32),
    specified: (bool, bool),
    references: (Option<f32>, Option<f32>),
    decorations: (f32, f32),
    diagnostics: &mut Vec<Diagnostic>,
) -> (f32, f32) {
    let wb = bounds(b, true, references.0, decorations.0, 0.0, diagnostics);
    let hb = bounds(b, false, references.1, decorations.1, 0.0, diagnostics);
    if b.image
        .as_ref()
        .is_some_and(|image| !image.preserves_ratio(&b.style))
    {
        return (size.0.clamp(wb.0, wb.1), size.1.clamp(hb.0, hb.1));
    }
    if !specified.0 && !specified.1 {
        let shrink = (wb.1 / size.0.max(f32::MIN_POSITIVE))
            .min(hb.1 / size.1.max(f32::MIN_POSITIVE))
            .min(1.0);
        size.0 *= shrink;
        size.1 *= shrink;
        let grow = (wb.0 / size.0.max(1.0))
            .max(hb.0 / size.1.max(1.0))
            .max(1.0);
        size.0 *= grow;
        size.1 *= grow;
    } else if !specified.1 {
        let width = size.0.clamp(wb.0, wb.1);
        size.1 *= width / size.0.max(f32::MIN_POSITIVE);
        size.0 = width;
    } else if !specified.0 {
        let height = size.1.clamp(hb.0, hb.1);
        size.0 *= height / size.1.max(f32::MIN_POSITIVE);
        size.1 = height;
    }
    (size.0.clamp(wb.0, wb.1), size.1.clamp(hb.0, hb.1))
}
