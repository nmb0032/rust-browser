//! Shared font selection, shaping and indexed raster sources. Positions are
//! CSS pixels, relative to an explicit baseline; clusters are UTF-8 byte offsets.
use std::{
    collections::{BTreeMap, HashMap},
    fmt, fs,
    hash::{Hash, Hasher},
    ops::Deref,
    sync::{Arc, Mutex, OnceLock},
};

use fontdb::{Database, Family, Query, Source, Style, Weight};
use rustybuzz::{BufferClusterLevel, UnicodeBuffer};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    css::values::{FamilyName, FontStyle},
    hash::FastHasher,
    layout::Rect,
    style::ComputedStyle,
};
pub(crate) mod transform;

const BUNDLED: &[u8] = include_bytes!("../../assets/fonts/noto-sans/NotoSans.ttf");
pub const MAX_TEXT_BYTES: usize = 65_536;
const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FACES: usize = 16;
const MAX_REQUESTS: usize = 128;
/// Shaped-run cache bound per generation. A large article shapes thousands
/// of paragraphs and re-measures each one for intrinsic sizing and for every
/// relayout, so the working set of one page must fit. Two generations give
/// least-recently-used behavior without per-entry bookkeeping.
const MAX_RUNS: usize = 16_384;
const MAX_RUN_BYTES: usize = 24 * 1024 * 1024;
const MAX_FONT_KEYS: usize = 4096;
const MAX_GLYPHS: usize = 200_000;
const MAX_COVERAGE_PROBES: usize = 8192;

type PlanKey = (
    rustybuzz::Direction,
    Option<rustybuzz::Script>,
    Option<rustybuzz::Language>,
    Vec<rustybuzz::Feature>,
);

const MAX_PLANS_PER_FACE: usize = 64;

pub struct FontFace {
    /// Parsed shaping tables (GSUB/GPOS lookup accelerators) borrowing
    /// `bytes`. Parsing them costs far more than shaping a short run, so it
    /// happens once per face. Declared before `bytes` so it drops first.
    shaping: std::sync::OnceLock<rustybuzz::Face<'static>>,
    /// Shape plans for the default instance, keyed by segment properties
    /// and user features.
    plans: Mutex<Vec<(PlanKey, Arc<rustybuzz::ShapePlan>)>>,
    pub name: String,
    pub index: u32,
    pub weight: u16,
    pub italic: bool,
    pub default_instance_only: bool,
    bytes: Arc<[u8]>,
    raster: Option<fontdue::Font>,
    color: bool,
    scaler: Mutex<swash::scale::ScaleContext>,
    scaler_key: swash::CacheKey,
    bitmaps: Mutex<HashMap<BitmapKey, Arc<GlyphBitmap>>>,
    glyph_count: u16,
    axes: Vec<rustybuzz::ttf_parser::VariationAxis>,
    units_per_em: f32,
    x_height: Option<i16>,
    cap_height: Option<i16>,
}

pub struct GlyphBitmap {
    pub metrics: fontdue::Metrics,
    pub pixels: Vec<u8>,
    pub color: bool,
}
type BitmapKey = (u16, u32, Vec<(u32, u32)>);

#[derive(Clone, Copy, Debug)]
pub(crate) struct FontUnits {
    pub ch: f32,
    pub ex: f32,
    pub cap: f32,
    pub ic: f32,
}

impl fmt::Debug for FontFace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontFace")
            .field("name", &self.name)
            .field("index", &self.index)
            .field("weight", &self.weight)
            .field("italic", &self.italic)
            .field("default_instance_only", &self.default_instance_only)
            .finish()
    }
}

impl PartialEq for FontFace {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.bytes == other.bytes
    }
}

impl Deref for FontFace {
    type Target = fontdue::Font;
    fn deref(&self) -> &Self::Target {
        self.raster.as_ref().unwrap_or_else(|| {
            static FALLBACK: OnceLock<fontdue::Font> = OnceLock::new();
            FALLBACK.get_or_init(|| {
                fontdue::Font::from_bytes(BUNDLED, fontdue::FontSettings::default())
                    .expect("bundled raster")
            })
        })
    }
}

impl FontFace {
    pub(crate) fn format_diagnostics(&self) -> Vec<String> {
        let face = self.shaping();
        let mut messages = Vec::new();
        if let Some(colr) = face
            .raw_face()
            .table(rustybuzz::ttf_parser::Tag::from_bytes(b"COLR"))
            && colr.get(..2).is_some_and(|b| b != [0, 0])
        {
            messages.push("COLR v1 paint graphs are unsupported; only COLR v0 layers or an available outline are rendered".into());
        }
        if let Some(colr) = face
            .raw_face()
            .table(rustybuzz::ttf_parser::Tag::from_bytes(b"COLR"))
            && colr.get(..2) == Some(&[0, 0])
        {
            let layers = colr
                .get(8..12)
                .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                .map(u32::from_be_bytes)
                .unwrap_or(0) as usize;
            let count = colr
                .get(12..14)
                .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
                .map(u16::from_be_bytes)
                .unwrap_or(0) as usize;
            if (0..count)
                .any(|i| colr.get(layers + i * 4 + 2..layers + i * 4 + 4) == Some(&[255, 255]))
            {
                messages.push("COLR foreground-palette entries are unsupported; fixed palette entries render, foreground entries use black".into());
            }
        }
        if face.tables().svg.is_some() {
            messages.push("OpenType SVG glyph documents are unsupported and never executed; available outline/bitmap fallback used".into());
        }
        if face.tables().cbdt.is_some() || face.tables().sbix.is_some() {
            for glyph in 0..face.number_of_glyphs() {
                if face
                    .glyph_raster_image(rustybuzz::ttf_parser::GlyphId(glyph), 20)
                    .is_some_and(|image| {
                        image.format != rustybuzz::ttf_parser::RasterImageFormat::PNG
                    })
                {
                    messages.push("bitmap font strike encoding is unsupported; only bounded PNG CBDT/sbix strikes are decoded".into());
                    break;
                }
            }
        }
        messages
    }
    pub(crate) fn has_source(&self, bytes: &Arc<[u8]>) -> bool {
        Arc::ptr_eq(&self.bytes, bytes)
    }
    pub(crate) fn units(&self, size: f32) -> FontUnits {
        let scale = size / self.units_per_em;
        FontUnits {
            ch: if self.lookup_glyph_index('0') == 0 {
                size * 0.5
            } else {
                self.metrics('0', size).advance_width
            },
            ex: self.x_height.map_or_else(
                || {
                    if self.lookup_glyph_index('x') == 0 {
                        size * 0.5
                    } else {
                        self.metrics('x', size).bounds.height
                    }
                },
                |h| h as f32 * scale,
            ),
            cap: self.cap_height.map_or_else(
                || {
                    if self.lookup_glyph_index('H') == 0 {
                        self.horizontal_line_metrics(size)
                            .map_or(size, |m| m.ascent)
                    } else {
                        self.metrics('H', size).bounds.height
                    }
                },
                |h| h as f32 * scale,
            ),
            ic: if self.lookup_glyph_index('水') == 0 {
                size
            } else {
                self.metrics('水', size).advance_width
            },
        }
    }

