use crate::html::parser;
use crate::net::fetch_html;

// First path: app → engine → navigation/net → html → dom → css/style → layout → paint/render
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let html = fetch_html("https://example.com")?;
    let document = parser::parse(&html)
        .map_err(|error| std::io::Error::other(format!("HTML parse failed: {error:?}")))?;

    println!("{document:#?}");
    Ok(())
}
