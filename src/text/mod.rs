//! Shared font selection, shaping and indexed raster sources. Positions are
//! CSS pixels, relative to an explicit baseline; clusters are UTF-8 byte offsets.
use std::{
    collections::HashMap,
    fmt, fs,
    ops::Deref,
    sync::{Arc, Mutex, OnceLock},
};

use fontdb::{Database, Family, Query, Source, Style, Weight};
use rustybuzz::{BufferClusterLevel, UnicodeBuffer};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    css::values::{FamilyName, FontStyle},
    layout::Rect,
    style::ComputedStyle,
};

const BUNDLED: &[u8] = include_bytes!("../../assets/fonts/noto-sans/NotoSans.ttf");
pub const MAX_TEXT_BYTES: usize = 65_536;
const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FACES: usize = 16;
const MAX_REQUESTS: usize = 128;
const MAX_GLYPHS: usize = 200_000;
const MAX_COVERAGE_PROBES: usize = 8192;

pub struct FontFace {
    pub name: String,
    pub index: u32,
    pub weight: u16,
    pub italic: bool,
    pub default_instance_only: bool,
    bytes: Arc<[u8]>,
    raster: fontdue::Font,
    bitmaps: Mutex<HashMap<(u16, u32), Arc<GlyphBitmap>>>,
    units_per_em: f32,
    x_height: Option<i16>,
    cap_height: Option<i16>,
}

pub struct GlyphBitmap {
    pub metrics: fontdue::Metrics,
    pub pixels: Vec<u8>,
}

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
        &self.raster
    }
}

impl FontFace {
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
        if style.writing_mode.vertical() {
            let face = self.shaper();
            let advance = |c| {
                face.glyph_index(c)
                    .and_then(|id| face.glyph_ver_advance(id))
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
        let default_instance_only = face.is_variable();
        let units_per_em = face.units_per_em() as f32;
        let x_height = face.x_height();
        let cap_height = face.capital_height();
        if face.tables().glyf.is_none() && face.tables().cff.is_none() {
            return Err(
                "unsupported raster format (color/bitmap-only or CFF2); outline fallback required"
                    .into(),
            );
        }
        let raster = fontdue::Font::from_bytes(
            bytes.as_ref(),
            fontdue::FontSettings {
                collection_index: index,
                ..fontdue::FontSettings::default()
            },
        )
        .map_err(str::to_owned)?;
        if raster.horizontal_line_metrics(16.0).is_none() {
            return Err("font has no horizontal metrics".into());
        }
        Ok(Self {
            name,
            index,
            weight,
            italic,
            default_instance_only,
            bytes,
            raster,
            bitmaps: Mutex::new(HashMap::new()),
            units_per_em,
            x_height,
            cap_height,
        })
    }

    pub fn shaper(&self) -> rustybuzz::Face<'_> {
        // Validated once on loading; the immutable bytes and index never change.
        rustybuzz::Face::from_slice(&self.bytes, self.index).expect("validated font face")
    }

    fn covers(&self, text: &str) -> bool {
        text.chars()
            .all(|c| invisible(c) || self.lookup_glyph_index(c) != 0)
    }

    pub fn rasterized(&self, id: u16, size: f32) -> Option<Arc<GlyphBitmap>> {
        if !size.is_finite() || !(0.0..=2048.0).contains(&size) || id >= self.glyph_count() {
            return None;
        }
        let key = (id, size.to_bits());
        let mut cache = self.bitmaps.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bitmap) = cache.get(&key) {
            return Some(bitmap.clone());
        }
        let metrics = self.metrics_indexed(id, size);
        if metrics
            .width
            .checked_mul(metrics.height)
            .is_none_or(|area| area > 1_048_576)
        {
            return None;
        }
        let (metrics, pixels) = self.rasterize_indexed(id, size);
        let bitmap = Arc::new(GlyphBitmap { metrics, pixels });
        if cache.len() >= 1024
            || cache.values().map(|b| b.pixels.len()).sum::<usize>() + bitmap.pixels.len()
                > 2_097_152
        {
            cache.clear();
        }
        cache.insert(key, bitmap.clone());
        Some(bitmap)
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

#[derive(Default)]
struct Cache {
    faces: HashMap<fontdb::ID, Option<Arc<FontFace>>>,
    requests: HashMap<String, Arc<FontFace>>,
    sources: HashMap<String, Arc<[u8]>>,
    bytes: usize,
    glyphs: usize,
    messages: Vec<String>,
    runs: HashMap<String, Arc<ShapedText>>,
    run_bytes: usize,
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
        &self.0.bundled.raster
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
            face.shaper()
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
                    .filter(|f| text.is_none_or(|t| f.descriptor.covers(t) && f.face.covers(t)))
                    .min_by_key(|f| {
                        (
                            style_distance(style.font_style, f.descriptor.style),
                            weight_distance(style.font_weight.0 as u16, f.descriptor.weight),
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

    pub fn select(&self, style: &ComputedStyle) -> Arc<FontFace> {
        let key = style_key(style);
        if let Some(key) = &key
            && let Some(face) = self
                .0
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .requests
                .get(key)
        {
            return face.clone();
        }
        let face = style
            .font_family
            .0
            .iter()
            .take(32)
            .find_map(|name| self.query_text(style, Self::family(name), Some("0")))
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
            style_key(style).map(|key| format!("{key}:{}:{:?}:{}:{}:{line_height:?}:{rtl:?}:{:?}:{:?}:{paint_boundaries:?}:{before:?}:{after:?}:{text}", style.font_size, style.font_language, style.bold, style.italic, style.writing_mode, style.text_orientation))
        } else {
            None
        };
        if let Some(key) = &key
            && let Some(run) = self
                .0
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .runs
                .get(key)
        {
            return run.clone();
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
        if let Some(key) = key {
            let bytes =
                key.len() + result.glyphs.len() * std::mem::size_of::<ShapedGlyph>() + text.len();
            let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.runs.len() < MAX_REQUESTS && cache.run_bytes + bytes <= 2_097_152 {
                cache.run_bytes += bytes;
                cache.runs.insert(key, result.clone());
            }
        }
        result
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

fn style_key(style: &ComputedStyle) -> Option<String> {
    let bytes = style.font_family.0.iter().try_fold(0usize, |sum, family| {
        let (FamilyName::Named(name) | FamilyName::Generic(name)) = family;
        sum.checked_add(name.len() + 4)
    })?;
    if bytes > 4096 || style.font_language.as_ref().is_some_and(|s| s.len() > 64) {
        return None;
    }
    Some(format!(
        "{}:{}:{:?}",
        style.font_family, style.font_weight.0, style.font_style
    ))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub unsafe_after: bool,
    pub line_extents: Option<(f32, f32)>,
}

impl ShapedText {
    pub fn append(&mut self, other: &Self) {
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
            let metrics = glyph.face.metrics_indexed(glyph.id, glyph.size);
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
    let shaper = face.shaper();
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
    let output = rustybuzz::shape(&shaper, &features, buffer);
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
    let metrics = face.horizontal_line_metrics(size);
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
            bold: style.bold && face.weight < 600,
            italic: style.italic && !face.italic,
            rotation: GlyphRotation::None,
            upright,
            clip: None,
        });
        result.width += advance;
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
