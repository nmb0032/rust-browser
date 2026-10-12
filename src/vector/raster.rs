//! Device-time vector coverage; no SVG bitmap is retained in Frame or layout.
use tiny_skia::{Pixmap, Transform};

use super::{Brush, Command, Scene, Spread};

pub(crate) fn render(
    scene: &Scene,
    width: u32,
    height: u32,
    fonts: &crate::text::FontContext,
) -> Option<Vec<u32>> {
    render_at(
        scene,
        width,
        height,
        width as f32 / scene.width,
        height as f32 / scene.height,
        fonts,
    )
}

pub(crate) fn render_at(
    scene: &Scene,
    width: u32,
    height: u32,
    sx: f32,
    sy: f32,
    fonts: &crate::text::FontContext,
) -> Option<Vec<u32>> {
    if width == 0
        || height == 0
        || width as usize * height as usize > super::MAX_VECTOR_PIXELS
        || scene.width <= 0.0
        || scene.height <= 0.0
    {
        return None;
    }
    validate(scene, width, height).ok()?;
    let mut pixmap = Pixmap::new(width, height)?;
    draw(scene, &mut pixmap, Transform::from_scale(sx, sy), fonts, 0);
    Some(
        pixmap
            .pixels()
            .iter()
            .map(|p| {
                let c = p.demultiply();
                ((c.alpha() as u32) << 24)
                    | ((c.red() as u32) << 16)
                    | ((c.green() as u32) << 8)
                    | c.blue() as u32
            })
            .collect(),
    )
}

