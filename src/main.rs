use clap::{Parser, ValueEnum};

#[derive(Parser)]
struct Args {
    #[arg(default_value = "https://example.com")]
    location: String,

    #[arg(long, value_enum)]
    dump: Option<DumpFormat>,
}

#[derive(Clone, ValueEnum)]
enum DumpFormat {
    Dom,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    rust_browser::app::run(&args.location, args.dump.is_some())
}
