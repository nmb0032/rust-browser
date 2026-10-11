use super::DisplayItem;

#[derive(Clone, Copy)]
struct PixelRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl PixelRect {
    fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            right: self.right.min(other.right),
            top: self.top.max(other.top),
            bottom: self.bottom.min(other.bottom),
        }
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

struct TextRun<'a> {
    glyphs: &'a crate::text::ShapedText,
    start_x: f32,
    baseline_y: f32,
    scale: f32,
    color: u32,
    alpha: u32,
    clip: PixelRect,
}

pub(crate) fn rasterize_display_list(
    display_list: &[DisplayItem],
    _font: &crate::text::FontContext,
    pixels: &mut [u32],
    width: usize,
    height: usize,
    scale_factor: f64,
    scroll_offset: f32,
) {
    let scale = scale_factor as f32;
    if !scale.is_finite()
        || scale <= 0.0
        || width.checked_mul(height).is_none_or(|n| n > pixels.len())
    {
        return;
    }

    let mut item_scroll = scroll_offset;
    let mut spatial = crate::layout::SpatialState::default();
    let framebuffer = PixelRect {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    for item in display_list {
        // Pixel centers decide coverage at fractional clip edges. Geometry is
        // never trimmed before image sampling or indexed glyph rasterization.
        let edge = |value: f32| (value * scale - 0.5).ceil() as i32;
        let clip = PixelRect {
            left: spatial.clip.x.map_or(0, |(a, _)| edge(a)),
            right: spatial.clip.x.map_or(width as i32, |(_, b)| edge(b)),
            top: spatial.clip.y.map_or(0, |(a, _)| edge(a - item_scroll)),
            bottom: spatial
                .clip
                .y
                .map_or(height as i32, |(_, b)| edge(b - item_scroll)),
        }
        .intersect(framebuffer);
        match item {
            DisplayItem::Spatial(state) => spatial = *state,
            DisplayItem::CoordinateSpace(space) => {
                item_scroll = if *space == crate::layout::CoordinateSpace::Document {
                    scroll_offset
                } else {
                    0.0
                };
            }
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
                    255,
                );
            }
            DisplayItem::Rect { x, y, w, h, .. } | DisplayItem::ColorRect { x, y, w, h, .. } => {
                let (color, alpha) = match item {
                    DisplayItem::Rect { color, .. } => (*color, 255),
                    DisplayItem::ColorRect { color, .. } => color.to_srgb8(),
                    _ => unreachable!(),
                };
                let Some(actual) = (crate::layout::Rect {
                    x: *x + spatial.offset.x,
                    y: *y + spatial.offset.y - item_scroll,
                    width: *w,
                    height: *h,
                })
                .actual(scale) else {
                    continue;
                };
                draw_rect(
                    pixels,
                    width,
                    height,
                    PixelRect {
                        left: actual.left,
                        top: actual.top,
                        right: actual.right,
                        bottom: actual.bottom,
                    }
                    .intersect(clip),
                    color,
                    alpha,
                );
            }
            DisplayItem::OutlineRect { x, y, w, h, color } => {
                let rect = PixelRect {
                    left: ((*x + spatial.offset.x) * scale).floor() as i32,
                    top: ((*y + spatial.offset.y - item_scroll) * scale).floor() as i32,
                    right: ((*x + spatial.offset.x + *w) * scale).ceil() as i32,
                    bottom: ((*y + spatial.offset.y - item_scroll + *h) * scale).ceil() as i32,
                };
                draw_outline_rect(pixels, width, height, rect, clip, scale, *color);
            }
            DisplayItem::Image { x, y, w, h, image } => {
                let left = (*x + spatial.offset.x) * scale;
                let top = (*y + spatial.offset.y - item_scroll) * scale;
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
                        if !clip.contains(target_x as i32, target_y as i32) {
                            continue;
                        }
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
                baseline,
                glyphs,
                ..
            }
            | DisplayItem::ColorText {
                x,
                baseline,
                glyphs,
                ..
            } => {
                let baseline = *baseline + spatial.offset.y - item_scroll;
                let (color, alpha) = match item {
                    DisplayItem::Text { color, .. } => (*color, 255),
                    DisplayItem::ColorText { color, .. } => color.to_srgb8(),
                    _ => unreachable!(),
                };
                draw_text(
                    pixels,
                    width,
                    height,
                    TextRun {
                        glyphs,
                        start_x: *x + spatial.offset.x,
                        baseline_y: baseline,
                        scale,
                        color,
                        alpha,
                        clip,
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
    clip: PixelRect,
    scale: f32,
    color: u32,
) {
    let thickness = scale.round().max(1.0) as i32;
    let inner_left = rect.left.saturating_add(thickness).min(rect.right);
    let inner_top = rect.top.saturating_add(thickness).min(rect.bottom);
    let inner_right = rect.right.saturating_sub(thickness).max(rect.left);
    let inner_bottom = rect.bottom.saturating_sub(thickness).max(rect.top);

    draw_rect(
        pixels,
        width,
        height,
        PixelRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: inner_top,
        }
        .intersect(clip),
        color,
        255,
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
        }
        .intersect(clip),
        color,
        255,
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
        }
        .intersect(clip),
        color,
        255,
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
        }
        .intersect(clip),
        color,
        255,
    );
}