    pub(crate) fn units_for_style(&self, style: &ComputedStyle) -> FontUnits {
        let mut units = self.units(style.font_size);
        let coordinates = self.coordinates(style);
        let varied = coordinates.iter().any(|&(tag, value)| {
            self.axes
                .iter()
                .find(|axis| axis.tag.to_bytes() == tag)
                .is_some_and(|axis| axis.def_value != value)
        });
        if !varied && !style.writing_mode.vertical() {
            return units;
        }
        let mut instance = self.shaper();
        instance.set_variations(
            &coordinates
                .iter()
                .map(|&(tag, value)| rustybuzz::Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(&tag),
                    value,
                })
                .collect::<Vec<_>>(),
        );
        if instance.is_variable() {
            let scale = style.font_size / self.units_per_em;
            units.ch = instance
                .glyph_index('0')
                .and_then(|g| instance.glyph_hor_advance(g))
                .map_or(units.ch, |a| a as f32 * scale);
            units.ex = instance.x_height().map_or(units.ex, |h| h as f32 * scale);
            units.cap = instance
                .capital_height()
                .map_or(units.cap, |h| h as f32 * scale);
        }
        if style.writing_mode.vertical() {
            let advance = |c| {
                instance
                    .glyph_index(c)
                    .and_then(|id| instance.glyph_ver_advance(id))
                    .map_or(style.font_size, |a| {
                        a as f32 * style.font_size / self.units_per_em
                    })
            };
            units.ic = advance('水');
            if style.text_orientation == crate::style::TextOrientation::Upright {
                units.ch = advance('0');
            }
        }
        units
    }

    fn load(
        bytes: Arc<[u8]>,
        index: u32,
        name: String,
        weight: u16,
        italic: bool,
    ) -> Result<Self, String> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(format!("font exceeds {MAX_FONT_BYTES}-byte limit"));
        }
        let face = rustybuzz::Face::from_slice(&bytes, index).ok_or("invalid OpenType face")?;
        let default_instance_only = false;
        let units_per_em = face.units_per_em() as f32;
        let x_height = face.x_height();
        let cap_height = face.capital_height();
        let glyph_count = face.number_of_glyphs();
        let axes = face.variation_axes().into_iter().collect();
        let color = face.tables().colr.is_some()
            || face.tables().cbdt.is_some()
            || face.tables().sbix.is_some();
        if face.tables().glyf.is_none()
            && face.tables().cff.is_none()
            && face.tables().cff2.is_none()
            && !color
        {
            return Err(
                "unsupported raster format (SVG-only font; font SVG execution is disabled)".into(),
            );
        }
        let raster = face
            .tables()
            .cff2
            .is_none()
            .then(|| {
                fontdue::Font::from_bytes(
                    bytes.as_ref(),
                    fontdue::FontSettings {
                        collection_index: index,
                        ..fontdue::FontSettings::default()
                    },
                )
                .ok()
            })
            .flatten();
        Ok(Self {
            shaping: std::sync::OnceLock::new(),
            plans: Mutex::new(Vec::new()),
            name,
            index,
            weight,
            italic,
            default_instance_only,
            bytes,
            raster,
            color,
            scaler: Mutex::new(swash::scale::ScaleContext::new()),
            scaler_key: swash::CacheKey::new(),
            bitmaps: Mutex::new(HashMap::new()),
            glyph_count,
            axes,
            units_per_em,
            x_height,
            cap_height,
        })
    }

    /// An owned shaping face, for callers that set variations.
    pub fn shaper(&self) -> rustybuzz::Face<'_> {
        self.shaping().clone()
    }

    /// The shared default-instance shaping face.
    pub fn shaping(&self) -> &rustybuzz::Face<'_> {
        self.shaping.get_or_init(|| {
            // SAFETY: `bytes` is an immutable `Arc` allocation owned by `self`
            // whose address never changes, and `shaping` drops before it. The
            // `'static` face is only handed out shortened to `&self`'s lifetime.
            let data: &'static [u8] = unsafe { &*std::ptr::from_ref::<[u8]>(&self.bytes) };
            // Validated once on loading; the immutable bytes and index never change.
            rustybuzz::Face::from_slice(data, self.index).expect("validated font face")
        })
    }

    fn plan(
        &self,
        direction: rustybuzz::Direction,
        script: Option<rustybuzz::Script>,
        language: Option<rustybuzz::Language>,
        features: &[rustybuzz::Feature],
    ) -> Arc<rustybuzz::ShapePlan> {
        let mut plans = self.plans.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, plan)) = plans.iter().find(|((d, s, l, f), _)| {
            *d == direction && *s == script && *l == language && f.as_slice() == features
        }) {
            return plan.clone();
        }
        let plan = Arc::new(rustybuzz::ShapePlan::new(
            self.shaping(),
            direction,
            script,
            language.as_ref(),
            features,
        ));
        if plans.len() >= MAX_PLANS_PER_FACE {
            plans.clear();
        }
        plans.push((
            (direction, script, language, features.to_vec()),
            plan.clone(),
        ));
        plan
    }

    fn covers(&self, text: &str) -> bool {
        text.chars()
            .all(|c| invisible(c) || self.lookup_glyph_index(c) != 0)
    }

    pub fn rasterized(&self, id: u16, size: f32) -> Option<Arc<GlyphBitmap>> {
        self.instance_bitmap(id, size, &[])
    }

    pub fn lookup_glyph_index(&self, c: char) -> u16 {
        self.raster.as_ref().map_or_else(
            || self.shaping().glyph_index(c).map_or(0, |g| g.0),
            |raster| raster.lookup_glyph_index(c),
        )
    }

    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    pub fn metrics(&self, c: char, size: f32) -> fontdue::Metrics {
        self.metrics_indexed(self.lookup_glyph_index(c), size)
    }

    pub fn metrics_indexed(&self, id: u16, size: f32) -> fontdue::Metrics {
        if let Some(raster) = &self.raster {
            raster.metrics_indexed(id, size)
        } else {
            self.rasterized(id, size)
                .map_or_else(fontdue::Metrics::default, |b| b.metrics)
        }
    }

    pub fn horizontal_line_metrics(&self, size: f32) -> Option<fontdue::LineMetrics> {
        if let Some(raster) = &self.raster {
            return raster.horizontal_line_metrics(size);
        }
        let face = self.shaping();
        let scale = size / self.units_per_em;
        let ascent = face.ascender() as f32 * scale;
        let descent = face.descender() as f32 * scale;
        let line_gap = face.line_gap() as f32 * scale;
        Some(fontdue::LineMetrics {
            ascent,
            descent,
            line_gap,
            new_line_size: ascent - descent + line_gap,
        })
    }

    pub(crate) fn coordinates(&self, style: &ComputedStyle) -> Vec<([u8; 4], f32)> {
        use crate::style::typography::{Keyword, Property, Value};
        let explicit = style.typography.settings(Property::FontVariations);
        let mut coordinates: Vec<_> = self
            .axes
            .iter()
            .map(|axis| {
                let tag = axis.tag.to_bytes();
                let value = explicit.get(&tag).copied().unwrap_or_else(|| match &tag {
                    b"wght" => style.font_weight.0,
                    b"wdth" => match style.typography.get(Property::FontWidth) {
                        Value::Width(w) => w,
                        _ => 100.0,
                    },
                    b"ital" => f32::from(style.italic),
                    b"slnt" if style.font_style == FontStyle::Oblique => -14.0,
                    b"opsz"
                        if style.typography.keyword(Property::FontOpticalSizing)
                            == Keyword::Auto =>
                    {
                        style.font_size * 0.75
                    }
                    _ => axis.def_value,
                });
                (tag, value.clamp(axis.min_value, axis.max_value))
            })
            .collect();
        coordinates.sort_by_key(|&(tag, _)| tag);
        coordinates
    }

    pub fn instance_bitmap(
        &self,
        id: u16,
        size: f32,
        coordinates: &[([u8; 4], f32)],
    ) -> Option<Arc<GlyphBitmap>> {
        if !size.is_finite()
            || !(0.0..=2048.0).contains(&size)
            || id >= self.glyph_count()
            || coordinates.len() > 64
            || coordinates.iter().any(|&(_, value)| !value.is_finite())
        {
            return None;
        }
        let mut cache_coordinates: Vec<_> = coordinates
            .iter()
            .map(|(tag, value)| (u32::from_be_bytes(*tag), value.to_bits()))
            .collect();
        cache_coordinates.sort_unstable();
        let key = (id, size.to_bits(), cache_coordinates);
        let mut cache = self.bitmaps.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bitmap) = cache.get(&key) {
            return Some(bitmap.clone());
        }
        let varied = coordinates.iter().any(|&(tag, value)| {
            self.axes
                .iter()
                .find(|a| a.tag.to_bytes() == tag)
                .is_some_and(|a| a.def_value != value)
        });
        let mut bounded = self.shaper();
        bounded.set_variations(
            &coordinates
                .iter()
                .map(|&(tag, value)| rustybuzz::Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(&tag),
                    value,
                })
                .collect::<Vec<_>>(),
        );
        if let Some(boxes) = bounded.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(id)) {
            let scale = size / self.units_per_em;
            let width = (i32::from(boxes.x_max) - i32::from(boxes.x_min)) as f32 * scale;
            let height = (i32::from(boxes.y_max) - i32::from(boxes.y_min)) as f32 * scale;
            if width.ceil() * height.ceil() > 1_048_576.0 {
                return None;
            }
        }
        let colr_v0 = bounded
            .raw_face()
            .table(rustybuzz::ttf_parser::Tag::from_bytes(b"COLR"))
            .is_some_and(|table| table.get(..2) == Some(&[0, 0]));
        let bitmap = if colr_v0 && bounded.is_color_glyph(rustybuzz::ttf_parser::GlyphId(id)) {
            Arc::new(self.colr_bitmap(id, size, coordinates, &bounded)?)
        } else if let Some(image) =
            bounded.glyph_raster_image(rustybuzz::ttf_parser::GlyphId(id), size.ceil() as u16)
        {
            Arc::new(self.png_bitmap(id, size, &bounded, image)?)
        } else if !self.color
            && !varied
            && let Some(raster) = &self.raster
        {
            let metrics = raster.metrics_indexed(id, size);
            if metrics
                .width
                .checked_mul(metrics.height)
                .is_none_or(|n| n > 1_048_576)
            {
                return None;
            }
            let (metrics, pixels) = raster.rasterize_indexed(id, size);
            Arc::new(GlyphBitmap {
                metrics,
                pixels,
                color: false,
            })
        } else {
            let mut font = swash::FontRef::from_index(&self.bytes, self.index as usize)?;
            font.key = self.scaler_key;
            let mut context = self.scaler.lock().unwrap_or_else(|e| e.into_inner());
            let mut scaler = context
                .builder(font)
                .size(size)
                .variations(
                    coordinates
                        .iter()
                        .map(|&(tag, value)| (u32::from_be_bytes(tag), value)),
                )
                .build();
            let outline = scaler.scale_outline(id)?;
            let bounds = outline.bounds();
            if !bounds.width().is_finite()
                || !bounds.height().is_finite()
                || (bounds.width().ceil() + 2.0) * (bounds.height().ceil() + 2.0) > 1_048_576.0
            {
                return None;
            }
            let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                swash::scale::Render::new(&[swash::scale::Source::Outline])
                    .format(swash::zeno::Format::Alpha)
                    .render(&mut scaler, id)
            }))
            .ok()??;
            let p = image.placement;
            if p.width.checked_mul(p.height).is_none_or(|n| n > 1_048_576) {
                return None;
            }
            let metrics = fontdue::Metrics {
                xmin: p.left,
                ymin: p.top - p.height as i32,
                width: p.width as usize,
                height: p.height as usize,
                advance_width: bounded
                    .glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(id))
                    .unwrap_or(0) as f32
                    * size
                    / self.units_per_em,
                advance_height: 0.0,
                ..fontdue::Metrics::default()
            };
            Arc::new(GlyphBitmap {
                metrics,
                pixels: image.data,
                color: image.content == swash::scale::image::Content::Color,
            })
        };
        if cache.len() >= 1024
            || cache.values().map(|b| b.pixels.len()).sum::<usize>() + bitmap.pixels.len()
                > 2_097_152
        {
            cache.clear();
        }
        if bitmap.pixels.len() <= 2_097_152 {
            cache.insert(key, bitmap.clone());
        }
        Some(bitmap)
    }

    fn png_bitmap(
        &self,
        id: u16,
        size: f32,
        face: &rustybuzz::Face<'_>,
        image: rustybuzz::ttf_parser::RasterGlyphImage<'_>,
    ) -> Option<GlyphBitmap> {
        if image.format != rustybuzz::ttf_parser::RasterImageFormat::PNG
            || image.data.len() > 4_194_304
            || image.pixels_per_em == 0
            || image.data.get(..8)? != b"\x89PNG\r\n\x1a\n"
        {
            return None;
        }
        let w = u32::from_be_bytes(image.data.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(image.data.get(20..24)?.try_into().ok()?);
        if w == 0 || h == 0 || w.checked_mul(h).is_none_or(|area| area > 1_048_576) {
            return None;
        }
        let ratio = size / f32::from(image.pixels_per_em);
        let width = (w as f32 * ratio).ceil() as usize;
        let height = (h as f32 * ratio).ceil() as usize;
        let area = width.checked_mul(height)?;
        if area > 1_048_576 {
            return None;
        }
        let decoded = crate::media::DecodedImage::decode(image.data).ok()?;
        if decoded.width() != w || decoded.height() != h {
            return None;
        }
        let mut pixels = vec![0u8; area.checked_mul(4)?];
        for y in 0..height {
            for x in 0..width {
                let sx = ((x as f32 / ratio) as u32).min(w.saturating_sub(1));
                let sy = ((y as f32 / ratio) as u32).min(h.saturating_sub(1));
                pixels[(y * width + x) * 4..][..4]
                    .copy_from_slice(&decoded.rgba()[((sy * w + sx) * 4) as usize..][..4]);
            }
        }
        Some(GlyphBitmap {
            metrics: fontdue::Metrics {
                xmin: (f32::from(image.x) * ratio).floor() as i32,
                ymin: (f32::from(image.y) * ratio).floor() as i32,
                width,
                height,
                advance_width: face.glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(id))? as f32
                    * size
                    / self.units_per_em,
                ..Default::default()
            },
            pixels,
            color: true,
        })
    }

    fn colr_bitmap(
        &self,
        id: u16,
        size: f32,
        coordinates: &[([u8; 4], f32)],
        face: &rustybuzz::Face<'_>,
    ) -> Option<GlyphBitmap> {
        let table = face
            .raw_face()
            .table(rustybuzz::ttf_parser::Tag::from_bytes(b"COLR"))?;
        let word = |data: &[u8], at: usize| {
            Some(u16::from_be_bytes(
                data.get(at..at.checked_add(2)?)?.try_into().ok()?,
            ))
        };
        let long = |data: &[u8], at: usize| {
            Some(u32::from_be_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?) as usize)
        };
        if word(table, 0)? != 0 {
            return None;
        }
        let records = long(table, 4)?;
        let layers = long(table, 8)?;
        let count = usize::from(word(table, 2)?);
        let record = (0..count)
            .map(|i| records + i * 6)
            .find(|&at| word(table, at) == Some(id))?;
        let first = usize::from(word(table, record + 2)?);
        let count = usize::from(word(table, record + 4)?);
        if count == 0 || count > 256 {
            return None;
        }
        let palette = face
            .raw_face()
            .table(rustybuzz::ttf_parser::Tag::from_bytes(b"CPAL"))?;
        let colors = long(palette, 8)?;
        let palette_first = usize::from(word(palette, 12)?);
        let mut commands = Vec::new();
        let mut bounds: Option<(i32, i32, i32, i32)> = None;
        for i in 0..count {
            let at = layers.checked_add((first + i).checked_mul(4)?)?;
            let glyph = word(table, at)?;
            let color = word(table, at + 2)?;
            let color = if color == u16::MAX {
                [0, 0, 0, 255]
            } else {
                if color >= word(palette, 2)? {
                    return None;
                }
                let at =
                    colors.checked_add((palette_first + usize::from(color)).checked_mul(4)?)?;
                let bgra = palette.get(at..at + 4)?;
                [bgra[2], bgra[1], bgra[0], bgra[3]]
            };
            let bbox = face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(glyph))?;
            let scale = size / self.units_per_em;
            let b = (
                (f32::from(bbox.x_min) * scale).floor() as i32,
                (f32::from(bbox.y_min) * scale).floor() as i32,
                (f32::from(bbox.x_max) * scale).ceil() as i32,
                (f32::from(bbox.y_max) * scale).ceil() as i32,
            );
            bounds = Some(bounds.map_or(b, |p| {
                (p.0.min(b.0), p.1.min(b.1), p.2.max(b.2), p.3.max(b.3))
            }));
            commands.push((glyph, color));
        }
        let (left, bottom, right, top) = bounds?;
        let width = usize::try_from(right.checked_sub(left)?).ok()?;
        let height = usize::try_from(top.checked_sub(bottom)?).ok()?;
        let area = width.checked_mul(height)?;
        if area > 1_048_576 {
            return None;
        }
        let mut pixels = vec![0u8; area.checked_mul(4)?];
        let mut font = swash::FontRef::from_index(&self.bytes, self.index as usize)?;
        font.key = self.scaler_key;
        let mut context = self.scaler.lock().unwrap_or_else(|e| e.into_inner());
        let mut scaler = context
            .builder(font)
            .size(size)
            .variations(
                coordinates
                    .iter()
                    .map(|&(tag, value)| (u32::from_be_bytes(tag), value)),
            )
            .build();
        for (glyph, color) in commands {
            let image = swash::scale::Render::new(&[swash::scale::Source::Outline])
                .format(swash::zeno::Format::Alpha)
                .render(&mut scaler, glyph)?;
            for y in 0..image.placement.height as usize {
                for x in 0..image.placement.width as usize {
                    let px = image.placement.left - left + x as i32;
                    let py = top - image.placement.top + y as i32;
                    if px < 0 || py < 0 || px as usize >= width || py as usize >= height {
                        continue;
                    }
                    let alpha = (u32::from(image.data[y * image.placement.width as usize + x])
                        * u32::from(color[3])
                        + 127)
                        / 255;
                    let p = &mut pixels[(py as usize * width + px as usize) * 4..][..4];
                    let old = u32::from(p[3]);
                    let combined = alpha + (old * (255 - alpha) + 127) / 255;
                    if combined == 0 {
                        continue;
                    }
                    for c in 0..3 {
                        p[c] = ((u32::from(color[c]) * alpha * 255
                            + u32::from(p[c]) * old * (255 - alpha)
                            + combined * 127)
                            / (combined * 255)) as u8;
                    }
                    p[3] = combined as u8;
                }
            }
        }
        Some(GlyphBitmap {
            metrics: fontdue::Metrics {
                xmin: left,
                ymin: bottom,
                width,
                height,
                advance_width: face.glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(id))? as f32
                    * size
                    / self.units_per_em,
                ..Default::default()
            },
            pixels,
            color: true,
        })
    }
}