pub(crate) fn validate(scene: &Scene, width: u32, height: u32) -> Result<(), String> {
    fn visit(
        scene: &Scene,
        matrix: Transform,
        pixels: usize,
        live: usize,
        depth: usize,
        count: &mut usize,
    ) -> Result<(), String> {
        if depth > 64 || live > 16_777_216 {
            return Err("vector group/depth budget exceeds 64 MiB or 64 levels".into());
        }
        for command in &scene.commands {
            *count += 1;
            if *count > super::MAX_COMMANDS * 4 {
                return Err("vector expanded paint budget exceeded".into());
            }
            match command {
                Command::Group {
                    scene,
                    transform,
                    opacity,
                    clip,
                } => visit(
                    scene,
                    matrix.pre_concat(*transform),
                    pixels,
                    live + if *opacity < 1.0 || clip.is_some() {
                        pixels * 2
                    } else {
                        0
                    },
                    depth + 1,
                    count,
                )?,
                Command::Path {
                    fill,
                    stroke,
                    transform,
                    ..
                } => {
                    for brush in fill.iter().chain(stroke.iter().map(|s| &s.brush)) {
                        if let Brush::Pattern {
                            scene,
                            tile,
                            transform: local,
                            ..
                        } = brush
                        {
                            let (sx, sy) =
                                matrix.pre_concat(*transform).pre_concat(*local).get_scale();
                            let w = (tile.width * sx).ceil();
                            let h = (tile.height * sy).ceil();
                            let n = area(w, h)?;
                            visit(
                                scene,
                                Transform::from_scale(w / scene.width, h / scene.height),
                                n,
                                live + n * 2,
                                depth + 1,
                                count,
                            )?;
                        }
                    }
                }
                Command::Text {
                    glyphs, transform, ..
                } => {
                    let (sx, sy) = matrix.pre_concat(*transform).get_scale();
                    let scale = sx.max(sy).clamp(0.125, 8.0);
                    let bounds = glyphs.ink_bounds(0.0, 0.0, 0.0);
                    area(
                        (bounds.width * scale + 4.0).ceil().max(1.0),
                        (bounds.height * scale + 4.0).ceil().max(1.0),
                    )?;
                }
                Command::Image {
                    image,
                    rect,
                    transform: local,
                    ..
                } if image.vector().is_some() => {
                    let matrix = matrix
                        .pre_concat(*local)
                        .pre_translate(rect.x, rect.y)
                        .pre_scale(
                            rect.width / image.width() as f32,
                            rect.height / image.height() as f32,
                        );
                    let (sx, sy) = matrix.get_scale();
                    let (w, h) = (
                        (image.width() as f32 * sx).ceil().max(1.0),
                        (image.height() as f32 * sy).ceil().max(1.0),
                    );
                    let n = area(w, h)?;
                    let scene = image.vector().expect("vector image");
                    visit(
                        scene,
                        Transform::from_scale(w / scene.width, h / scene.height),
                        n,
                        live + n * 2,
                        depth + 1,
                        count,
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    let pixels = area(width as f32, height as f32)?;
    if scene.width <= 0.0 || scene.height <= 0.0 {
        return Ok(());
    }
    visit(
        scene,
        Transform::from_scale(width as f32 / scene.width, height as f32 / scene.height),
        pixels,
        pixels,
        0,
        &mut 0,
    )
}
fn area(width: f32, height: f32) -> Result<usize, String> {
    if !width.is_finite()
        || !height.is_finite()
        || width < 0.0
        || height < 0.0
        || width > 4096.0
        || height > 4096.0
        || width as f64 * height as f64 > super::MAX_VECTOR_PIXELS as f64
    {
        return Err("vector texture exceeds 4096 axes or 4,194,304 pixels".into());
    }
    Ok(width as usize * height as usize)
}

fn sk_color(color: crate::css::values::ResolvedColor) -> tiny_skia::Color {
    let (rgb, a) = color.to_srgb8();
    tiny_skia::Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, a as u8)
}
fn spread(value: Spread) -> tiny_skia::SpreadMode {
    match value {
        Spread::Pad => tiny_skia::SpreadMode::Pad,
        Spread::Repeat => tiny_skia::SpreadMode::Repeat,
        Spread::Reflect => tiny_skia::SpreadMode::Reflect,
    }
}

fn draw(
    scene: &Scene,
    pixmap: &mut Pixmap,
    transform: Transform,
    fonts: &crate::text::FontContext,
    depth: usize,
) {
    if depth > 64 || !transform.is_finite() {
        return;
    }
    for command in &scene.commands {
        match command {
            Command::Path {
                path,
                transform: local,
                fill,
                even_odd,
                stroke,
                ..
            } => {
                let transform = transform.pre_concat(*local);
                if let Some(brush) = fill {
                    with_paint(brush, transform, fonts, depth, |paint| {
                        pixmap.fill_path(
                            path,
                            &paint,
                            if *even_odd {
                                tiny_skia::FillRule::EvenOdd
                            } else {
                                tiny_skia::FillRule::Winding
                            },
                            transform,
                            None,
                        )
                    });
                }
                if let Some(stroke) = stroke {
                    with_paint(&stroke.brush, transform, fonts, depth, |paint| {
                        pixmap.stroke_path(
                            path,
                            &paint,
                            &tiny_skia::Stroke {
                                width: stroke.width,
                                line_cap: stroke.cap,
                                line_join: stroke.join,
                                miter_limit: stroke.miter,
                                dash: tiny_skia::StrokeDash::new(
                                    stroke.dash.clone(),
                                    stroke.dash_offset,
                                ),
                            },
                            transform,
                            None,
                        )
                    });
                }
            }
            Command::Group {
                scene,
                transform: local,
                opacity,
                clip,
            } => {
                let matrix = transform.pre_concat(*local);
                if *opacity == 1.0 && clip.is_none() {
                    draw(scene, pixmap, matrix, fonts, depth + 1);
                    continue;
                }
                let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
                    continue;
                };
                draw(scene, &mut layer, matrix, fonts, depth + 1);
                let mask = clip.and_then(super::rect_path).and_then(|path| {
                    let mut mask = tiny_skia::Mask::new(pixmap.width(), pixmap.height())?;
                    mask.fill_path(&path, tiny_skia::FillRule::Winding, true, matrix);
                    Some(mask)
                });
                pixmap.draw_pixmap(
                    0,
                    0,
                    layer.as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity: *opacity,
                        ..Default::default()
                    },
                    Transform::identity(),
                    mask.as_ref(),
                );
            }
            Command::Text {
                glyphs,
                x,
                baseline,
                color,
                transform: local,
                ..
            } => {
                text(
                    pixmap,
                    glyphs,
                    *x,
                    *baseline,
                    *color,
                    transform.pre_concat(*local),
                    fonts,
                );
            }
            Command::Image {
                image,
                rect,
                transform: local,
                ..
            } => {
                let matrix = transform
                    .pre_concat(*local)
                    .pre_translate(rect.x, rect.y)
                    .pre_scale(
                        rect.width / image.width() as f32,
                        rect.height / image.height() as f32,
                    );
                let pixels = if let Some(scene) = image.vector() {
                    let (sx, sy) = matrix.get_scale();
                    let w = (image.width() as f32 * sx).ceil().clamp(1.0, 4096.0) as u32;
                    let h = (image.height() as f32 * sy).ceil().clamp(1.0, 4096.0) as u32;
                    render(scene, w, h, fonts).map(|rgba| (rgba, w, h))
                } else {
                    Some((
                        image
                            .rgba()
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|p| {
                                ((p[3] as u32) << 24)
                                    | ((p[0] as u32) << 16)
                                    | ((p[1] as u32) << 8)
                                    | p[2] as u32
                            })
                            .collect(),
                        image.width(),
                        image.height(),
                    ))
                };
                if let Some((pixels, w, h)) = pixels
                    && let Some(source) = from_pixels(&pixels, w, h)
                {
                    let matrix = matrix.pre_scale(
                        image.width() as f32 / w as f32,
                        image.height() as f32 / h as f32,
                    );
                    pixmap.draw_pixmap(
                        0,
                        0,
                        source.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        matrix,
                        None,
                    );
                }
            }
        }
    }
}

fn with_paint(
    brush: &Brush,
    transform: Transform,
    fonts: &crate::text::FontContext,
    depth: usize,
    mut draw: impl FnMut(tiny_skia::Paint<'_>),
) {
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    match brush {
        Brush::Solid(color) => paint.set_color(sk_color(*color)),
        Brush::Linear {
            start,
            end,
            stops,
            spread: mode,
            transform,
            ..
        } => {
            let stops = stops
                .iter()
                .map(|&(offset, color)| tiny_skia::GradientStop::new(offset, sk_color(color)))
                .collect();
            let Some(shader) = tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(start.0, start.1),
                tiny_skia::Point::from_xy(end.0, end.1),
                stops,
                spread(*mode),
                *transform,
            ) else {
                return;
            };
            paint.shader = shader;
        }
        Brush::Radial {
            center,
            focus,
            radius,
            stops,
            spread: mode,
            transform,
            ..
        } => {
            let stops = stops
                .iter()
                .map(|&(offset, color)| tiny_skia::GradientStop::new(offset, sk_color(color)))
                .collect();
            let Some(shader) = tiny_skia::RadialGradient::new(
                tiny_skia::Point::from_xy(focus.0, focus.1),
                tiny_skia::Point::from_xy(center.0, center.1),
                *radius,
                stops,
                spread(*mode),
                *transform,
            ) else {
                return;
            };
            paint.shader = shader;
        }
        Brush::Pattern {
            scene,
            tile,
            transform: local,
            opacity,
        } => {
            if depth >= super::MAX_DEPTH {
                return;
            }
            let (sx, sy) = transform.pre_concat(*local).get_scale();
            let w = (tile.width * sx).ceil().clamp(1.0, 4096.0) as u32;
            let h = (tile.height * sy).ceil().clamp(1.0, 4096.0) as u32;
            let Some(pixels) = render(scene, w, h, fonts) else {
                return;
            };
            let Some(source) = from_pixels(&pixels, w, h) else {
                return;
            };
            paint.shader = tiny_skia::Pattern::new(
                source.as_ref(),
                tiny_skia::SpreadMode::Repeat,
                tiny_skia::FilterQuality::Nearest,
                *opacity,
                local
                    .pre_translate(tile.x, tile.y)
                    .pre_scale(tile.width / w as f32, tile.height / h as f32),
            );
            draw(paint);
            return;
        }
    }
    draw(paint);
}

fn from_pixels(pixels: &[u32], width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    for (target, &source) in pixmap.pixels_mut().iter_mut().zip(pixels) {
        *target = tiny_skia::ColorU8::from_rgba(
            (source >> 16) as u8,
            (source >> 8) as u8,
            source as u8,
            (source >> 24) as u8,
        )
        .premultiply();
    }
    Some(pixmap)
}

fn text(
    pixmap: &mut Pixmap,
    glyphs: &crate::text::ShapedText,
    x: f32,
    baseline: f32,
    color: crate::css::values::ResolvedColor,
    transform: Transform,
    fonts: &crate::text::FontContext,
) {
    // Reuse the exact indexed glyph mask/synthesis/variation rasterizer, then
    // transform each bounded run. The shaped glyph offsets are never rewritten.
    let (sx, sy) = transform.get_scale();
    let scale = sx.max(sy).clamp(0.125, 8.0);
    let direct = transform.is_scale_translate()
        && transform.sx > 0.0
        && transform.sy > 0.0
        && (transform.sx - transform.sy).abs() < 0.00001
        && (scale - transform.sx).abs() < 0.00001;
    let (x, baseline) = if direct {
        (x + transform.tx / scale, baseline + transform.ty / scale)
    } else {
        (x, baseline)
    };
    let bounds = glyphs.ink_bounds(0.0, x, baseline);
    let origin_x = (bounds.x * scale).floor() / scale - 2.0 / scale;
    let origin_y = (bounds.y * scale).floor() / scale - 2.0 / scale;
    let w = ((bounds.width + 4.0 / scale) * scale).ceil().max(1.0) as u32;
    let h = ((bounds.height + 4.0 / scale) * scale).ceil().max(1.0) as u32;
    if w as usize * h as usize > super::MAX_VECTOR_PIXELS {
        return;
    }
    let mut pixels = vec![0; w as usize * h as usize];
    crate::paint::rasterizer::rasterize_rgba(
        &[crate::paint::DisplayItem::ColorText {
            x: x - origin_x,
            y: baseline - origin_y,
            baseline: baseline - origin_y,
            text: String::new(),
            glyphs: Arc::new(glyphs.clone()),
            size: 0.0,
            color,
            bold: false,
            italic: false,
        }],
        fonts,
        &mut pixels,
        w as usize,
        h as usize,
        scale as f64,
        0.0,
    );
    if let Some(source) = from_pixels(&pixels, w, h) {
        pixmap.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            if direct {
                Transform::from_translate(origin_x * scale, origin_y * scale)
            } else {
                transform
                    .pre_translate(origin_x, origin_y)
                    .pre_scale(1.0 / scale, 1.0 / scale)
            },
            None,
        );
    }
}

use std::sync::Arc;
