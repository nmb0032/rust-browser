use clap::Parser;
use rust_browser::app::DumpFormat;
use std::{io, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(default_value = "https://example.com")]
    location: String,

    #[arg(long, value_enum)]
    dump: Option<DumpFormat>,

    #[arg(long)]
    trace: bool,

    #[arg(long, value_name = "MILLISECONDS")]
    throttle_ms: Option<u64>,

    /// Disable system discovery for deterministic geometry/dump tests.
    #[arg(long)]
    bundled_fonts: bool,

    /// Save a deterministic native PNG, without opening a window.
    #[arg(long, conflicts_with_all = ["dump", "reftests", "trace", "throttle_ms"])]
    screenshot: Option<PathBuf>,
    /// Capture a one-based physical print page with --screenshot or --reftests.
    #[arg(long)]
    print_page: Option<usize>,
    /// Fallback physical sheet size in CSS pixels. @page size has precedence.
    #[arg(long, value_parser = viewport)]
    page_size: Option<(u32, u32)>,
    /// Read-only native paged-media preview (wheel/PageUp/PageDown to scroll).
    #[arg(long, conflicts_with_all = ["screenshot", "reftests", "dump", "trace", "throttle_ms"])]
    print_preview: bool,

    /// Enumerate and run local HTML rel=match/mismatch tests.
    #[arg(long, conflicts_with_all = ["dump", "trace", "throttle_ms"])]
    reftests: Option<PathBuf>,

    /// Dedicated reftest evidence directory (inside target/ or outside the repo).
    #[arg(long, default_value = "target/reftests")]
    artifacts: PathBuf,

    /// Canonical boundary for local documents, stylesheets, images and fonts.
    #[arg(long, default_value = ".")]
    resource_root: PathBuf,

    /// Unzoomed logical viewport for screenshot/reftests (physical / DPR).
    #[arg(long, default_value = "800x600", value_parser = viewport)]
    viewport: (u32, u32),

    #[arg(long, alias = "scale", default_value = "1")]
    device_scale: f64,

    /// Page magnification; CSS viewport shrinks, device buffers stay fixed.
    #[arg(long, default_value = "1")]
    page_zoom: f64,

    /// Encoded output profile. Display-P3 requires a tagged headless PNG;
    /// native softbuffer supports assumed sRGB only.
    #[arg(long, value_enum, default_value = "srgb")]
    color_profile: rust_browser::display::ColorProfile,

    /// Root document offset in CSS pixels, X,Y; clamped to the layout range.
    #[arg(long, default_value = "0,0", value_parser = scroll)]
    scroll: rust_browser::layout::ScrollOffset,

    /// Programmatic panel offset, element-id:X:Y (repeatable).
    #[arg(long, value_parser = panel)]
    panel_scroll: Vec<rust_browser::screenshot::PanelScroll>,
    /// Apply snap selection to configured offsets, without smooth animation.
    #[arg(long)]
    snap_scroll: bool,
    /// Same-document fragment to reveal after resources settle.
    #[arg(long)]
    fragment: Option<String>,

    /// Freeze declarative animations for native screenshots/reftests: 500ms,
    /// 0.5s, or bare milliseconds. Default zero; no scripts are executed.
    #[arg(long, default_value="0", value_parser=animation_time)]
    animation_time: f64,
}

fn animation_time(value: &str) -> Result<f64, String> {
    let (value, divisor) = if let Some(v) = value.strip_suffix("ms") {
        (v, 1000.0)
    } else if let Some(v) = value.strip_suffix('s') {
        (v, 1.0)
    } else {
        (value, 1000.0)
    };
    let seconds = value.parse::<f64>().map_err(|_| "invalid animation time")? / divisor;
    (rust_browser::style::animation::valid_time(seconds) && seconds >= 0.0)
        .then_some(seconds)
        .ok_or("animation time must be finite, nonnegative and bounded".into())
}

fn viewport(value: &str) -> Result<(u32, u32), String> {
    let (width, height) = value.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    Ok((
        width.parse().map_err(|_| "invalid width")?,
        height.parse().map_err(|_| "invalid height")?,
    ))
}