pub fn bundled_face() -> Arc<FontFace> {
    static FACE: OnceLock<Arc<FontFace>> = OnceLock::new();
    FACE.get_or_init(|| {
        Arc::new(
            FontFace::load(Arc::from(BUNDLED), 0, "Noto Sans".into(), 400, false)
                .expect("licensed bundled Noto Sans must be valid"),
        )
    })
    .clone()
}

/// The font-selection inputs of a style, interned to a small id so that
/// selection and run caches never format or clone style data per lookup.
struct FontKey {
    family: crate::css::values::FontFamily,
    weight: u32,
    style: FontStyle,
    typography: crate::style::typography::Typography,
}

impl FontKey {
    /// Typography is deliberately not hashed: an explicit initial value
    /// equals an absent one, so it is only compared within a bucket.
    fn hash(style: &ComputedStyle) -> u64 {
        let mut hasher = FastHasher::default();
        style.font_family.hash(&mut hasher);
        style.font_weight.0.to_bits().hash(&mut hasher);
        style.font_style.hash(&mut hasher);
        hasher.finish()
    }

    fn matches(&self, style: &ComputedStyle) -> bool {
        self.weight == style.font_weight.0.to_bits()
            && self.style == style.font_style
            && self.family == style.font_family
            && self.typography == style.typography
    }
}

