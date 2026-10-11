use clap::Parser;
use rust_browser::app::DumpFormat;

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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    rust_browser::app::run_with_fonts(
        &args.location,
        args.dump,
        args.trace,
        args.throttle_ms,
        args.bundled_fonts,
    )
}
