use super::DisplayItem;

#[derive(Clone, Copy)]
struct PixelRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

struct TextRun<'a> {
    text: &'a str,
    start_x: i32,
    baseline_y: i32,
    px: f32,
    color: u32,
    bold: bool,
    italic: bool,
}

pub(crate) fn rasterize_display_list(
    display_list: &[DisplayItem],
    font: &fontdue::Font,
    pixels: &mut [u32],
    width: usize,
    height: usize,
    scale_factor: f64,
    scroll_offset: f32,
) {
    let scale = scale_factor as f32;
    let viewport_height = height as f32 / scale;

    for item in display_list {
        match item {
            DisplayItem::Rect { x, y, w, h, color } => {
                draw_rect(
                    pixels,
                    width,
                    height,
                    PixelRect {
                        left: (*x * scale).floor() as i32,
                        top: (*y * scale).floor() as i32,
                        right: ((*x + *w) * scale).ceil() as i32,
                        bottom: ((*y + *h) * scale).ceil() as i32,
                    },
                    *color,
                );
            }
            DisplayItem::OutlineRect { x, y, w, h, color } => {
                let rect = PixelRect {
                    left: (*x * scale).floor() as i32,
                    top: ((*y - scroll_offset) * scale).floor() as i32,
                    right: ((*x + *w) * scale).ceil() as i32,
                    bottom: ((*y - scroll_offset + *h) * scale).ceil() as i32,
                };
                draw_outline_rect(pixels, width, height, rect, scale, *color);
            }
            DisplayItem::Text {
                x,
                y,
                text,
                size,
                color,
                bold,
                italic,
            } => {
                let line_top = *y - scroll_offset;
                if line_top + size * 1.25 <= 0.0 || line_top >= viewport_height {
                    continue;
                }

                let baseline = ((line_top + size * 1.25) * scale).round() as i32;
                draw_text(
                    font,
                    pixels,
                    width,
                    height,
                    TextRun {
                        text,
                        start_x: (*x * scale).round() as i32,
                        baseline_y: baseline,
                        px: *size * scale,
                        color: *color,
                        bold: *bold,
                        italic: *italic,
                    },
                );
            }
        }
    }
}

fn draw_outline_rect(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    rect: PixelRect,
    scale: f32,
    color: u32,
) {
    let thickness = scale.round().max(1.0) as i32;
    let inner_left = (rect.left + thickness).min(rect.right);
    let inner_top = (rect.top + thickness).min(rect.bottom);
    let inner_right = (rect.right - thickness).max(rect.left);
    let inner_bottom = (rect.bottom - thickness).max(rect.top);

    draw_rect(
        pixels,
        width,
        height,
        PixelRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: inner_top,
        },
        color,
    );
    draw_rect(
        pixels,
        width,
        height,
        PixelRect {
            left: rect.left,
            top: inner_bottom,
            right: rect.right,
            bottom: rect.bottom,
        },
        color,
    );
    draw_rect(
        pixels,
        width,
        height,
        PixelRect {
            left: rect.left,
            top: inner_top,
            right: inner_left,
            bottom: inner_bottom,
        },
        color,
    );
    draw_rect(
        pixels,
        width,
        height,
        PixelRect {
            left: inner_right,
            top: inner_top,
            right: rect.right,
            bottom: inner_bottom,
        },
        color,
    );
}

fn draw_rect(pixels: &mut [u32], width: usize, height: usize, rect: PixelRect, color: u32) {
    let start_x = rect.left.max(0).min(width as i32) as usize;
    let end_x = rect.right.max(0).min(width as i32) as usize;
    let start_y = rect.top.max(0).min(height as i32) as usize;
    let end_y = rect.bottom.max(0).min(height as i32) as usize;

    for y in start_y..end_y {
        pixels[y * width + start_x..y * width + end_x].fill(color);
    }
}