/// Owned shaping-cache key; built only when a run is inserted.
struct RunKey {
    font: u32,
    size: u32,
    language: Option<String>,
    bold: bool,
    italic: bool,
    writing_mode: crate::style::WritingMode,
    text_orientation: crate::style::TextOrientation,
    line_height: Option<u32>,
    rtl: Option<bool>,
    boundaries: Vec<usize>,
    before: String,
    after: String,
    text: String,
}

impl RunKey {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.language.as_ref().map_or(0, String::len)
            + self.boundaries.len() * std::mem::size_of::<usize>()
            + self.before.len()
            + self.after.len()
            + self.text.len()
    }
}

/// Borrowed view of a [`RunKey`] so cache hits allocate nothing.
#[derive(Hash)]
struct RunQuery<'a> {
    font: u32,
    size: u32,
    language: Option<&'a str>,
    bold: bool,
    italic: bool,
    writing_mode: crate::style::WritingMode,
    text_orientation: crate::style::TextOrientation,
    line_height: Option<u32>,
    rtl: Option<bool>,
    boundaries: &'a [usize],
    before: &'a str,
    after: &'a str,
    text: &'a str,
}

impl RunQuery<'_> {
    fn hash(&self) -> u64 {
        let mut hasher = FastHasher::default();
        Hash::hash(self, &mut hasher);
        hasher.finish()
    }

    fn matches(&self, key: &RunKey) -> bool {
        self.font == key.font
            && self.size == key.size
            && self.language == key.language.as_deref()
            && self.bold == key.bold
            && self.italic == key.italic
            && self.writing_mode == key.writing_mode
            && self.text_orientation == key.text_orientation
            && self.line_height == key.line_height
            && self.rtl == key.rtl
            && self.boundaries == key.boundaries.as_slice()
            && self.before == key.before
            && self.after == key.after
            && self.text == key.text
    }

    fn to_key(&self) -> RunKey {
        RunKey {
            font: self.font,
            size: self.size,
            language: self.language.map(str::to_owned),
            bold: self.bold,
            italic: self.italic,
            writing_mode: self.writing_mode,
            text_orientation: self.text_orientation,
            line_height: self.line_height,
            rtl: self.rtl,
            boundaries: self.boundaries.to_vec(),
            before: self.before.to_owned(),
            after: self.after.to_owned(),
            text: self.text.to_owned(),
        }
    }
}

type RunBuckets = HashMap<u64, Vec<(RunKey, Arc<ShapedText>)>>;

#[derive(Default)]
struct Cache {
    faces: HashMap<fontdb::ID, Option<Arc<FontFace>>>,
    font_keys: HashMap<u64, Vec<(FontKey, u32)>>,
    font_key_count: u32,
    requests: HashMap<u32, Arc<FontFace>>,
    sources: HashMap<String, Arc<[u8]>>,
    bytes: usize,
    glyphs: usize,
    messages: Vec<String>,
    runs: RunBuckets,
    run_count: usize,
    run_bytes: usize,
    previous_runs: RunBuckets,
    previous_run_count: usize,
    missing: Vec<String>,
}

struct FontSystem {
    database: Database,
    bundled: Arc<FontFace>,
    cache: Mutex<Cache>,
    report_missing: bool,
    parent: Option<FontContext>,
    author: Vec<AuthorFont>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AuthorFont {
    pub descriptor: crate::css::font_face::FontDescriptor,
    pub face: Arc<FontFace>,
}

/// Clones share discovery and bounded caches. `bundled()` never reads OS fonts;
/// `system()` discovers installed fonts once for the browsing process.
#[derive(Clone)]
pub struct FontContext(Arc<FontSystem>);

impl fmt::Debug for FontContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontContext")
            .field("discovered_faces", &self.0.database.len())
            .finish()
    }
}

impl Deref for FontContext {
    type Target = fontdue::Font;
    fn deref(&self) -> &Self::Target {
        &self.0.bundled
    }
}

impl Default for FontContext {
    fn default() -> Self {
        Self::bundled()
    }
}

impl FontContext {
    pub fn bundled() -> Self {
        static CONTEXT: OnceLock<FontContext> = OnceLock::new();
        CONTEXT
            .get_or_init(|| Self::with_database(Database::new(), false))
            .clone()
    }

    pub fn system() -> Self {
        static CONTEXT: OnceLock<FontContext> = OnceLock::new();
        CONTEXT
            .get_or_init(|| {
                let mut database = Database::new();
                database.load_system_fonts();
                for (generic, candidates) in [
                    (
                        Family::SansSerif,
                        &["Arial", "Noto Sans", "DejaVu Sans", "Liberation Sans"][..],
                    ),
                    (
                        Family::Serif,
                        &[
                            "Times New Roman",
                            "Noto Serif",
                            "DejaVu Serif",
                            "Liberation Serif",
                        ][..],
                    ),
                    (
                        Family::Monospace,
                        &["Menlo", "Consolas", "DejaVu Sans Mono", "Liberation Mono"][..],
                    ),
                ] {
                    if database
                        .query(&Query {
                            families: &[generic],
                            ..Query::default()
                        })
                        .is_none()
                        && let Some(name) = candidates.iter().find(|name| {
                            database
                                .query(&Query {
                                    families: &[Family::Name(name)],
                                    ..Query::default()
                                })
                                .is_some()
                        })
                    {
                        match generic {
                            Family::SansSerif => database.set_sans_serif_family(*name),
                            Family::Serif => database.set_serif_family(*name),
                            Family::Monospace => database.set_monospace_family(*name),
                            _ => {}
                        }
                    }
                }
                Self::from_database(database)
            })
            .clone()
    }

    /// Inject licensed font resources and generic-family mappings for tests or
    /// embedding. Sources must be bounded; file data is loaded lazily.
    pub fn from_database(database: Database) -> Self {
        Self::with_database(database, true)
    }

    fn with_database(database: Database, report_missing: bool) -> Self {
        Self(Arc::new(FontSystem {
            database,
            bundled: bundled_face(),
            cache: Mutex::new(Cache::default()),
            report_missing,
            parent: None,
            author: Vec::new(),
        }))
    }

    pub(crate) fn with_author_fonts(&self, author: Vec<AuthorFont>) -> Self {
        if author.len() == self.0.author.len()
            && author
                .iter()
                .zip(&self.0.author)
                .all(|(a, b)| a.descriptor == b.descriptor && Arc::ptr_eq(&a.face, &b.face))
        {
            return self.clone();
        }
        let base = self.0.parent.as_ref().unwrap_or(self);
        if author.is_empty() {
            return base.clone();
        }
        Self(Arc::new(FontSystem {
            database: Database::new(),
            bundled: base.0.bundled.clone(),
            cache: Mutex::new(Cache::default()),
            report_missing: base.0.report_missing,
            parent: Some(base.clone()),
            author,
        }))
    }

    /// Identical discovery, author faces and caches.
    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn same_discovery(&self, other: &Self) -> bool {
        Arc::ptr_eq(
            &self.0.parent.as_ref().unwrap_or(self).0,
            &other.0.parent.as_ref().unwrap_or(other).0,
        )
    }

    /// Exact full-font or PostScript name, never a family query that could
    /// silently resolve a different weight or collection index.
    pub(crate) fn local(&self, name: &str) -> Option<Arc<FontFace>> {
        fn matches(face: &FontFace, name: &str) -> bool {
            face.shaping()
                .names()
                .into_iter()
                .filter(|n| matches!(n.name_id, 4 | 6))
                .filter_map(|n| n.to_string())
                .any(|s| s.eq_ignore_ascii_case(name))
        }
        if let Some(parent) = &self.0.parent {
            return parent.local(name);
        }
        if matches(&self.0.bundled, name) {
            return Some(self.0.bundled.clone());
        }
        let mut probes = 0;
        let mut source_bytes = 0;
        for info in self.0.database.faces().take(4096) {
            if !(info.post_script_name.eq_ignore_ascii_case(name)
                || info.families.iter().any(|f| {
                    name.to_ascii_lowercase()
                        .starts_with(&f.0.to_ascii_lowercase())
                }))
            {
                continue;
            }
            let size = match self.0.database.face_source(info.id)?.0 {
                Source::Binary(bytes) => bytes.as_ref().as_ref().len(),
                Source::File(path) | Source::SharedFile(path, _) => {
                    let Ok(metadata) = fs::metadata(path) else {
                        continue;
                    };
                    usize::try_from(metadata.len()).ok()?
                }
            };
            if size > MAX_FONT_BYTES {
                continue;
            }
            if probes >= 128 || source_bytes + size > MAX_SOURCE_BYTES {
                break;
            }
            probes += 1;
            source_bytes += size;
            if self.0.database.with_face_data(info.id, |bytes, index| {
                bytes.len() <= MAX_FONT_BYTES
                    && rustybuzz::Face::from_slice(bytes, index).is_some_and(|face| {
                        face.names()
                            .into_iter()
                            .filter(|n| matches!(n.name_id, 4 | 6))
                            .filter_map(|n| n.to_string())
                            .any(|n| n.eq_ignore_ascii_case(name))
                    })
            }) == Some(true)
                && let Some(face) = self.load(info.id)
            {
                return Some(face);
            }
        }
        None
    }

