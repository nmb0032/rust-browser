use clap::Parser;
use rust_browser::app::DumpFormat;

#[derive(Parser)]
struct Args {
    #[arg(default_value = "https://example.com")]
    location: String,

    #[arg(long, value_enum, value_name = "dom|stylesheets|layout|display-list")]
    dump: Option<DumpFormat>,

    #[arg(long)]
    trace: bool,

    #[arg(long, value_name = "MILLISECONDS")]
    throttle_ms: Option<u64>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    rust_browser::app::run(&args.location, args.dump, args.trace, args.throttle_ms)
}