fn scroll(value: &str) -> Result<rust_browser::layout::ScrollOffset, String> {
    let (x, y) = value.split_once(',').ok_or("expected X,Y")?;
    Ok(rust_browser::layout::ScrollOffset {
        x: x.parse().map_err(|_| "invalid scroll x")?,
        y: y.parse().map_err(|_| "invalid scroll y")?,
    })
}

fn panel(value: &str) -> Result<rust_browser::screenshot::PanelScroll, String> {
    let (id, offsets) = value.split_once(':').ok_or("expected ID:X:Y")?;
    let (x, y) = offsets.split_once(':').ok_or("expected ID:X:Y")?;
    if id.is_empty() {
        return Err("empty panel id".into());
    }
    Ok(rust_browser::screenshot::PanelScroll {
        id: id.into(),
        offset: scroll(&format!("{x},{y}"))?,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if (args.print_page.is_some() || args.page_size.is_some())
        && args.screenshot.is_none()
        && args.reftests.is_none()
        && !args.print_preview
    {
        return Err(io::Error::other(
            "--print-page/--page-size require --screenshot, --reftests or --print-preview",
        )
        .into());
    }
    let mut profile = rust_browser::engine::PrintOptions::default();
    if let Some((width, height)) = args.page_size {
        profile.width = width as f32;
        profile.height = height as f32;
        // Explicit small sheets are useful for deterministic page fixtures.
        profile.margin = 0.0;
    }
    if args.animation_time != 0.0
        && args.screenshot.is_none()
        && args.reftests.is_none()
        && !args.print_preview
    {
        return Err(
            io::Error::other("--animation-time requires --screenshot or --reftests").into(),
        );
    }
    let options = rust_browser::screenshot::Options {
        print: (args.print_page.is_some() || args.page_size.is_some() || args.print_preview)
            .then_some(rust_browser::screenshot::PrintCapture {
                options: profile,
                page: args.print_page.unwrap_or(1),
            }),
        width: args.viewport.0,
        height: args.viewport.1,
        scale: args.device_scale,
        zoom: args.page_zoom,
        color_profile: args.color_profile,
        scroll: args.scroll,
        panels: args.panel_scroll,
        snap_scroll: args.snap_scroll,
        fragment: args.fragment,
        animation_time: args.animation_time,
    };
    if args.print_preview {
        args.color_profile.validate_native()?;
        let url = rust_browser::net::location_url(&args.location)?;
        let mut frame = rust_browser::screenshot::load_url(&url, &args.resource_root, &options)?;
        frame.layout_with_container_queries(&rust_browser::text::FontContext::bundled());
        frame.set_animation_time(args.animation_time)?;
        let layout = frame.print_layout(profile).map_err(io::Error::other)?;
        if !layout.diagnostics.is_empty() {
            return Err(io::Error::other(format!(
                "print preview diagnostics: {:?}",
                layout.diagnostics
            ))
            .into());
        }
        return rust_browser::ui::print_preview::run_with_zoom(layout, args.page_zoom);
    }
    if let Some(suite) = args.reftests {
        let report =
            rust_browser::reftest::run(&suite, &args.resource_root, &args.artifacts, &options)?;
        for outcome in &report.outcomes {
            println!(
                "{} {}",
                if outcome.passed { "PASS" } else { "FAIL" },
                outcome.path.display()
            );
            if !outcome.passed {
                if let Some(error) = &outcome.error {
                    eprintln!("{error}");
                }
                for comparison in &outcome.comparisons {
                    eprintln!("{comparison}");
                }
            }
        }
        println!("manifest: {}", report.manifest.display());
        if !report.passed() {
            return Err(io::Error::other("reftest failures").into());
        }
        return Ok(());
    }
    if let Some(path) = args.screenshot {
        let url = rust_browser::net::location_url(&args.location)?;
        let mut frame = rust_browser::screenshot::load_url(&url, &args.resource_root, &options)?;
        let image = rust_browser::screenshot::render(&mut frame, &options)?;
        image.save(&path)?;
        println!("{}x{} PNG: {}", image.width, image.height, path.display());
        return Ok(());
    }
    if args.dump.is_none() {
        args.color_profile.validate_native()?;
    }
    rust_browser::app::run_with_display(
        &args.location,
        args.dump,
        args.trace,
        args.throttle_ms,
        args.bundled_fonts,
        args.page_zoom,
        args.color_profile,
    )
}