    pub(crate) fn downloadable(bytes: Arc<[u8]>) -> Result<Arc<FontFace>, String> {
        let face = rustybuzz::Face::from_slice(&bytes, 0).ok_or("invalid OpenType font")?;
        let name = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == 4)
            .find_map(|n| n.to_string())
            .unwrap_or_else(|| "web font".into());
        let weight = face.weight().to_number();
        let italic = face.is_italic() || face.is_oblique();
        if face.number_of_glyphs() as usize > MAX_GLYPHS {
            return Err("font glyph limit".into());
        }
        std::panic::catch_unwind(|| FontFace::load(bytes, 0, name, weight, italic))
            .map_err(|_| "font parser rejected malformed outlines".to_string())?
            .map(Arc::new)
    }

    fn report(cache: &mut Cache, message: String) {
        if cache.messages.len() < MAX_REQUESTS && !cache.messages.contains(&message) {
            eprintln!("CSS font: {message}");
            cache.messages.push(message);
        }
    }

    pub fn diagnostics(&self) -> Vec<String> {
        self.0
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .messages
            .clone()
    }

    fn load(&self, id: fontdb::ID) -> Option<Arc<FontFace>> {
        let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(face) = cache.faces.get(&id) {
            return face.clone();
        }
        let info = self.0.database.face(id)?;
        if cache.faces.len() >= MAX_FACES {
            Self::report(
                &mut cache,
                format!("font face cache limit ({MAX_FACES}); using loaded/bundled fallback"),
            );
            return None;
        }
        let loaded = (|| {
            let (source, index) = self
                .0
                .database
                .face_source(id)
                .ok_or("missing font source")?;
            let bytes: Arc<[u8]> = match source {
                Source::Binary(data) => {
                    if data.as_ref().as_ref().len() > MAX_FONT_BYTES {
                        return Err("font byte limit".into());
                    }
                    if cache.bytes + data.as_ref().as_ref().len() > MAX_SOURCE_BYTES {
                        return Err("font source cache byte limit".into());
                    }
                    cache.bytes += data.as_ref().as_ref().len();
                    Arc::from(data.as_ref().as_ref())
                }
                Source::File(path) | Source::SharedFile(path, _) => {
                    let key = path.to_string_lossy().into_owned();
                    if let Some(bytes) = cache.sources.get(&key) {
                        bytes.clone()
                    } else {
                        let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
                        if size > MAX_FONT_BYTES as u64 {
                            return Err("font byte limit".into());
                        }
                        // Bound even a file that changes between metadata and read.
                        use std::io::Read;
                        let mut data = Vec::new();
                        fs::File::open(&path)
                            .map_err(|e| e.to_string())?
                            .take(MAX_FONT_BYTES as u64 + 1)
                            .read_to_end(&mut data)
                            .map_err(|e| e.to_string())?;
                        if data.len() > MAX_FONT_BYTES {
                            return Err("font byte limit".into());
                        }
                        let bytes: Arc<[u8]> = data.into();
                        if cache.bytes + bytes.len() > MAX_SOURCE_BYTES {
                            return Err("font source cache byte limit".into());
                        }
                        cache.bytes += bytes.len();
                        cache.sources.insert(key, bytes.clone());
                        bytes
                    }
                }
            };
            if cache.bytes > MAX_SOURCE_BYTES {
                return Err("font source cache byte limit".into());
            }
            let face = FontFace::load(
                bytes,
                index,
                info.families
                    .first()
                    .map_or("unnamed", |f| f.0.as_str())
                    .into(),
                info.weight.0,
                info.style != Style::Normal,
            )?;
            if cache.glyphs + face.glyph_count() as usize > MAX_GLYPHS {
                return Err("font glyph cache limit".into());
            }
            cache.glyphs += face.glyph_count() as usize;
            for message in face.format_diagnostics() {
                Self::report(&mut cache, format!("{}: {message}", face.name));
            }
            if face.default_instance_only {
                Self::report(
                    &mut cache,
                    format!(
                        "{}: variable font uses its default instance only; variation-axis selection is unsupported",
                        face.name
                    ),
                );
            }
            Ok::<_, String>(Arc::new(face))
        })();
        let face = match loaded {
            Ok(face) => Some(face),
            Err(error) => {
                Self::report(
                    &mut cache,
                    format!(
                        "{} face {}: {error}; using outline/bundled fallback",
                        info.post_script_name, info.index
                    ),
                );
                None
            }
        };
        cache.faces.insert(id, face.clone());
        face
    }

    fn query(&self, style: &ComputedStyle, family: Family<'_>) -> Option<Arc<FontFace>> {
        self.query_text(style, family, None)
    }

    fn query_text(
        &self,
        style: &ComputedStyle,
        family: Family<'_>,
        text: Option<&str>,
    ) -> Option<Arc<FontFace>> {
        if let Some(parent) = &self.0.parent {
            if let Family::Name(name) = family {
                let candidates = self
                    .0
                    .author
                    .iter()
                    .rev()
                    .filter(|f| f.descriptor.family.eq_ignore_ascii_case(name));
                let defined = candidates.clone().next().is_some();
                let selected = candidates
                    .filter(|f| {
                        // Metric selection respects declared ranges without
                        // requiring a sparse face to contain the probe glyph.
                        text.map_or_else(
                            || f.descriptor.covers("0"),
                            |t| f.descriptor.covers(t) && f.face.covers(t),
                        )
                    })
                    .min_by_key(|f| {
                        (
                            style_distance(style.font_style, f.descriptor.style),
                            weight_distance(
                                style.font_weight.0 as u16,
                                (style.font_weight.0 as u16).clamp(
                                    f.descriptor.weight_range.0,
                                    f.descriptor.weight_range.1,
                                ),
                            ),
                        )
                    })
                    .map(|f| f.face.clone());
                if defined {
                    return selected;
                }
            }
            return parent.query_text(style, family, text);
        }
        let selected = self
            .0
            .database
            .query(&Query {
                families: &[family],
                weight: Weight(style.font_weight.0.clamp(1.0, 1000.0) as u16),
                style: match style.font_style {
                    FontStyle::Normal => Style::Normal,
                    FontStyle::Italic => Style::Italic,
                    FontStyle::Oblique => Style::Oblique,
                },
                ..Query::default()
            })
            .and_then(|id| self.load(id));
        selected.or_else(|| match family {
            Family::Name(name) if name.eq_ignore_ascii_case(&self.0.bundled.name) => {
                Some(self.0.bundled.clone())
            }
            _ => None,
        })
    }

    fn family<'a>(name: &'a FamilyName) -> Family<'a> {
        match name {
            FamilyName::Named(name) => Family::Name(name),
            FamilyName::Generic(name) => match name.as_str() {
                "serif" | "ui-serif" | "fangsong" => Family::Serif,
                "monospace" | "ui-monospace" => Family::Monospace,
                "cursive" => Family::Cursive,
                "fantasy" => Family::Fantasy,
                _ => Family::SansSerif,
            },
        }
    }

    /// Interned id of `style`'s font-selection inputs, or `None` when they
    /// are too large to cache or the bounded table is full.
    fn font_key(&self, style: &ComputedStyle) -> Option<u32> {
        let bytes = style.font_family.0.iter().try_fold(0usize, |sum, family| {
            let (FamilyName::Named(name) | FamilyName::Generic(name)) = family;
            sum.checked_add(name.len() + 4)
        })?;
        if bytes > 4096 || style.font_language.as_ref().is_some_and(|s| s.len() > 64) {
            return None;
        }
        let hash = FontKey::hash(style);
        let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&(_, id)) = cache
            .font_keys
            .get(&hash)
            .and_then(|bucket| bucket.iter().find(|(key, _)| key.matches(style)))
        {
            return Some(id);
        }
        if cache.font_key_count as usize >= MAX_FONT_KEYS {
            return None;
        }
        let id = cache.font_key_count;
        cache.font_key_count += 1;
        cache.font_keys.entry(hash).or_default().push((
            FontKey {
                family: style.font_family.clone(),
                weight: style.font_weight.0.to_bits(),
                style: style.font_style,
                typography: style.typography.clone(),
            },
            id,
        ));
        Some(id)
    }

    pub fn select(&self, style: &ComputedStyle) -> Arc<FontFace> {
        let key = self.font_key(style);
        if let Some(key) = key
            && let Some(face) = self
                .0
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .requests
                .get(&key)
        {
            return face.clone();
        }
        let face = style
            .font_family
            .0
            .iter()
            .take(32)
            .find_map(|name| self.query_text(style, Self::family(name), None))
            .unwrap_or_else(|| self.0.bundled.clone());
        let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.requests.len() < MAX_REQUESTS
            && let Some(key) = key
        {
            cache.requests.insert(key, face.clone());
        }
        face
    }

    fn fallback(
        &self,
        style: &ComputedStyle,
        text: &str,
        primary: &Arc<FontFace>,
        remaining_probes: &mut usize,
    ) -> Arc<FontFace> {
        if self.0.parent.is_none() && primary.covers(text) {
            return primary.clone();
        }
        for name in style.font_family.0.iter().take(32) {
            if let Some(face) = self.query_text(style, Self::family(name), Some(text))
                && face.covers(text)
            {
                return face;
            }
            if let Some(parent) = &self.0.parent {
                let author_family = matches!(name, FamilyName::Named(name) if self.0.author.iter().any(|f|f.descriptor.family.eq_ignore_ascii_case(name)));
                if !author_family
                    && let Some(face) = parent.query_text(style, Self::family(name), Some(text))
                    && face.covers(text)
                {
                    return face;
                }
            }
        }
        if let Some(parent) = &self.0.parent {
            // Exhaust declared web families before installed fallback; the
            // parent's cache must never admit document-owned faces.
            let mut installed = style.clone();
            installed.font_family.0.retain(|family| {
                !matches!(family,FamilyName::Named(name)
                if self.0.author.iter().any(|f|f.descriptor.family.eq_ignore_ascii_case(name)))
            });
            return parent.fallback(
                &installed,
                text,
                &parent.select(&installed),
                remaining_probes,
            );
        }
        {
            let cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.missing.iter().any(|s| s == text) {
                return self.0.bundled.clone();
            }
            if let Some(face) = cache
                .faces
                .values()
                .flatten()
                .filter(|f| f.covers(text))
                .min_by_key(|f| {
                    (
                        f.weight.abs_diff(style.font_weight.0 as u16)
                            + if f.italic == style.italic { 0 } else { 1000 },
                        &f.name,
                        f.index,
                    )
                })
            {
                return face.clone();
            }
        }
        // Test cmap coverage before parsing every raster face. Discovery is
        // retained; no per-character directory scans or font-file reads.
        for info in self.0.database.faces().take(4096) {
            if *remaining_probes == 0 {
                let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
                Self::report(
                    &mut cache,
                    format!(
                        "font coverage probe limit ({MAX_COVERAGE_PROBES}) for this shaping operation; using loaded/bundled fallback"
                    ),
                );
                // A truncated search does not establish missing coverage.
                // Do not memoize it and poison subsequent shaping operations.
                return self.0.bundled.clone();
            }
            *remaining_probes -= 1;
            let source = self.0.database.face_source(info.id);
            let bounded = match &source {
                Some((Source::File(path) | Source::SharedFile(path, _), _)) => {
                    fs::metadata(path).is_ok_and(|m| m.len() <= MAX_FONT_BYTES as u64)
                }
                Some((Source::Binary(bytes), _)) => bytes.as_ref().as_ref().len() <= MAX_FONT_BYTES,
                None => false,
            };
            if bounded
                && self.0.database.with_face_data(info.id, |bytes, index| {
                    rustybuzz::Face::from_slice(bytes, index).is_some_and(|face| {
                        text.chars()
                            .all(|c| invisible(c) || face.glyph_index(c).is_some())
                    })
                }) == Some(true)
            {
                if let Some(name) = info.families.first()
                    && let Some(face) = self.query(style, Family::Name(&name.0))
                    && face.covers(text)
                {
                    return face;
                }
                if let Some(face) = self.load(info.id) {
                    return face;
                }
            }
        }
        if self.0.report_missing {
            let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.missing.len() < MAX_REQUESTS && text.len() <= 64 {
                cache.missing.push(text.into());
            }
            if let Some(c) = text
                .chars()
                .find(|&c| !invisible(c) && self.0.bundled.lookup_glyph_index(c) == 0)
            {
                Self::report(
                    &mut cache,
                    format!(
                        "no compatible outline fallback for U+{:04X}; bundled missing-glyph box used",
                        c as u32
                    ),
                );
            }
        }
        self.0.bundled.clone()
    }

    pub fn shape(&self, style: &ComputedStyle, text: &str) -> Arc<ShapedText> {
        self.shape_with_boundaries(style, text, &[])
    }

    pub(crate) fn shape_with_boundaries(
        &self,
        style: &ComputedStyle,
        text: &str,
        paint_boundaries: &[usize],
    ) -> Arc<ShapedText> {
        self.shape_with_context(style, text, paint_boundaries, "", "")
    }

    pub(crate) fn shape_with_context(
        &self,
        style: &ComputedStyle,
        text: &str,
        paint_boundaries: &[usize],
        before: &str,
        after: &str,
    ) -> Arc<ShapedText> {
        self.shape_resolved(style, text, paint_boundaries, before, after, None)
    }

    pub(crate) fn shape_resolved(
        &self,
        style: &ComputedStyle,
        text: &str,
        paint_boundaries: &[usize],
        before: &str,
        after: &str,
        rtl: Option<bool>,
    ) -> Arc<ShapedText> {
        if text.len() > MAX_TEXT_BYTES {
            let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
            Self::report(
                &mut cache,
                format!("text run exceeds {MAX_TEXT_BYTES} bytes; omitted"),
            );
            return Arc::new(ShapedText::default());
        }
        let before = &before[before.char_indices().rev().nth(15).map_or(0, |(i, _)| i)..];
        let after = &after[..after.char_indices().nth(16).map_or(after.len(), |(i, _)| i)];
        // Shaping reads only the nearest non-transparent context character on
        // each side (joining) and whether pre-context is empty. Whitespace is
        // never joining-transparent, so context beyond it cannot change the
        // result; trimming it lets repeated words share cache entries.
        let before = before
            .rfind(char::is_whitespace)
            .map_or(before, |i| &before[i..]);
        let after = after.find(char::is_whitespace).map_or(after, |i| {
            &after[..i + after[i..].chars().next().map_or(0, char::len_utf8)]
        });
        let key = if text.len() <= 1024 && paint_boundaries.len() <= 32 {
            let line_height = if style.line_height == crate::css::values::LineHeight::Normal {
                None
            } else {
                Some(
                    style
                        .line_height
                        .used_with_diagnostic(style.font_size, |_| {})
                        .to_bits(),
                )
            };
            self.font_key(style).map(|font| RunQuery {
                font,
                size: style.font_size.to_bits(),
                language: style.font_language.as_deref(),
                bold: style.bold,
                italic: style.italic,
                writing_mode: style.writing_mode,
                text_orientation: style.text_orientation,
                line_height,
                rtl,
                boundaries: paint_boundaries,
                before,
                after,
                text,
            })
        } else {
            None
        };
        let hash = key.as_ref().map(RunQuery::hash);
        if let (Some(key), Some(hash)) = (&key, hash)
            && let Some(run) = self.cached_run(key, hash)
        {
            return run;
        }
        let primary = self.select(style);
        let size = if style.font_size.is_finite() {
            style.font_size.clamp(0.0, 512.0)
        } else {
            0.0
        };
        let mut result = ShapedText::default();
        let mut start = 0;
        let mut face = primary.clone();
        let mut script = Script::Common;
        let mut pending = String::new();
        let mut orientation = unicode_vo::Orientation::Rotated;
        let mut remaining_probes = MAX_COVERAGE_PROBES;
        // A whole grapheme chooses one font; consecutive same-script graphemes
        // share shaping. Prefer a face covering the entire script word, so a
        // missing mark does not sever Arabic joining or an Indic syllable.
        for (word_start, word) in text.split_word_bound_indices() {
            let word_face = self.fallback(style, word, &primary, &mut remaining_probes);
            for (local, grapheme) in word.grapheme_indices(true) {
                let offset = word_start + local;
                let next_script = grapheme
                    .chars()
                    .map(|c| c.script())
                    .find(|s| !matches!(s, Script::Common | Script::Inherited | Script::Unknown))
                    .unwrap_or(script);
                let next_orientation = grapheme
                    .chars()
                    .next()
                    .map(unicode_vo::char_orientation)
                    .unwrap_or(unicode_vo::Orientation::Rotated);
                let author_word = self
                    .0
                    .author
                    .iter()
                    .any(|f| Arc::ptr_eq(&f.face, &word_face) && f.descriptor.covers(word));
                let next_face =
                    if word_face.covers(grapheme) && (self.0.author.is_empty() || author_word) {
                        word_face.clone()
                    } else {
                        self.fallback(style, grapheme, &primary, &mut remaining_probes)
                    };
                if !pending.is_empty()
                    && (!Arc::ptr_eq(&face, &next_face)
                        || script != next_script
                        || style.writing_mode.vertical()
                            && style.text_orientation == crate::style::TextOrientation::Mixed
                            && orientation != next_orientation)
                {
                    result.append(&shape_face(
                        &face,
                        style,
                        size,
                        &pending,
                        start,
                        if start == 0 { before } else { &text[..start] },
                        &text[offset..],
                        paint_boundaries,
                        rtl,
                    ));
                    pending.clear();
                }

                if pending.is_empty() {
                    start = offset;
                    face = next_face;
                    script = next_script;
                    orientation = next_orientation;
                }
                pending.push_str(grapheme);
            }
        }
        if !pending.is_empty() {
            result.append(&shape_face(
                &face,
                style,
                size,
                &pending,
                start,
                if start == 0 { before } else { &text[..start] },
                after,
                paint_boundaries,
                rtl,
            ));
        }
        let source: Arc<str> = Arc::from(text);
        for glyph in &mut result.glyphs {
            glyph.source = source.clone();
        }
        let result = Arc::new(result);
        if let (Some(key), Some(hash)) = (key, hash) {
            let key = key.to_key();
            let bytes = key.bytes() + result.glyphs.len() * std::mem::size_of::<ShapedGlyph>();
            let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
            Self::insert_run(&mut cache, hash, key, bytes, result.clone());
        }
        result
    }

    fn cached_run(&self, query: &RunQuery<'_>, hash: u64) -> Option<Arc<ShapedText>> {
        let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(run) = cache
            .runs
            .get(&hash)
            .and_then(|bucket| bucket.iter().find(|(key, _)| query.matches(key)))
        {
            return Some(run.1.clone());
        }
        let bucket = cache.previous_runs.get_mut(&hash)?;
        let index = bucket.iter().position(|(key, _)| query.matches(key))?;
        let (key, run) = bucket.swap_remove(index);
        if bucket.is_empty() {
            cache.previous_runs.remove(&hash);
        }
        cache.previous_run_count -= 1;
        let bytes = key.bytes() + run.glyphs.len() * std::mem::size_of::<ShapedGlyph>();
        Self::insert_run(&mut cache, hash, key, bytes, run.clone());
        Some(run)
    }

    fn insert_run(cache: &mut Cache, hash: u64, key: RunKey, bytes: usize, run: Arc<ShapedText>) {
        if bytes > MAX_RUN_BYTES {
            return;
        }
        if cache.run_count >= MAX_RUNS || cache.run_bytes + bytes > MAX_RUN_BYTES {
            cache.previous_runs = std::mem::take(&mut cache.runs);
            cache.previous_run_count = std::mem::take(&mut cache.run_count);
            cache.run_bytes = 0;
        }
        cache.run_bytes += bytes;
        cache.run_count += 1;
        cache.runs.entry(hash).or_default().push((key, run));
    }
}

