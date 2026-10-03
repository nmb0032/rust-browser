use super::{DisplayItem, LINE_HEIGHT};

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
                    (*x * scale).floor() as i32,
                    (*y * scale).floor() as i32,
                    ((*x + *w) * scale).ceil() as i32,
                    ((*y + *h) * scale).ceil() as i32,
                    *color,
                );
            }
            DisplayItem::Text {
                x,
                y,
                text,
                size,
                color,
            } => {
                let line_top = *y - scroll_offset;
                if line_top + LINE_HEIGHT <= 0.0 || line_top >= viewport_height {
                    continue;
                }

                let baseline = ((line_top + size * 1.25) * scale).round() as i32;
                draw_text(
                    font,
                    pixels,
                    width,
                    height,
                    text,
                    (*x * scale).round() as i32,
                    baseline,
                    *size * scale,
                    *color,
                );
            }
        }
    }
}

fn draw_rect(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    color: u32,
) {
    let start_x = left.max(0).min(width as i32) as usize;
    let end_x = right.max(0).min(width as i32) as usize;
    let start_y = top.max(0).min(height as i32) as usize;
    let end_y = bottom.max(0).min(height as i32) as usize;

    for y in start_y..end_y {
        pixels[y * width + start_x..y * width + end_x].fill(color);
    }
}

fn draw_text(
    font: &fontdue::Font,
    pixels: &mut [u32],
    width: usize,
    height: usize,
    text: &str,
    start_x: i32,
    baseline_y: i32,
    px: f32,
    color: u32,
) {
    let mut pen_x = start_x as f32;

    for character in text.chars() {
        let (metrics, bitmap) = font.rasterize(character, px);
        let top = baseline_y - (metrics.ymin + metrics.height as i32);

        for glyph_y in 0..metrics.height {
            for glyph_x in 0..metrics.width {
                let x = pen_x.round() as i32 + metrics.xmin + glyph_x as i32;
                let y = top + glyph_y as i32;

                if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                    continue;
                }

                let coverage = bitmap[glyph_y * metrics.width + glyph_x] as u32;
                let index = y as usize * width + x as usize;
                pixels[index] = blend(pixels[index], color, coverage);
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
            },
        ];
        let mut pixels = vec![BACKGROUND_COLOR; 120 * 60];

        rasterize_display_list(&items, &font, &mut pixels, 120, 60, 1.0, 0.0);
        assert!(pixels.iter().all(|pixel| *pixel == BACKGROUND_COLOR));

        rasterize_display_list(&items, &font, &mut pixels, 120, 60, 1.0, 90.0);
        assert!(pixels.iter().any(|pixel| *pixel != BACKGROUND_COLOR));
    }
}