fn draw_text(
    font: &fontdue::Font,
    pixels: &mut [u32],
    width: usize,
    height: usize,
    run: TextRun<'_>,
) {
    let mut pen_x = run.start_x as f32;

    for character in run.text.chars() {
        let (metrics, bitmap) = font.rasterize(character, run.px);
        let top = run.baseline_y - (metrics.ymin + metrics.height as i32);

        for glyph_y in 0..metrics.height {
            for glyph_x in 0..metrics.width {
                let italic_offset = if run.italic {
                    ((metrics.height - glyph_y) as f32 * 0.2).round() as i32
                } else {
                    0
                };
                let x = pen_x.round() as i32 + metrics.xmin + glyph_x as i32 + italic_offset;
                let y = top + glyph_y as i32;

                if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                    continue;
                }

                let coverage = bitmap[glyph_y * metrics.width + glyph_x] as u32;
                let index = y as usize * width + x as usize;
                pixels[index] = blend(pixels[index], run.color, coverage);
                if run.bold && x + 1 >= 0 && ((x + 1) as usize) < width {
                    let bold_index = y as usize * width + (x + 1) as usize;
                    pixels[bold_index] = blend(pixels[bold_index], run.color, coverage);
                }
            }
        }

        pen_x += metrics.advance_width;
    }
}

fn blend(background: u32, foreground: u32, coverage: u32) -> u32 {
    let inverse_coverage = 255 - coverage;
    let red = (((foreground >> 16) & 0xFF) * coverage
        + ((background >> 16) & 0xFF) * inverse_coverage)
        / 255;
    let green = (((foreground >> 8) & 0xFF) * coverage
        + ((background >> 8) & 0xFF) * inverse_coverage)
        / 255;
    let blue = ((foreground & 0xFF) * coverage + (background & 0xFF) * inverse_coverage) / 255;

    (red << 16) | (green << 8) | blue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::load_bundled_font;

    const BACKGROUND_COLOR: u32 = 0x00FF_FFFF;
    const TEXT_COLOR: u32 = 0x0000_0000;

    #[test]
    fn skips_text_lines_outside_the_viewport() {
        let font = load_bundled_font().unwrap();
        let items = [
            DisplayItem::Rect {
                x: 0.0,
                y: 0.0,
                w: 120.0,
                h: 60.0,
                color: BACKGROUND_COLOR,
            },
            DisplayItem::Text {
                x: 24.0,
                y: 100.0,
                text: "visible after scrolling".to_string(),
                size: 32.0,
                color: TEXT_COLOR,
                bold: false,
                italic: false,
            },
        ];
        let mut pixels = vec![BACKGROUND_COLOR; 120 * 60];

        rasterize_display_list(&items, &font, &mut pixels, 120, 60, 1.0, 0.0);
        assert!(pixels.iter().all(|pixel| *pixel == BACKGROUND_COLOR));

        rasterize_display_list(&items, &font, &mut pixels, 120, 60, 1.0, 90.0);
        assert!(pixels.iter().any(|pixel| *pixel != BACKGROUND_COLOR));
    }

    #[test]
    fn draws_layout_outlines_without_filling_the_box() {
        let items = [
            DisplayItem::Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                color: BACKGROUND_COLOR,
            },
            DisplayItem::OutlineRect {
                x: 2.0,
                y: 2.0,
                w: 5.0,
                h: 5.0,
                color: 0x00FF_0000,
            },
        ];
        let mut pixels = vec![BACKGROUND_COLOR; 10 * 10];

        rasterize_display_list(
            &items,
            &load_bundled_font().unwrap(),
            &mut pixels,
            10,
            10,
            1.0,
            0.0,
        );

        assert_eq!(pixels[2 * 10 + 2], 0x00FF_0000);
        assert_eq!(pixels[4 * 10 + 4], BACKGROUND_COLOR);
    }

    #[test]
    fn rasterizes_synthetic_bold_and_italic_styles() {
        let font = load_bundled_font().unwrap();
        let render = |bold, italic| {
            let mut pixels = vec![BACKGROUND_COLOR; 80 * 40];
            rasterize_display_list(
                &[DisplayItem::Text {
                    x: 4.0,
                    y: 0.0,
                    text: "A".to_string(),
                    size: 24.0,
                    color: TEXT_COLOR,
                    bold,
                    italic,
                }],
                &font,
                &mut pixels,
                80,
                40,
                1.0,
                0.0,
            );
            pixels
        };

        let regular = render(false, false);
        assert_ne!(regular, render(true, false));
        assert_ne!(regular, render(false, true));
    }
}