#[derive(Clone, PartialEq)]
pub struct ShapedGlyph {
    pub source: Arc<str>,
    pub face: Arc<FontFace>,
    pub id: u16,
    pub size: f32,
    pub cluster: usize,
    pub cluster_end: usize,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub unsafe_to_break: bool,
    pub rtl: bool,
    pub bold: bool,
    pub italic: bool,
    pub rotation: GlyphRotation,
    pub upright: bool,
    /// Optional horizontal paint slice, relative to the glyph's positioned x.
    /// Mandatory clusters crossing paint owners retain one shaping result.
    pub clip: Option<(f32, f32)>,
    pub coordinates: Arc<[([u8; 4], f32)]>,
    pub source_spans: Arc<[SourceSpan]>,
}

/// Original decoded DOM text and byte ranges. A mandatory cluster can span
/// multiple DOM owners; transformed display bytes never become DOM offsets.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub node_id: Option<crate::dom::NodeId>,
    pub source: Arc<str>,
    pub start: usize,
    pub end: usize,
}

impl fmt::Debug for SourceSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceSpan")
            .field("node_id", &self.node_id)
            .field("range", &(self.start..self.end))
            .field("source_bytes", &self.source.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphRotation {
    #[default]
    None,
    Clockwise,
    CounterClockwise,
}

impl GlyphRotation {
    pub fn point(self, x: f32, y: f32) -> (f32, f32) {
        match self {
            Self::None => (x, y),
            Self::Clockwise => (-y, x),
            Self::CounterClockwise => (y, -x),
        }
    }
    pub fn rect(self, rect: Rect) -> Rect {
        let a = self.point(rect.x, rect.y);
        let b = self.point(rect.x + rect.width, rect.y + rect.height);
        Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            width: (b.0 - a.0).abs(),
            height: (b.1 - a.1).abs(),
        }
    }
}

