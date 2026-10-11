//! Deterministic native screenshots: the same Frame, shaping, display list and
//! software rasterizer as the window, without winit or desktop capture.
use std::{error::Error, io, path::Path};

use crate::{
    css::media::{ColorScheme, MediaEnvironment},
    engine::{Frame, load_local_page},
    layout::ScrollOffset,
    paint,
    text::FontContext,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub const MAX_DIMENSION: u32 = 4096;

#[derive(Clone, Debug)]
pub struct PanelScroll {
    pub id: String,
    pub offset: ScrollOffset,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub scroll: ScrollOffset,
    pub panels: Vec<PanelScroll>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            scale: 1.0,
            scroll: ScrollOffset::default(),
            panels: Vec::new(),
        }
    }
}

impl Options {
    pub fn dimensions(&self) -> Result<(u32, u32)> {
        let width = f64::from(self.width) * self.scale;
        let height = f64::from(self.height) * self.scale;
        if !self.scale.is_finite()
            || !(0.125..=8.0).contains(&self.scale)
            || width.fract() != 0.0
            || height.fract() != 0.0
            || !(1.0..=f64::from(MAX_DIMENSION)).contains(&width)
            || !(1.0..=f64::from(MAX_DIMENSION)).contains(&height)
            || self.panels.len() > 128
        {
            return Err(io::Error::other(
                "invalid screenshot size/scale: integral device dimensions 1..4096, scale 0.125..8, at most 128 panels required",
            ).into());
        }
        for offset in std::iter::once(&self.scroll).chain(self.panels.iter().map(|p| &p.offset)) {
            if [offset.x, offset.y]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1_000_000.0).contains(v))
            {
                return Err(io::Error::other("invalid screenshot scroll offset").into());
            }
        }
        Ok((width as u32, height as u32))
    }

    pub fn environment(&self) -> MediaEnvironment {
        MediaEnvironment {
            viewport_width: self.width as f32,
            viewport_height: self.height as f32,
            resolution_dppx: Some(self.scale as f32),
            color_scheme: Some(ColorScheme::Light),
            ..MediaEnvironment::default()
        }
    }
}

/// RGBA8, unpremultiplied sRGB. Native paint composites onto an opaque white
/// canvas, so actual screenshots always have alpha=255.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Screenshot {
    pub fn validate(&self) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || self.rgba.len() != self.width as usize * self.height as usize * 4
        {
            return Err(io::Error::other("invalid screenshot dimensions/RGBA length").into());
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(io::Error::other("screenshot output is a symlink").into());
        }
        image::save_buffer_with_format(
            path,
            &self.rgba,
            self.width,
            self.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        Ok(())
    }

    /// PNG references are bounded and interpreted as RGBA8 sRGB, without
    /// platform color management. Transparent pixels composite onto white.
    pub fn read_png(path: &Path) -> Result<Self> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(crate::net::MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > crate::net::MAX_IMAGE_BYTES
            || image::guess_format(&bytes)? != image::ImageFormat::Png
        {
            return Err(io::Error::other("reference must be a PNG of at most 16 MiB").into());
        }
        let decoded = crate::media::DecodedImage::decode(&bytes)?;
        let mut rgba = decoded.rgba().to_vec();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
            }
            pixel[3] = 255;
        }
        let result = Self {
            width: decoded.width(),
            height: decoded.height(),
            rgba,
        };
        result.validate()?;
        Ok(result)
    }
}

pub fn load(path: &Path, resource_root: &Path, options: &Options) -> Result<Frame> {
    options.dimensions()?;
    let path = path.canonicalize()?;
    let url = url::Url::from_file_path(path).map_err(|_| io::Error::other("invalid page path"))?;
    load_url(&url, resource_root, options)
}

pub fn load_url(url: &url::Url, resource_root: &Path, options: &Options) -> Result<Frame> {
    options.dimensions()?;
    let frame = load_local_page(url, resource_root, options.environment())?;
    check_ready(&frame)?;
    Ok(frame)
}

/// Reject readiness mechanisms we cannot execute, rather than capturing a
/// pre-ready state. Parser recovery remains allowed and uses the production DOM.
pub fn check_ready(frame: &Frame) -> Result<()> {
    if let Some(error) = &frame.error {
        return Err(io::Error::other(error.clone()).into());
    }
    for id in frame.document.preorder() {
        let Some(element) = frame.document.node(id).and_then(|n| n.as_element()) else {
            continue;
        };
        if element.tag_name().eq_ignore_ascii_case("script")
            || element.attribute("class").is_some_and(|value| {
                value
                    .split_ascii_whitespace()
                    .any(|c| matches!(c, "reftest-wait" | "test-wait"))
            })
            || element
                .attributes()
                .iter()
                .any(|a| a.name().starts_with("on"))
        {
            return Err(io::Error::other(
                "unsupported readiness: scripts, testdriver and reftest-wait require JavaScript/WebDriver milestones 9.8/10",
            ).into());
        }
    }
    Ok(())
}

pub fn render(frame: &mut Frame, options: &Options) -> Result<Screenshot> {
    let (width, height) = options.dimensions()?;
    if !frame.resource_policy.local() {
        return Err(io::Error::other(
            "headless render requires a local Frame from screenshot::load or load_url",
        )
        .into());
    }
    check_ready(frame)?;
    let fonts = FontContext::bundled();
    frame.set_media_environment(options.environment());
    // Viewport changes can activate font rules not requested during initial load.
    for _ in 0..16 {
        if !frame.has_pending_web_fonts() {
            break;
        }
        frame.load_pending_web_fonts(|_| Ok(()), || false)?;
    }
    if frame.has_pending_web_fonts() {
        return Err(io::Error::other("headless web fonts did not settle").into());
    }
    let mut tree = frame.layout_with_container_queries(&fonts);
    frame.load_pending_effect_images(|_| Ok(()), || false)?;
    tree.effect_images = frame.effect_images.clone();
    tree.set_viewport_scroll(options.scroll);
    for panel in &options.panels {
        let ids = frame
            .document
            .preorder()
            .filter(|&id| {
                frame
                    .document
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.attribute("id") == Some(panel.id.as_str()))
            })
            .collect::<Vec<_>>();
        if ids.len() != 1 || tree.scroll_container(ids[0]).is_none() {
            return Err(io::Error::other(format!(
                "missing/ambiguous/non-scrollable panel: {}",
                panel.id
            ))
            .into());
        }
        tree.set_scroll_offset(ids[0], panel.offset);
    }
    if !frame.stylesheets.diagnostics.is_empty() || !tree.diagnostics().is_empty() {
        return Err(io::Error::other(format!(
            "headless render diagnostics: CSS/resources {:?}; layout {:?}",
            frame.stylesheets.diagnostics,
            tree.diagnostics()
        ))
        .into());
    }
    frame.retain_scroll_state(&tree);
    let list = paint::build_display_list(&tree, options.width as f32, options.height as f32, false);
    paint::compositor::validate(&list, width as usize, height as usize, options.scale)
        .map_err(io::Error::other)?;
    let mut pixels = vec![0xffffff; width as usize * height as usize];
    paint::rasterize_display_list(
        &list,
        frame.font_context(),
        &mut pixels,
        width as usize,
        height as usize,
        options.scale,
        tree.viewport_scroll().y,
    );
    let rgba = pixels
        .into_iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, p as u8, 255])
        .collect();
    Ok(Screenshot {
        width,
        height,
        rgba,
    })
}

pub fn capture(path: &Path, resource_root: &Path, options: &Options) -> Result<Screenshot> {
    render(&mut load(path, resource_root, options)?, options)
}
