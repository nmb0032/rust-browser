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
            DisplayItem::ViewportBackground { w, h, color } => {
                draw_rect(
                    pixels,
                    width,
                    height,
                    PixelRect {
                        left: 0,
                        top: 0,
                        right: (*w * scale).ceil() as i32,
                        bottom: (*h * scale).ceil() as i32,
                    },
                    *color,
                );
            }
            DisplayItem::Rect { x, y, w, h, color } => {
                draw_rect(
                    pixels,
                    width,
                    height,
                    PixelRect {
                        left: (*x * scale).floor() as i32,
                        top: ((*y - scroll_offset) * scale).floor() as i32,
                        right: ((*x + *w) * scale).ceil() as i32,
                        bottom: ((*y - scroll_offset + *h) * scale).ceil() as i32,
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
            DisplayItem::Image { x, y, w, h, image } => {
                let left = *x * scale;
                let top = (*y - scroll_offset) * scale;
                let image_width = *w * scale;
                let image_height = *h * scale;
                if image_width <= 0.0 || image_height <= 0.0 {
                    continue;
                }
                let start_x = left.floor().clamp(0.0, width as f32) as usize;
                let start_y = top.floor().clamp(0.0, height as f32) as usize;
                let end_x = (left + image_width).ceil().clamp(0.0, width as f32) as usize;
                let end_y = (top + image_height).ceil().clamp(0.0, height as f32) as usize;
                for target_y in start_y..end_y {
                    let source_y = (((target_y as f32 + 0.5 - top) / image_height
                        * image.height() as f32) as u32)
                        .min(image.height() - 1);
                    for target_x in start_x..end_x {
                        let source_x = (((target_x as f32 + 0.5 - left) / image_width
                            * image.width() as f32) as u32)
                            .min(image.width() - 1);
                        let source =
                            (source_y as usize * image.width() as usize + source_x as usize) * 4;
                        let rgba = &image.rgba()[source..source + 4];
                        let foreground = (u32::from(rgba[0]) << 16)
                            | (u32::from(rgba[1]) << 8)
                            | u32::from(rgba[2]);
                        let target = target_y * width + target_x;
                        pixels[target] = blend(pixels[target], foreground, u32::from(rgba[3]));
                    }
                }
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

    if start_x >= end_x || start_y >= end_y {
        return;
    }

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
        let left = pen_x + metrics.xmin as f32;
        if left + metrics.width as f32 * 1.2 <= 0.0 || left >= width as f32 {
            pen_x += metrics.advance_width;
            continue;
        }

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
    fn images_scale_clip_scroll_and_alpha_blend_with_the_destination() {
        use std::{io::Cursor, sync::Arc};
        let source = image::ImageBuffer::from_pixel(2, 1, image::Rgba([255u8, 0, 0, 128]));
        let mut encoded = Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let image = Arc::new(crate::media::DecodedImage::decode(&encoded.into_inner()).unwrap());
        let items = [DisplayItem::Image {
            x: 1.0,
            y: 3.0,
            w: 2.0,
            h: 1.0,
            image,
        }];
        let mut pixels = vec![BACKGROUND_COLOR; 8 * 4];
        let font = load_bundled_font().unwrap();
        rasterize_display_list(&items, &font, &mut pixels, 8, 4, 2.0, 0.0);
        assert!(pixels.iter().all(|pixel| *pixel == BACKGROUND_COLOR));
        rasterize_display_list(&items, &font, &mut pixels, 8, 4, 2.0, 2.0);
        for y in 0..4 {
            for x in 0..8 {
                let expected = if (2..6).contains(&x) && (2..4).contains(&y) {
                    0xFF7F7F
                } else {
                    BACKGROUND_COLOR
                };
                assert_eq!(pixels[y * 8 + x], expected, "pixel ({x}, {y})");
            }
        }
        let mut clipped = vec![BACKGROUND_COLOR; 2];
        rasterize_display_list(&items, &font, &mut clipped, 2, 1, 1.0, 3.0);
        assert_eq!(clipped, [BACKGROUND_COLOR, 0xFF7F7F]);
    }

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

    #[test]
    fn backgrounds_and_borders_scroll_with_text_while_the_viewport_stays_fixed() {
        let items = [
            DisplayItem::ViewportBackground {
                w: 20.0,
                h: 20.0,
                color: BACKGROUND_COLOR,
            },
            DisplayItem::Rect {
                x: 2.0,
                y: 100.0,
                w: 10.0,
                h: 10.0,
                color: 0xFF0000,
            },
            DisplayItem::Rect {
                x: 2.0,
                y: 100.0,
                w: 10.0,
                h: 2.0,
                color: 0x0000FF,
            },
        ];
        let font = load_bundled_font().unwrap();
        let mut pixels = vec![0; 400];
        rasterize_display_list(&items, &font, &mut pixels, 20, 20, 1.0, 95.0);
        assert_eq!(pixels[5 * 20 + 3], 0x0000FF);
        assert_eq!(pixels[8 * 20 + 3], 0xFF0000);
        assert_eq!(pixels[19 * 20 + 3], BACKGROUND_COLOR);
        rasterize_display_list(&items, &font, &mut pixels, 20, 20, 1.0, 0.0);
        assert!(pixels.iter().all(|pixel| *pixel == BACKGROUND_COLOR));
    }

    #[test]
    fn rasterizes_computed_box_model_and_inherited_text_colors() {
        let document = crate::html::parser::parse(
            "<div style='width:20px; padding:2px; border-width:2px;\
             border-color:blue; background-color:yellow; color:red'>A</div>",
        )
        .unwrap();
        let font = load_bundled_font().unwrap();
        let mut tree = crate::layout::build_layout_tree(&document);
        tree.layout(100.0, &font);
        let items = crate::paint::build_display_list(&tree, 100.0, 100.0, false);
        let mut pixels = vec![BACKGROUND_COLOR; 10_000];
        rasterize_display_list(&items, &font, &mut pixels, 100, 100, 1.0, 0.0);
        assert_eq!(pixels[24 * 100 + 24], 0x0000FF);
        assert_eq!(pixels[27 * 100 + 27], 0xFFFF00);
        assert_eq!(pixels[23 * 100 + 24], BACKGROUND_COLOR);
        assert!(pixels.iter().any(|pixel| {
            let green = (*pixel >> 8) & 0xFF;
            (*pixel >> 16) & 0xFF == 255 && green < 255 && *pixel & 0xFF == 0
        }));
    }

    #[test]
    fn clips_extreme_horizontal_css_positions_without_integer_overflow() {
        let font = load_bundled_font().unwrap();
        let mut pixels = vec![BACKGROUND_COLOR; 400];
        for x in [1e30, -1e30] {
            rasterize_display_list(
                &[DisplayItem::Text {
                    x,
                    y: 0.0,
                    text: "A".into(),
                    size: 16.0,
                    color: TEXT_COLOR,
                    bold: true,
                    italic: true,
                }],
                &font,
                &mut pixels,
                20,
                20,
                1.0,
                0.0,
            );
        }
        assert!(pixels.iter().all(|pixel| *pixel == BACKGROUND_COLOR));
    }
}
