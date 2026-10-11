//! Document-owned, bounded font resources. Only the resource-loading phase
//! performs I/O; style/layout snapshots overlay immutable selected faces.
use super::{Frame, document_base_url, loader::check_cancelled};
use crate::{
    css::font_face::FontSource,
    net, style,
    text::{AuthorFont, FontContext, FontFace},
};
use base64::Engine;
use std::{
    collections::{HashMap, HashSet, hash_map::RandomState},
    error::Error,
    hash::BuildHasher,
    io::{self, Read},
    sync::Arc,
};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_REQUESTS: usize = 64;
const MAX_FACES: usize = 16;

#[derive(Clone, Debug, Default)]
pub(super) struct WebFonts {
    resources: HashMap<url::Url, Result<Arc<[u8]>, String>>,
    faces: HashMap<String, Result<Arc<FontFace>, String>>,
    selections: HashMap<u64, Option<Arc<FontFace>>>,
    hashes: RandomState,
    diagnostics: Vec<style::Diagnostic>,
    bytes: usize,
    work_bytes: usize,
    requests: usize,
    glyphs: usize,
    pub ready: bool,
}

fn key(rule: &style::FontRule, base: &url::Url, hashes: &RandomState) -> u64 {
    // A document-random keyed hash avoids retaining repeated multi-megabyte
    // data: sources in selection/cache keys. Cloned snapshots retain the seed.
    hashes.hash_one((rule.url.as_ref().unwrap_or(base), &rule.descriptor))
}

impl WebFonts {
    pub fn pending(&self, frame: &Frame) -> bool {
        let base = document_base_url(&frame.document, &frame.url);
        self.ready
            && style::font_rules(
                &frame.document,
                &frame.stylesheets,
                frame.media_environment(),
            )
            .iter()
            .any(|r| r.active && !self.selections.contains_key(&key(r, &base, &self.hashes)))
    }
    fn report(&mut self, node: Option<crate::dom::NodeId>, message: String) {
        if self.diagnostics.len() < 128 && !self.diagnostics.iter().any(|d| d.message == message) {
            eprintln!("CSS font: {message}");
            self.diagnostics.push(style::Diagnostic {
                node_id: node,
                message,
            });
        }
    }

    pub fn context(&self, frame: &Frame, fonts: &FontContext) -> FontContext {
        let base = document_base_url(&frame.document, &frame.url);
        if self.selections.is_empty() {
            return fonts.with_author_fonts(Vec::new());
        }
        let author = style::font_rules(
            &frame.document,
            &frame.stylesheets,
            frame.media_environment(),
        )
        .into_iter()
        .filter(|r| r.active)
        .filter_map(|r| {
            self.selections
                .get(&key(&r, &base, &self.hashes))
                .cloned()
                .flatten()
                .map(|face| AuthorFont {
                    descriptor: r.descriptor,
                    face,
                })
        })
        .collect();
        if frame.font_context().same_discovery(fonts) {
            frame.font_context().with_author_fonts(author)
        } else {
            fonts.with_author_fonts(author)
        }
    }

    pub fn diagnostics(&self) -> &[style::Diagnostic] {
        &self.diagnostics
    }

