use crate::html::parser;
use crate::layout;
use crate::net::fetch_html;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DumpFormat {
    Dom,
    Layout,
    DisplayList,
}

// First path: app → engine → navigation/net → html → dom → css/style → layout → paint/render
pub fn run(url: &str, dump: Option<DumpFormat>) -> Result<(), Box<dyn std::error::Error>> {
    let html = fetch_html(url)?;
    let document = parser::parse(&html)
        .map_err(|error| std::io::Error::other(format!("HTML parse failed: {error:?}")))?;

    if dump == Some(DumpFormat::Dom) {
        crate::devtools::dump_dom(&document);
        return Ok(());
    }

    let font = crate::paint::load_bundled_font()?;
    let mut layout_tree = layout::build_layout_tree(&document);
    layout_tree.layout(crate::paint::DEFAULT_VIEWPORT_WIDTH, &font);

    if dump == Some(DumpFormat::Layout) {
        crate::devtools::dump_layout(&layout_tree);
        return Ok(());
    }

    if dump == Some(DumpFormat::DisplayList) {
        let display_list = crate::paint::build_display_list(
            &layout_tree,
            crate::paint::DEFAULT_VIEWPORT_WIDTH,
            crate::paint::DEFAULT_VIEWPORT_HEIGHT,
            false,
        );
        crate::devtools::dump_display_list(&display_list);
        return Ok(());
    }

    crate::ui::window::run(layout_tree, font)
}
