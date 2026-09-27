use crate::html::parser;
use crate::net::fetch_html;

// First path: app → engine → navigation/net → html → dom → css/style → layout → paint/render
pub fn run(url: &str, dump_dom: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !dump_dom {
        return crate::ui::window::run();
    }

    let html = fetch_html(url)?;
    let document = parser::parse(&html)
        .map_err(|error| std::io::Error::other(format!("HTML parse failed: {error:?}")))?;

    crate::devtools::dump_dom(&document);

    Ok(())
}