    fn resource(
        &mut self,
        url: &url::Url,
        page: &url::Url,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<[u8]>, String> {
        if let Some(result) = self.resources.get(url) {
            return result.clone();
        }
        self.requests += 1;
        if self.requests > MAX_REQUESTS {
            return Err("document font source count exceeds 64".into());
        }
        let mut final_url = None;
        let result = (|| {
            let raw = if url.scheme() == "data" {
                data_font(url)?
            } else {
                let resource = net::fetch_font(url, page, cancelled).map_err(|e| e.to_string())?;
                final_url = Some(resource.metadata.url);
                resource.bytes
            };
            self.work_bytes = self.work_bytes.saturating_add(raw.len());
            if self.work_bytes > MAX_DOCUMENT_BYTES {
                return Err("document font download/decode work exceeds 64 MiB".into());
            }
            let bytes = decode_with_work(&raw, &mut self.work_bytes)?;
            if self.bytes + bytes.len() > MAX_DOCUMENT_BYTES {
                return Err("document decoded font sources exceed 64 MiB".into());
            }
            self.bytes += bytes.len();
            Ok(Arc::from(bytes))
        })();
        if let Some(final_url) = final_url {
            self.resources.insert(final_url, result.clone());
        }
        self.resources.insert(url.clone(), result.clone());
        result
    }

    fn face(
        &mut self,
        source: &FontSource,
        base: &url::Url,
        page: &url::Url,
        policy: &super::resource_policy::ResourcePolicy,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<FontFace>, String> {
        let source_key = match source {
            FontSource::Local(_) if policy.local() => {
                return Err(
                    "local() system font discovery is unsupported in headless tests".into(),
                );
            }
            FontSource::Local(name) => format!("local:{name}"),
            FontSource::Url(reference) => {
                if !reference.starts_with("data:") && reference.len() > 8192 {
                    return Err("font URL exceeds 8192 bytes".into());
                }
                let mut url = base.join(reference).map_err(|e| e.to_string())?;
                if url.scheme() != "data" {
                    policy.check(&url).map_err(|e| e.to_string())?;
                }
                if url.fragment().is_some() {
                    return Err("font URL collection fragments are unsupported; use local() for named collection faces".into());
                }
                url.set_fragment(None);
                url.to_string()
            }
        };
        if let Some(result) = self.faces.get(&source_key) {
            return result.clone();
        }
        if self.faces.len() >= MAX_REQUESTS {
            return Err("document font source count exceeds 64".into());
        }
        if let FontSource::Url(_) = source
            && let Ok(url) = url::Url::parse(&source_key)
            && let Some(Ok(bytes)) = self.resources.get(&url)
            && let Some(face) = self
                .faces
                .values()
                .filter_map(|f| f.as_ref().ok())
                .find(|f| f.has_source(bytes))
                .cloned()
        {
            self.faces.insert(source_key, Ok(face.clone()));
            return Ok(face);
        }
        let loaded = self
            .faces
            .values()
            .filter_map(|f| f.as_ref().ok())
            .map(|f| Arc::as_ptr(f) as usize)
            .collect::<HashSet<_>>();
        if loaded.len() >= MAX_FACES {
            return Err("document font face count exceeds 16".into());
        }
        let result = match source {
            FontSource::Local(name) => FontContext::system()
                .local(name)
                .ok_or_else(|| format!("local font {name:?} unavailable")),
            FontSource::Url(_) => self
                .resource(
                    &url::Url::parse(&source_key).map_err(|e| e.to_string())?,
                    page,
                    cancelled,
                )
                .and_then(|bytes| {
                    self.faces
                        .values()
                        .filter_map(|f| f.as_ref().ok())
                        .find(|f| f.has_source(&bytes))
                        .cloned()
                        .map_or_else(|| FontContext::downloadable(bytes), Ok)
                }),
        }
        .and_then(|face| {
            if self
                .faces
                .values()
                .filter_map(|f| f.as_ref().ok())
                .any(|f| Arc::ptr_eq(f, &face))
            {
                return Ok(face);
            }
            if self.glyphs + face.glyph_count() as usize > 200_000 {
                return Err("document font glyph count exceeds 200000".into());
            }
            self.glyphs += face.glyph_count() as usize;
            Ok(face)
        });
        self.faces.insert(source_key, result.clone());
        result
    }
}

pub(super) fn load(
    frame: &mut Frame,
    progress: &mut impl FnMut(&Frame) -> Result<(), Box<dyn Error>>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), Box<dyn Error>> {
    let base = document_base_url(&frame.document, &frame.url);
    let rules = style::font_rules(
        &frame.document,
        &frame.stylesheets,
        frame.media_environment(),
    );
    if rules.len() == 256 {
        frame.web_fonts.report(
            None,
            "document @font-face rule limit (256) reached; further rules are not loaded".into(),
        );
    }
    for rule in rules.into_iter().filter(|r| r.active) {
        check_cancelled(cancelled)?;
        let key = key(&rule, &base, &frame.web_fonts.hashes);
        if frame.web_fonts.selections.contains_key(&key) {
            continue;
        }
        if frame.web_fonts.selections.len() >= 256 {
            frame.web_fonts.report(
                rule.node_id,
                "document @font-face rule count exceeds 256".into(),
            );
            break;
        }
        let mut selected = None;
        for source in &rule.descriptor.sources {
            check_cancelled(cancelled)?;
            let result = frame.web_fonts.face(
                source,
                rule.url.as_ref().unwrap_or(&base),
                &frame.url,
                &frame.resource_policy,
                cancelled,
            );
            check_cancelled(cancelled)?;
            match result {
                Ok(face) => {
                    if face.default_instance_only {
                        frame.web_fonts.report(rule.node_id, format!("{}: web variable font uses its default instance only; variation axes are unsupported", face.name));
                    }
                    selected = Some(face);
                    break;
                }
                Err(error) => {
                    // Do not echo potentially multi-megabyte data URLs.
                    frame.web_fonts.report(
                        rule.node_id,
                        format!(
                            "@font-face {:?} source failed: {error}",
                            rule.descriptor.family
                        ),
                    );
                }
            }
        }
        frame.web_fonts.selections.insert(key, selected);
        frame.refresh_fonts();
        progress(frame)?;
    }
    frame.refresh_fonts();
    Ok(())
}

fn data_font(url: &url::Url) -> Result<Vec<u8>, String> {
    let (meta, data) = url
        .as_str()
        .strip_prefix("data:")
        .and_then(|s| s.split_once(','))
        .ok_or("invalid data font URL")?;
    if data.len() > (net::MAX_FONT_BYTES as usize * 4 / 3 + 8) {
        return Err("data font exceeds encoded byte limit".into());
    }
    let mime = meta.split(';').next().unwrap_or_default();
    if !matches!(
        mime.to_ascii_lowercase().as_str(),
        "font/ttf"
            | "font/otf"
            | "font/woff"
            | "font/woff2"
            | "application/font-woff"
            | "application/octet-stream"
    ) {
        return Err("unsupported data font MIME type".into());
    }
    let data = percent_encoding::percent_decode_str(data).collect::<Vec<_>>();
    let result = if meta.split(';').any(|p| p.eq_ignore_ascii_case("base64")) {
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| e.to_string())?
    } else {
        data
    };
    if result.len() > net::MAX_FONT_BYTES as usize {
        return Err("data font exceeds byte limit".into());
    }
    Ok(result)
}

fn bounded_read(
    mut reader: impl Read,
    hint: usize,
    work: &mut usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    if hint > MAX_BYTES {
        return Err(io::Error::other("font decompression size exceeds 32 MiB").into());
    }
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        *work = work.saturating_add(count);
        if *work > MAX_DOCUMENT_BYTES {
            return Err(io::Error::other("document font decode work exceeds 64 MiB").into());
        }
        if result.len() + count > MAX_BYTES {
            return Err(io::Error::other("font decompression exceeds 32 MiB").into());
        }
        result.extend_from_slice(&buffer[..count]);
    }
    Ok(result)
}