impl fmt::Debug for ShapedGlyph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShapedGlyph")
            .field("face", &self.face)
            .field("id", &self.id)
            .field("cluster", &(self.cluster..self.cluster_end))
            .field("size", &self.size)
            .field("advance", &self.advance)
            .field("x", &self.x)
            .field("y", &self.y)
            .field("x_offset", &self.x_offset)
            .field("y_offset", &self.y_offset)
            .field("rtl", &self.rtl)
            .field("bold", &self.bold)
            .field("italic", &self.italic)
            .field("clip", &self.clip)
            .field("rotation", &self.rotation)
            .field("upright", &self.upright)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub unsafe_after: bool,
    pub line_extents: Option<(f32, f32)>,
    pub(crate) logical_text: String,
    pub(crate) logical_span: Option<SourceSpan>,
}

impl ShapedText {
    pub fn append(&mut self, other: &Self) {
        self.logical_text.push_str(&other.logical_text);
        let origin = self.width;
        self.glyphs
            .extend(other.glyphs.iter().cloned().map(|mut glyph| {
                glyph.x += origin;
                glyph
            }));
        self.width += other.width;
        self.ascent = self.ascent.max(other.ascent);
        self.descent = self.descent.max(other.descent);
        self.unsafe_after = other.unsafe_after;
        self.line_extents = match (self.line_extents, other.line_extents) {
            (Some((a, d)), Some((b, e))) => Some((a.max(b), d.max(e))),
            (a, b) => a.or(b),
        };
    }

