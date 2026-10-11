use super::DisplayItem;
use crate::{
    layout::{LayoutBox, Rect, TextFragment},
    style::typography::{Keyword as K, Value},
};

pub(super) fn append(b: &LayoutBox, text: &TextFragment, items: &mut Vec<DisplayItem>) {
    for source in &b.style().decoration_sources {
        let mut face = source.face.shaper();
        face.set_variations(
            &source
                .coordinates
                .iter()
                .map(|&(tag, value)| rustybuzz::Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(&tag),
                    value,
                })
                .collect::<Vec<_>>(),
        );
        let scale = source.size / face.units_per_em() as f32;
        let metrics = face.underline_metrics();
        let thickness = match &source.thickness {
            Value::Length(l) => l.resolve(source.size, source.size),
            Value::Keyword(K::FromFont) => {
                metrics.map_or(source.size / 16.0, |m| m.thickness as f32 * scale)
            }
            _ => source.size / 16.0,
        }
        .max(0.0);
        if thickness == 0.0 {
            continue;
        }
        let horizontal = !b.style().writing_mode.vertical();
        for (bit, offset) in [
            (
                1,
                if source.position == K::Under {
                    source.size * 0.25
                } else {
                    metrics.map_or(source.size * 0.1, |m| -m.position as f32 * scale)
                } + source.offset,
            ),
            (2, -source.size * 0.8),
            (4, -source.size * 0.3),
        ] {
            if source.lines & bit == 0 {
                continue;
            }
            let mut rect = if horizontal {
                Rect {
                    x: text.x,
                    y: text.baseline + offset,
                    width: text.width.max(0.0),
                    height: thickness,
                }
            } else {
                // Native vertical baselines are physical x; underline is on
                // the right unless an explicit left side was selected.
                let side = if source.position == K::Left {
                    -1.0
                } else {
                    1.0
                };
                Rect {
                    x: text.x + side * offset,
                    y: text.y,
                    width: thickness,
                    height: text.height.max(0.0),
                }
            };
            let (begin, end) = if horizontal {
                (rect.x, rect.x + rect.width)
            } else {
                (rect.y, rect.y + rect.height)
            };
            let mut exclusions = Vec::new();
            if bit == 1 && source.skip != K::None {
                for g in &text.glyphs.glyphs {
                    let mut run = crate::text::ShapedText::default();
                    run.glyphs.push(g.clone());
                    let ink = run.ink_bounds(g.size, text.x, text.baseline);
                    if horizontal && ink.y < rect.y + thickness && ink.y + ink.height > rect.y {
                        exclusions.push((ink.x - thickness, ink.x + ink.width + thickness));
                    } else if !horizontal
                        && ink.x < rect.x + thickness
                        && ink.x + ink.width > rect.x
                    {
                        exclusions.push((ink.y - thickness, ink.y + ink.height + thickness));
                    }
                }
            }
            exclusions.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut cursor = begin;
            for (left, right) in exclusions.into_iter().chain(std::iter::once((end, end))) {
                if left > cursor {
                    if horizontal {
                        rect.x = cursor;
                        rect.width = left.min(end) - cursor;
                    } else {
                        rect.y = cursor;
                        rect.height = left.min(end) - cursor;
                    }
                    if rect.width > 0.0 && rect.height > 0.0 {
                        items.push(DisplayItem::TextDecoration {
                            rect,
                            horizontal,
                            style: source.style,
                            color: source.color,
                        });
                    }
                }
                cursor = cursor.max(right).min(end);
                if cursor >= end {
                    break;
                }
            }
        }
    }
}
