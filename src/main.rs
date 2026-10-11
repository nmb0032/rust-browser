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

    /// Enumerate and run local HTML rel=match/mismatch tests.
    #[arg(long, conflicts_with_all = ["dump", "trace", "throttle_ms"])]
    reftests: Option<PathBuf>,

    /// Dedicated reftest evidence directory (inside target/ or outside the repo).
    #[arg(long, default_value = "target/reftests")]
    artifacts: PathBuf,

    /// Canonical boundary for local documents, stylesheets, images and fonts.
    #[arg(long, default_value = ".")]
    resource_root: PathBuf,

    /// CSS-pixel viewport for screenshot/reftests.
    #[arg(long, default_value = "800x600", value_parser = viewport)]
    viewport: (u32, u32),

    #[arg(long, default_value = "1")]
    device_scale: f64,

    /// Root document offset in CSS pixels, X,Y; clamped to the layout range.
    #[arg(long, default_value = "0,0", value_parser = scroll)]
    scroll: rust_browser::layout::ScrollOffset,

    /// Programmatic panel offset, element-id:X:Y (repeatable).
    #[arg(long, value_parser = panel)]
    panel_scroll: Vec<rust_browser::screenshot::PanelScroll>,
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
    let options = rust_browser::screenshot::Options {
        width: args.viewport.0,
        height: args.viewport.1,
        scale: args.device_scale,
        scroll: args.scroll,
        panels: args.panel_scroll,
    };
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
    rust_browser::app::run_with_fonts(
        &args.location,
        args.dump,
        args.trace,
        args.throttle_ms,
        args.bundled_fonts,
    )
}