    pub fn ink_bounds(&self, _size: f32, x: f32, baseline: f32) -> Rect {
        let mut bounds = Rect::default();
        for glyph in &self.glyphs {
            let Some(bitmap) = glyph
                .face
                .instance_bitmap(glyph.id, glyph.size, &glyph.coordinates)
            else {
                continue;
            };
            let metrics = bitmap.metrics;
            if metrics.width != 0 && metrics.height != 0 {
                let mut local = Rect {
                    x: metrics.xmin as f32,
                    y: -(metrics.ymin as f32 + metrics.height as f32),
                    width: metrics.width as f32
                        + if glyph.bold { 1.0 } else { 0.0 }
                        + if glyph.italic {
                            (metrics.height as f32 * 0.2).round()
                        } else {
                            0.0
                        },
                    height: metrics.height as f32,
                };
                if let Some((left, right)) = glyph.clip {
                    let start = local.x.max(left);
                    let end = (local.x + local.width).min(right);
                    local.x = start;
                    local.width = (end - start).max(0.0);
                }
                let mut rect = glyph.rotation.rect(local);
                rect.x += x + glyph.x;
                rect.y += baseline - glyph.y;
                bounds = bounds.union(rect);
            }
        }
        bounds
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_face(
    face: &Arc<FontFace>,
    style: &ComputedStyle,
    size: f32,
    text: &str,
    base: usize,
    before: &str,
    after: &str,
    paint_boundaries: &[usize],
    resolved_rtl: Option<bool>,
) -> ShapedText {
    let coordinates: Arc<[([u8; 4], f32)]> = face.coordinates(style).into();
    let varied;
    let shaper = if coordinates.is_empty() {
        face.shaping()
    } else {
        let mut instance = face.shaper();
        instance.set_variations(
            &coordinates
                .iter()
                .map(|&(tag, value)| rustybuzz::Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(&tag),
                    value,
                })
                .collect::<Vec<_>>(),
        );
        varied = instance;
        &varied
    };
    let mut buffer = UnicodeBuffer::new();
    for (offset, c) in text.char_indices() {
        if paint_boundaries.contains(&(base + offset))
            && !invisible(c)
            && face.metrics(c, size).advance_width == 0.0
        {
            // Prevent cross-color Unicode composition without discarding the
            // grapheme or losing mark attachment; clusters keep source offsets.
            buffer.add('\u{034f}', offset as u32);
        }
        buffer.add(c, offset as u32);
    }
    // Joining needs nearby logical context, not a copy of every preceding
    // style run in the paragraph.
    let before = &before[before.char_indices().rev().nth(15).map_or(0, |(i, _)| i)..];
    let after = &after[..after.char_indices().nth(16).map_or(after.len(), |(i, _)| i)];
    buffer.set_pre_context(before);
    buffer.set_post_context(after);
    buffer.set_cluster_level(BufferClusterLevel::MonotoneGraphemes);
    if let Some(language) = &style.font_language
        && let Ok(language) = language.parse()
    {
        buffer.set_language(language);
    }
    buffer.guess_segment_properties();
    if let Some(rtl) = resolved_rtl {
        buffer.set_direction(if rtl {
            rustybuzz::Direction::RightToLeft
        } else {
            rustybuzz::Direction::LeftToRight
        });
    }
    let upright = style.writing_mode.vertical()
        && !matches!(
            style.writing_mode,
            crate::style::WritingMode::SidewaysRl | crate::style::WritingMode::SidewaysLr
        )
        && (style.text_orientation == crate::style::TextOrientation::Upright
            || style.text_orientation == crate::style::TextOrientation::Mixed
                && text.chars().any(vertical_upright));
    if upright {
        buffer.set_direction(rustybuzz::Direction::TopToBottom);
    }
    let rtl = !upright && buffer.direction() == rustybuzz::Direction::RightToLeft;
    let mut features = Vec::new();
    use crate::style::typography::{Keyword, Property, Value};
    let mut settings = style.typography.settings(Property::FontFeatures);
    let mut defaults = BTreeMap::new();
    if style.typography.keyword(Property::FontKerning) == Keyword::None {
        defaults.insert(*b"kern", 0.0);
    }
    if let Value::Ligatures {
        common,
        discretionary,
        historical,
        contextual,
    } = style.typography.get(Property::FontLigatures)
    {
        for (tag, value) in [
            (*b"liga", common),
            (*b"clig", common),
            (*b"dlig", discretionary),
            (*b"hlig", historical),
            (*b"calt", contextual),
        ] {
            defaults.insert(tag, f32::from(value));
        }
        if style.typography.length(Property::LetterSpacing, size) != 0.0
            || style.typography.keyword(Property::WordBreak) == Keyword::BreakAll
            || matches!(
                style.typography.keyword(Property::OverflowWrap),
                Keyword::Anywhere | Keyword::BreakWord
            )
            || style.typography.keyword(Property::LineBreak) == Keyword::Anywhere
        {
            defaults.insert(*b"liga", 0.0);
            defaults.insert(*b"clig", 0.0);
        }
    }
    defaults.append(&mut settings);
    for (tag, value) in defaults {
        features.push(rustybuzz::Feature {
            tag: rustybuzz::ttf_parser::Tag::from_bytes(&tag),
            value: value as u32,
            start: 0,
            end: u32::MAX,
        });
    }
    for &boundary in paint_boundaries {
        if boundary > base && boundary < base + text.len() {
            let end = boundary - base;
            let start = text[..end].char_indices().last().map_or(0, |(i, _)| i);
            for tag in [b"liga", b"clig"] {
                features.push(rustybuzz::Feature {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(tag),
                    value: 0,
                    start: start as u32,
                    end: end as u32,
                });
            }
        }
    }
    let output = if coordinates.is_empty() {
        buffer.guess_segment_properties();
        let plan = face.plan(
            buffer.direction(),
            // `rustybuzz::shape` plans an undetected script as `None`.
            Some(buffer.script()).filter(|&s| s != rustybuzz::script::UNKNOWN),
            buffer.language(),
            &features,
        );
        rustybuzz::shape_with_plan(shaper, &plan, buffer)
    } else {
        rustybuzz::shape(shaper, &features, buffer)
    };
    if upright
        && style.text_orientation == crate::style::TextOrientation::Mixed
        && text.chars().all(|c| {
            unicode_vo::char_orientation(c) == unicode_vo::Orientation::TransformedOrRotated
        })
        && output.glyph_infos().iter().all(|g| {
            text[g.cluster as usize..]
                .chars()
                .next()
                .is_some_and(|c| g.glyph_id == u32::from(face.lookup_glyph_index(c)))
        })
    {
        let mut fallback = style.clone();
        fallback.text_orientation = crate::style::TextOrientation::Sideways;
        return shape_face(
            face,
            &fallback,
            size,
            text,
            base,
            before,
            after,
            paint_boundaries,
            resolved_rtl,
        );
    }
    let scale = size / shaper.units_per_em() as f32;
    let varied = coordinates.iter().any(|&(tag, value)| {
        face.axes
            .iter()
            .find(|axis| axis.tag.to_bytes() == tag)
            .is_some_and(|axis| axis.def_value != value)
    });
    let metrics = if varied {
        let ascent = shaper.ascender() as f32 * scale;
        let descent = shaper.descender() as f32 * scale;
        let line_gap = shaper.line_gap() as f32 * scale;
        Some(fontdue::LineMetrics {
            ascent,
            descent,
            line_gap,
            new_line_size: ascent - descent + line_gap,
        })
    } else {
        face.horizontal_line_metrics(size)
    };
    let line_extents = metrics.map(|m| {
        let height = if style.line_height == crate::css::values::LineHeight::Normal {
            m.new_line_size
        } else {
            style.line_height.used_with_diagnostic(size, |_| {})
        };
        let leading = (height as f64 - m.ascent as f64 + m.descent as f64) / 2.0;
        (
            (m.ascent as f64 + leading) as f32,
            (-m.descent as f64 + leading) as f32,
        )
    });
    let mut result = ShapedText {
        ascent: metrics.map_or(size * 0.8, |m| m.ascent),
        descent: metrics.map_or(size * 0.2, |m| -m.descent),
        line_extents,
        ..ShapedText::default()
    };
    if style.writing_mode.vertical() {
        result.ascent = size * 0.5;
        result.descent = size * 0.5;
        let height = if style.line_height == crate::css::values::LineHeight::Normal {
            size
        } else {
            style.line_height.used_with_diagnostic(size, |_| {})
        };
        result.line_extents = Some((height * 0.5, height * 0.5));
    }
    let mut clusters: Vec<_> = output
        .glyph_infos()
        .iter()
        .map(|g| g.cluster as usize)
        .collect();
    clusters.push(text.len());
    clusters.sort_unstable();
    clusters.dedup();
    for (info, pos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        let cluster = info.cluster as usize;
        let end = clusters
            .get(clusters.partition_point(|&c| c <= cluster))
            .copied()
            .unwrap_or(text.len());
        let advance = if upright {
            -pos.y_advance
        } else {
            pos.x_advance
        } as f32
            * scale;
        result.glyphs.push(ShapedGlyph {
            source: Arc::from(""),
            face: face.clone(),
            id: info.glyph_id as u16,
            size,
            cluster: base + cluster,
            cluster_end: base + end,
            x: result.width + if upright { -pos.y_offset } else { pos.x_offset } as f32 * scale,
            y: if upright {
                pos.x_offset as f32 * scale
            } else {
                pos.y_offset as f32 * scale
                    - if style.writing_mode.vertical() {
                        metrics.map_or(size * 0.3, |m| (m.ascent + m.descent) * 0.5)
                    } else {
                        0.0
                    }
            },
            advance,
            x_offset: if upright { -pos.y_offset } else { pos.x_offset } as f32 * scale,
            y_offset: if upright { pos.x_offset } else { pos.y_offset } as f32 * scale,
            unsafe_to_break: info.unsafe_to_break(),
            rtl,
            bold: style.bold
                && face.weight < 600
                && !coordinates.iter().any(|(tag, _)| {
                    tag == b"wght"
                        && (style.font_weight.0 >= 600.0
                            || style
                                .typography
                                .settings(Property::FontVariations)
                                .contains_key(b"wght"))
                }),
            italic: style.italic
                && !face.italic
                && !coordinates.iter().any(|(tag, _)| {
                    matches!(tag, b"ital" | b"slnt")
                        && (style.font_style != FontStyle::Normal
                            || style
                                .typography
                                .settings(Property::FontVariations)
                                .contains_key(tag))
                }),
            rotation: GlyphRotation::None,
            upright,
            clip: None,
            coordinates: coordinates.clone(),
            source_spans: Arc::from([]),
        });
        result.width += advance;
    }
    let spacing = style.typography.length(Property::LetterSpacing, size);
    if spacing != 0.0 {
        let mut shift = 0.0;
        let count = result.glyphs.len();
        for i in 0..count {
            let glyph = &mut result.glyphs[i];
            glyph.x += shift;
            if i + 1 == count
                || output.glyph_infos()[i + 1].cluster != output.glyph_infos()[i].cluster
            {
                glyph.advance += spacing;
                shift += spacing;
            }
        }
        result.width += shift;
    }

    result
}

/// UAX #50 drives orientation, including vertical substitutions before the
/// specified rotated fallback for Tr characters.
fn vertical_upright(c: char) -> bool {
    unicode_vo::char_orientation(c) != unicode_vo::Orientation::Rotated
}

fn style_distance(request: FontStyle, candidate: FontStyle) -> u8 {
    if request == candidate {
        0
    } else if request != FontStyle::Normal && candidate != FontStyle::Normal {
        1
    } else {
        2
    }
}

fn weight_distance(request: u16, candidate: u16) -> (u8, u16) {
    // CSS Fonts' asymmetric 400..500 search, not absolute nearest weight.
    if (400..=500).contains(&request) {
        if (request..=500).contains(&candidate) {
            (0, candidate - request)
        } else if candidate < request {
            (1, request - candidate)
        } else {
            (2, candidate - 500)
        }
    } else if request < 400 {
        if candidate <= request {
            (0, request - candidate)
        } else {
            (1, candidate - request)
        }
    } else if candidate >= request {
        (0, candidate - request)
    } else {
        (1, request - candidate)
    }
}

pub(crate) fn invisible(c: char) -> bool {
    matches!(c as u32, 0x00ad | 0x034f | 0x061c | 0x180e | 0x200b..=0x200f
        | 0x202a..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f | 0xfeff | 0xe0100..=0xe01ef)
}

#[cfg(test)]
mod tests;
