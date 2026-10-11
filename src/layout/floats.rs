//! One exclusion set per block formatting context. Y is relative to the BFC,
//! while used block descendants keep their existing local border-origin Y.
use super::tree::finite_used;
use super::{
    LayoutBox, Rect,
    tree::{resolve_edges, resolve_margins},
};
use crate::style::{Clear, Diagnostic, Float};

const MAX_FLOATS: usize = 4096;
fn add(a: f32, b: f32) -> f32 {
    ((a as f64 + b as f64).clamp(-(f32::MAX as f64), f32::MAX as f64)) as f32
}

#[derive(Clone, Default)]
pub(super) struct FloatContext {
    pub(super) offset: f32,
    entries: Vec<(Float, Rect)>,
    overflow_bottom: f32,
}

impl FloatContext {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub(super) fn opportunity(&self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
        let (mut left, mut right) = (x, add(x, width));
        if self.overflow_bottom > 0.0 && self.overflow_bottom > y {
            return (x + width, 0.0);
        }
        for (side, rect) in &self.entries {
            if rect.y < add(y, height.max(0.001)) && add(rect.y, rect.height) > y {
                match side {
                    Float::Left | Float::InlineStart => left = left.max(add(rect.x, rect.width)),
                    Float::Right | Float::InlineEnd => right = right.min(rect.x),
                    Float::None => {}
                }
            }
        }
        (left.min(add(x, width)), (right - left).max(0.0))
    }

    pub(super) fn next_bottom(&self, y: f32) -> Option<f32> {
        self.entries
            .iter()
            .map(|(_, r)| add(r.y, r.height))
            .chain((self.overflow_bottom > 0.0).then_some(self.overflow_bottom))
            .filter(|bottom| *bottom > y)
            .min_by(f32::total_cmp)
    }

    pub(super) fn clear_y(&self, clear: Clear, y: f32) -> f32 {
        let y = if clear != Clear::None && self.overflow_bottom > 0.0 {
            y.max(self.overflow_bottom)
        } else {
            y
        };
        self.entries
            .iter()
            .filter(|(side, _)| {
                clear == Clear::Both
                    || clear == Clear::Left && *side == Float::Left
                    || clear == Clear::Right && *side == Float::Right
            })
            .map(|(_, r)| add(r.y, r.height))
            .fold(y, f32::max)
    }

    pub(super) fn bottom(&self) -> f32 {
        self.entries
            .iter()
            .map(|(_, r)| add(r.y, r.height))
            .fold(self.overflow_bottom, f32::max)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn place(
        &mut self,
        b: &mut LayoutBox,
        x: f32,
        local_y: f32,
        width: f32,
        preceding: f32,
        line_height: f32,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let margin_width = finite_used(
            b.rect.width as f64 + b.margin.left as f64 + b.margin.right as f64,
            b.node_id,
            "float margin width",
            diagnostics,
        )
        .max(0.0);
        let margin_height = finite_used(
            b.rect.height as f64 + b.margin.top as f64 + b.margin.bottom as f64,
            b.node_id,
            "float margin height",
            diagnostics,
        )
        .max(0.0);
        let mut y = self.clear_y(b.style.clear, add(local_y, self.offset));
        if self.entries.len() >= MAX_FLOATS {
            y = y.max(self.bottom());
        }
        // A later float may not have a top above an earlier float.
        if let Some((_, last)) = self.entries.last() {
            y = y.max(last.y);
        }
        let (left, available) = loop {
            let band = self.opportunity(x, y, width, margin_height);
            if margin_width + preceding <= band.1 || self.next_bottom(y).is_none() {
                break band;
            }
            y = self.next_bottom(y).unwrap();
        };
        if preceding > 0.0 && margin_width + preceding > available {
            y = y.max(add(add(local_y, self.offset), line_height));
        }
        let (left, available) = if y > local_y + self.offset {
            self.opportunity(x, y, width, margin_height)
        } else {
            (left, available)
        };
        let margin_x = if b.style.float == Float::Right {
            add(left, available) - margin_width
        } else {
            left
        };
        super::positioning::translate(b, add(margin_x, b.margin.left) - b.rect.x, 0.0, diagnostics);
        super::block::place_block(
            b,
            y as f64 - self.offset as f64 + b.margin.top as f64,
            diagnostics,
        );
        if self.entries.len() < MAX_FLOATS {
            self.entries.push((
                b.style.float,
                Rect {
                    x: margin_x,
                    y,
                    width: margin_width,
                    height: margin_height,
                },
            ));
        } else {
            self.overflow_bottom = add(y, margin_height);
            if !diagnostics
                .iter()
                .any(|d| d.message.starts_with("float exclusion limit"))
            {
                diagnostics.push(Diagnostic { node_id: b.node_id, message: format!("float exclusion limit {MAX_FLOATS} reached; subsequent floats stack below tracked floats with conservative full-width exclusion") });
            }
        }
    }
}

pub(super) fn layout_float(
    b: &mut LayoutBox,
    width: f32,
    height: Option<f32>,
    font: &crate::text::FontContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let padding = resolve_edges(
        &b.style.padding,
        b.style.font_size,
        width,
        b.node_id,
        "float padding",
        diagnostics,
    );
    let margins = resolve_margins(&b.style, width, b.node_id, diagnostics);
    let decor =
        padding.left + padding.right + b.style.border_width.left + b.style.border_width.right;
    let vertical =
        padding.top + padding.bottom + b.style.border_width.top + b.style.border_width.bottom;
    let explicit =
        super::sizing::definite_size(b, b.style.width.as_ref(), Some(width), decor, diagnostics);
    let intrinsic = super::positioning::intrinsic_content(b, width, font, diagnostics);
    let available =
        (width - decor - margins.left.unwrap_or(0.0) - margins.right.unwrap_or(0.0)).max(0.0);
    let used = explicit.unwrap_or_else(|| available.max(intrinsic.0).min(intrinsic.1));
    let bounds = super::sizing::bounds(b, true, Some(width), decor, 0.0, diagnostics);
    let height =
        super::sizing::definite_height(b, b.style.height.as_ref(), height, vertical, diagnostics);
    super::block::layout_sized(
        b,
        width,
        used.clamp(bounds.0, bounds.1),
        height,
        (explicit.is_some(), true),
        font,
        diagnostics,
    );
}