#[cfg(test)]
fn decode(raw: &[u8]) -> Result<Vec<u8>, String> {
    decode_with_work(raw, &mut 0)
}

fn decode_with_work(raw: &[u8], work: &mut usize) -> Result<Vec<u8>, String> {
    let signature = raw.get(..4).ok_or("truncated font")?;
    if signature == b"ttcf" || raw.get(4..8) == Some(b"ttcf") {
        return Err("downloadable font collections are unsupported; local() preserves named collection indices".into());
    }
    if signature != b"wOFF" && signature != b"wOF2" {
        if raw.len() > MAX_BYTES {
            return Err("font exceeds 32 MiB".into());
        }
        *work = work.saturating_add(raw.len());
        if *work > MAX_DOCUMENT_BYTES {
            return Err("document font decode work exceeds 64 MiB".into());
        }
        return Ok(raw.to_vec());
    }
    // These are resource-limit checks, not a font decoder. Wuff validates and
    // reconstructs the container, transformed outlines and OpenType tables.
    let declared = raw.get(16..20).ok_or("truncated webfont header")?;
    let declared = u32::from_be_bytes(declared.try_into().unwrap()) as usize;
    if declared > MAX_BYTES || declared == 0 {
        return Err("webfont declared output exceeds size limit".into());
    }
    let tables = raw.get(12..14).ok_or("truncated webfont header")?;
    let tables = u16::from_be_bytes(tables.try_into().unwrap()) as usize;
    if tables == 0 || tables > 128 {
        return Err("webfont table count exceeds 128".into());
    }
    if signature == b"wOFF" {
        let mut sum = 12 + tables * 16;
        for table in 0..tables {
            let offset = 44 + table * 20 + 12;
            let length = raw
                .get(offset..offset + 4)
                .ok_or("truncated WOFF directory")?;
            let length = u32::from_be_bytes(length.try_into().unwrap()) as usize;
            sum += (length + 3) & !3;
            if sum > MAX_BYTES {
                return Err("WOFF table output exceeds 32 MiB".into());
            }
        }
    }
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if signature == b"wOFF" {
            wuff::decompress_woff1_with_custom_z(raw, &mut |input, hint| {
                bounded_read(flate2::read::ZlibDecoder::new(input), hint, work)
            })
        } else {
            wuff::decompress_woff2_with_custom_brotli(raw, &mut |input, hint| {
                bounded_read(
                    brotli_decompressor::Decompressor::new(input, 4096),
                    hint,
                    work,
                )
            })
        }
    }))
    .map_err(|_| "malformed webfont decoder input".to_string())?
    .map_err(|e| format!("invalid webfont: {e}"))?;
    if decoded.len() > MAX_BYTES {
        return Err("webfont decoded output exceeds 32 MiB".into());
    }
    *work = work.saturating_add(decoded.len());
    if *work > MAX_DOCUMENT_BYTES {
        return Err("document font decode work exceeds 64 MiB".into());
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_containers_size_limits_and_decode_work_have_bounded_failures() {
        for signature in [b"wOFF", b"wOF2"] {
            let mut raw = vec![0; 48];
            raw[..4].copy_from_slice(signature);
            raw[12..14].copy_from_slice(&1u16.to_be_bytes());
            raw[16..20].copy_from_slice(&((MAX_BYTES + 1) as u32).to_be_bytes());
            assert!(decode(&raw).unwrap_err().contains("size limit"));
            raw[16..20].copy_from_slice(&512u32.to_be_bytes());
            assert!(decode(&raw).is_err());
        }
        assert!(bounded_read(io::empty(), MAX_BYTES + 1, &mut 0).is_err());
        let mut work = MAX_DOCUMENT_BYTES;
        assert!(bounded_read(io::Cursor::new([1; 10]), 10, &mut work).is_err());
        assert!(decode(b"ttcf\0\0\0\0").unwrap_err().contains("collections"));
        let mut fonts = WebFonts {
            work_bytes: MAX_DOCUMENT_BYTES,
            ..Default::default()
        };
        let url = url::Url::parse("data:font/ttf;base64,AAAAAA==").unwrap();
        assert!(
            fonts
                .resource(&url, &url, &|| false)
                .unwrap_err()
                .contains("work")
        );
        for _ in 0..3 {
            assert!(fonts.resource(&url, &url, &|| false).is_err());
        }
        assert_eq!(fonts.requests, 1);
        fonts.requests = MAX_REQUESTS;
        let next = url::Url::parse("data:font/ttf;base64,AAAAAQ==").unwrap();
        assert!(
            fonts
                .resource(&next, &next, &|| false)
                .unwrap_err()
                .contains("count")
        );
    }

    #[test]
    fn production_frame_and_display_list_rasterize_downloaded_outline_not_synthetic_fallback() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/web_fonts.html");
        let mut frame = crate::engine::load_page(
            &url::Url::from_file_path(path).unwrap(),
            crate::engine::LoadOptions::default(),
            |_| Ok(()),
            || false,
        )
        .unwrap();
        let fonts = FontContext::system();
        let tree = frame.layout_with_container_queries(&fonts);
        let list = crate::paint::build_display_list(&tree, 800.0, 600.0, false);
        let mut pixels = vec![0; 800 * 600];
        crate::paint::rasterize_display_list(&list, &fonts, &mut pixels, 800, 600, 1.0, 0.0);
        assert!(list.iter().any(|item| matches!(item,
            crate::paint::DisplayItem::Text { glyphs, .. }
                if glyphs.glyphs.iter().any(|g| g.face.name == "Noto Sans Italic" && g.id != 0 && !g.italic))));
        assert!(pixels.iter().any(|&p| p != 0 && p != 0xffffff));
        let mut without = frame.clone();
        without.web_fonts = WebFonts::default();
        let tree = without.layout_with_container_queries(&fonts);
        let list = crate::paint::build_display_list(&tree, 800.0, 600.0, false);
        let mut fallback = vec![0; 800 * 600];
        crate::paint::rasterize_display_list(&list, &fonts, &mut fallback, 800, 600, 1.0, 0.0);
        assert_ne!(pixels, fallback);
    }
}