fn draw_rect(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    rect: PixelRect,
    color: u32,
    alpha: u32,
) {
    let start_x = rect.left.max(0).min(width as i32) as usize;
    let end_x = rect.right.max(0).min(width as i32) as usize;
    let start_y = rect.top.max(0).min(height as i32) as usize;
    let end_y = rect.bottom.max(0).min(height as i32) as usize;

    if start_x >= end_x || start_y >= end_y {
        return;
    }

    for y in start_y..end_y {
        let row = &mut pixels[y * width + start_x..y * width + end_x];
        if alpha == 255 {
            row.fill(color);
        } else {
            for pixel in row {
                *pixel = blend(*pixel, color, alpha);
            }
        }
    }
}

fn draw_text(pixels: &mut [u32], width: usize, height: usize, run: TextRun<'_>) {
    for glyph in &run.glyphs.glyphs {
        let pen_x = ((run.start_x + glyph.x) * run.scale).round();
        let baseline = ((run.baseline_y - glyph.y) * run.scale).round();
        // Cull in floating point before converting hostile coordinates to ints.
        if !pen_x.is_finite()
            || !baseline.is_finite()
            || pen_x.abs() > i32::MAX as f32 / 2.0
            || baseline.abs() > i32::MAX as f32 / 2.0
        {
            continue;
        }
        let Some(bitmap) = glyph.face.rasterized(glyph.id, glyph.size * run.scale) else {
            continue;
        };
        let metrics = &bitmap.metrics;
        let top = baseline as i32 - (metrics.ymin + metrics.height as i32);
        let left = pen_x + metrics.xmin as f32;
        if glyph.rotation == crate::text::GlyphRotation::None
            && (left + metrics.width as f32 * 1.2 <= 0.0
                || left >= width as f32
                || top + metrics.height as i32 <= 0
                || top >= height as i32)
        {
            continue;
        }

        for glyph_y in 0..metrics.height {
            for glyph_x in 0..metrics.width {
                let italic_offset = if glyph.italic {
                    ((metrics.height - glyph_y) as f32 * 0.2).round() as i32
                } else {
                    0
                };
                let local_x = metrics.xmin + glyph_x as i32 + italic_offset;
                let local_y = -(metrics.ymin + metrics.height as i32) + glyph_y as i32;
                let (dx, dy) = match glyph.rotation {
                    crate::text::GlyphRotation::None => (local_x, local_y),
                    crate::text::GlyphRotation::Clockwise => (-local_y - 1, local_x),
                    crate::text::GlyphRotation::CounterClockwise => (local_y, -local_x - 1),
                };
                let x = pen_x as i32 + dx;
                let y = baseline as i32 + dy;

                if let Some((left, right)) = glyph.clip
                    && ((local_x as f32) < (left * run.scale).floor()
                        || (local_x as f32) >= (right * run.scale).floor())
                {
                    continue;
                }
                if x < 0
                    || y < 0
                    || x as usize >= width
                    || y as usize >= height
                    || !run.clip.contains(x, y)
                {
                    continue;
                }

                let coverage =
                    bitmap.pixels[glyph_y * metrics.width + glyph_x] as u32 * run.alpha / 255;
                let index = y as usize * width + x as usize;
                pixels[index] = blend(pixels[index], run.color, coverage);
                let (bold_x, bold_y) = match glyph.rotation {
                    crate::text::GlyphRotation::None => (x + 1, y),
                    crate::text::GlyphRotation::Clockwise => (x, y + 1),
                    crate::text::GlyphRotation::CounterClockwise => (x, y - 1),
                };
                if glyph.bold
                    && bold_x >= 0
                    && bold_y >= 0
                    && (bold_x as usize) < width
                    && (bold_y as usize) < height
                    && run.clip.contains(bold_x, bold_y)
                {
                    let bold_index = bold_y as usize * width + bold_x as usize;
                    pixels[bold_index] = blend(pixels[bold_index], run.color, coverage);
                }
            }
        }
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
    fn typed_colors_and_images_share_alpha_compositing_and_order() {
        use crate::css::values::parse_color;
        use std::{io::Cursor, sync::Arc};
        let color = |source| {
            parse_color(source)
                .unwrap()
                .resolve(crate::css::values::ResolvedColor::BLACK)
                .unwrap()
        };
        let rect = |color| DisplayItem::ColorRect {
            x: 0.0,
            y: 0.0,
            w: 2.0,
            h: 1.0,
            color,
        };
        let red = rect(color("color(display-p3 1 0 0 / .5)"));
        let blue = rect(color("rgb(0 0 255 / .5)"));
        let transparent = rect(color("rgb(0 255 0 / 0)"));
        let font = load_bundled_font().unwrap();
        let mut pixels = vec![BACKGROUND_COLOR; 2];
        rasterize_display_list(
            &[red.clone(), blue.clone(), transparent],
            &font,
            &mut pixels,
            2,
            1,
            1.0,
            0.0,
        );
        assert_eq!(pixels, [0x7f3fbf; 2]);
        let mut reverse = vec![BACKGROUND_COLOR; 2];
        rasterize_display_list(
            &[blue.clone(), red.clone()],
            &font,
            &mut reverse,
            2,
            1,
            1.0,
            0.0,
        );
        assert_eq!(reverse, [0xbf3f7f; 2]);

        let source = image::ImageBuffer::from_pixel(2, 1, image::Rgba([255u8, 0, 0, 128]));
        let mut encoded = Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let image = Arc::new(crate::media::DecodedImage::decode(&encoded.into_inner()).unwrap());
        let mut image_pixels = vec![BACKGROUND_COLOR; 2];
        rasterize_display_list(
            &[
                DisplayItem::Image {
                    x: 0.0,
                    y: 0.0,
                    w: 2.0,
                    h: 1.0,
                    image,
                },
                blue,
            ],
            &font,
            &mut image_pixels,
            2,
            1,
            1.0,
            0.0,
        );
        assert_eq!(image_pixels, pixels);
    }

    #[test]
    fn text_color_alpha_multiplies_glyph_coverage() {
        let font = load_bundled_font().unwrap();
        let color = crate::css::values::ResolvedColor::srgb(0.0, 0.0, 0.0, 0.5);
        let legacy = DisplayItem::Text {
            x: 0.0,
            y: 0.0,
            baseline: font.horizontal_line_metrics(16.0).unwrap().ascent,
            text: "M".into(),
            glyphs: font.shape(&crate::style::ComputedStyle::default(), "M"),
            size: 16.0,
            color: 0,
            bold: false,
            italic: false,
        };
        let typed = DisplayItem::ColorText {
            x: 0.0,
            y: 0.0,
            baseline: font.horizontal_line_metrics(16.0).unwrap().ascent,
            text: "M".into(),
            glyphs: font.shape(&crate::style::ComputedStyle::default(), "M"),
            size: 16.0,
            color,
            bold: false,
            italic: false,
        };
        let mut opaque = vec![BACKGROUND_COLOR; 32 * 32];
        let mut semi = opaque.clone();
        rasterize_display_list(&[legacy], &font, &mut opaque, 32, 32, 1.0, 0.0);
        rasterize_display_list(&[typed], &font, &mut semi, 32, 32, 1.0, 0.0);
        assert!(semi.iter().any(|pixel| *pixel != BACKGROUND_COLOR));
        for (opaque, semi) in opaque.into_iter().zip(semi) {
            let coverage = 255 - (opaque & 255);
            let expected = 255 - coverage * 128 / 255;
            assert_eq!(semi, expected * 0x010101);
        }
    }

    #[test]
    fn computed_background_border_and_text_alpha_are_painted_over_each_other() {
        let document = crate::html::parser::parse(
            "<div style='width:60px;
            background:rgb(255 0 0 / .5);border:3px solid rgb(0 0 255 / .5);
            color:rgb(0 255 0 / .5)'>M</div>",
        )
        .unwrap();
        let styles = crate::style::resolve(&document).styles;
        let mut layout = crate::layout::build_layout_tree_with_styles(&document, &styles);
        let font = load_bundled_font().unwrap();
        layout.layout(100.0, &font);
        let list = crate::paint::build_display_list(&layout, 100.0, 100.0, false);
        let mut pixels = vec![0; 100 * 100];
        rasterize_display_list(&list, &font, &mut pixels, 100, 100, 1.0, 0.0);
        assert!(
            pixels.contains(&0xff7f7f),
            "background was not alpha blended"
        );
        assert!(
            pixels.contains(&0x7f3fbf),
            "border did not blend over background"
        );
        assert!(
            pixels
                .iter()
                .any(|p| (p >> 16) & 255 < 255 && (p >> 8) & 255 > 127 && p & 255 < 127),
            "text did not blend over background"
        );
    }

    #[test]
    fn actual_rectangle_stage_matches_fractional_scaled_scrolled_raster_pixels() {
        let rect = crate::layout::Rect {
            x: 1.25,
            y: 2.0,
            width: 3.5,
            height: 2.5,
        };
        let actual = rect.actual(1.5).unwrap();
        assert_eq!(
            actual,
            crate::layout::ActualRect {
                left: 1,
                top: 3,
                right: 8,
                bottom: 7
            }
        );
        let font = load_bundled_font().unwrap();
        let mut pixels = vec![BACKGROUND_COLOR; 64];
        rasterize_display_list(
            &[DisplayItem::Rect {
                x: rect.x,
                y: rect.y + 0.25,
                w: rect.width,
                h: rect.height,
                color: 0xff0000,
            }],
            &font,
            &mut pixels,
            8,
            8,
            1.5,
            0.25,
        );
        for y in 0..8 {
            for x in 0..8 {
                let painted =
                    x >= actual.left && x < actual.right && y >= actual.top && y < actual.bottom;
                assert_eq!(
                    pixels[(y * 8 + x) as usize],
                    if painted { 0xff0000 } else { BACKGROUND_COLOR }
                );
            }
        }
        assert!(rect.actual(0.0).is_none());
    }

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
                baseline: 100.0 + font.horizontal_line_metrics(32.0).unwrap().ascent,
                text: "visible after scrolling".to_string(),
                glyphs: font.shape(
                    &crate::style::ComputedStyle {
                        font_size: 32.0,
                        ..Default::default()
                    },
                    "visible after scrolling",
                ),
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
                    baseline: font.horizontal_line_metrics(24.0).unwrap().ascent,
                    text: "A".to_string(),
                    glyphs: font.shape(
                        &crate::style::ComputedStyle {
                            font_size: 24.0,
                            bold,
                            italic,
                            ..Default::default()
                        },
                        "A",
                    ),
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
            "<div style='width:20px; padding:2px; border-width:2px; border-style:solid;\
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
                    baseline: font.horizontal_line_metrics(16.0).unwrap().ascent,
                    text: "A".into(),
                    glyphs: font.shape(&crate::style::ComputedStyle::default(), "A"),
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
