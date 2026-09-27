use crate::html::parser;
use crate::net::fetch_html;

// First path: app → engine → navigation/net → html → dom → css/style → layout → paint/render
pub fn run(url: &str, dump_dom: bool) -> Result<(), Box<dyn std::error::Error>> {
    let html = fetch_html(url)?;
    let document = parser::parse(&html)
        .map_err(|error| std::io::Error::other(format!("HTML parse failed: {error:?}")))?;

    if dump_dom {
        crate::devtools::dump_dom(&document);
    } else {
        println!("{document:#?}");
    }

    Ok(())
}
