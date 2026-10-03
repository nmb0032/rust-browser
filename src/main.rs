use clap::Parser;
use rust_browser::app::DumpFormat;

#[derive(Parser)]
struct Args {
    #[arg(default_value = "https://example.com")]
    location: String,

    #[arg(long, value_enum)]
    dump: Option<DumpFormat>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    rust_browser::app::run(&args.location, args.dump)
}
