use crate::html::parser;

// First path: app → engine → navigation/net → html → dom → css/style → layout → paint/render
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{:#?}",
        parser::parse(
            r#"<html><body><main><h1>Hello, browser!</h1><p>Welcome to our page.</p><ul><li>Parse elements</li><li>Parse nested content</li></ul></main></body></html>"#
        )
    );
    Ok(())
}
