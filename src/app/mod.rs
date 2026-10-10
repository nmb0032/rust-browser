use std::{error::Error, io, thread, time::Duration};

use crate::dom::Document;
use crate::html::parser;
use crate::layout;
use crate::net::stream_html;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DumpFormat {
    Dom,
    Layout,
    DisplayList,
    Stylesheets,
}

// First path: app → engine → navigation/net → html → dom → css/style → layout → paint/render
pub fn run(
    url: &str,
    dump: Option<DumpFormat>,
    trace: bool,
    throttle_ms: Option<u64>,
) -> Result<(), Box<dyn Error>> {
    if let Some(dump_format) = dump {
        let document = load_document(url, trace, throttle_ms, |_| Ok(()))?;
        return dump_document(document, dump_format);
    }

    let font = crate::paint::load_bundled_font()?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let location = url.to_string();

    let _worker = thread::Builder::new()
        .name("html-loader".to_string())
        .spawn(move || {
            let result = load_document(&location, trace, throttle_ms, |document| {
                sender
                    .send(crate::ui::window::LoadEvent::Progress(document.clone()))
                    .map_err(|_| {
                        Box::new(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "browser window closed during page load",
                        )) as Box<dyn Error>
                    })
            });
            let completion = result.map(|_| ()).map_err(|error| error.to_string());

            if let Err(error) = sender.send(crate::ui::window::LoadEvent::Finished(completion)) {
                eprintln!("could not report page load completion: {error}");
            }
        })?;

    crate::ui::window::run(receiver, font)
}

fn load_document(
    location: &str,
    trace: bool,
    throttle_ms: Option<u64>,
    mut on_progress: impl FnMut(&Document) -> Result<(), Box<dyn Error>>,
) -> Result<Document, Box<dyn Error>> {
    let chunk_size = if throttle_ms.is_some() { 256 } else { 8192 };
    let delay = throttle_ms.map(Duration::from_millis);
    let mut parser = parser::HtmlParser::new();
    let mut last_node_count = parser.node_count();

    stream_html(location, chunk_size, |chunk| {
        parser
            .feed(chunk)
            .map_err(|error| io::Error::other(format!("HTML parse failed: {error:?}")))?;

        let node_count = parser.node_count();
        if trace {
            eprintln!("[trace] {node_count} DOM nodes after {} bytes", chunk.len());
        }
        if node_count != last_node_count {
            on_progress(parser.document())?;
            last_node_count = node_count;
        }
        if let Some(delay) = delay {
            thread::sleep(delay);
        }
        Ok(())
    })?;

    let document = parser
        .finish()
        .map_err(|error| io::Error::other(format!("HTML parse failed: {error:?}")))?;
    if trace {
        eprintln!(
            "[trace] parsing finished with {} DOM nodes",
            document.node_count()
        );
    }
    on_progress(&document)?;
    Ok(document)
}

fn dump_document(document: Document, dump: DumpFormat) -> Result<(), Box<dyn Error>> {
    match dump {
        DumpFormat::Stylesheets => {
            let result = crate::style::collect_stylesheets(&document);
            for diagnostic in &result.diagnostics {
                eprintln!("CSS at {:?}: {}", diagnostic.node_id, diagnostic.message);
            }
            crate::devtools::dump_stylesheets(&result.stylesheets);
            Ok(())
        }
        DumpFormat::Dom => {
            crate::devtools::dump_dom(&document);
            Ok(())
        }
        DumpFormat::Layout => {
            let font = crate::paint::load_bundled_font()?;
            let mut layout_tree = layout::build_layout_tree(&document);
            layout_tree.layout(crate::paint::DEFAULT_VIEWPORT_WIDTH, &font);
            crate::devtools::dump_layout(&layout_tree);
            Ok(())
        }
        DumpFormat::DisplayList => {
            let font = crate::paint::load_bundled_font()?;
            let mut layout_tree = layout::build_layout_tree(&document);
            layout_tree.layout(crate::paint::DEFAULT_VIEWPORT_WIDTH, &font);
            let display_list = crate::paint::build_display_list(
                &layout_tree,
                crate::paint::DEFAULT_VIEWPORT_WIDTH,
                crate::paint::DEFAULT_VIEWPORT_HEIGHT,
                false,
            );
            crate::devtools::dump_display_list(&display_list);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_document;
    use crate::{layout, paint};

    #[test]
    fn reports_partial_documents_while_loading_a_file() {
        let location = format!("{}/tests/pages/basic.html", env!("CARGO_MANIFEST_DIR"));
        let mut node_counts = Vec::new();
        let document = load_document(&location, false, Some(0), |document| {
            node_counts.push(document.node_count());
            Ok(())
        })
        .unwrap();

        assert!(node_counts.len() > 1);
        assert!(node_counts[0] < document.node_count());
        assert_eq!(node_counts.last(), Some(&document.node_count()));
    }

    #[test]
    fn partial_documents_produce_paintable_content_before_finish() {
        let location = format!("{}/tests/pages/basic.html", env!("CARGO_MANIFEST_DIR"));
        let font = paint::load_bundled_font().unwrap();
        let display_list = |document: &crate::dom::Document| {
            let mut tree = layout::build_layout_tree(document);
            tree.layout(paint::DEFAULT_VIEWPORT_WIDTH, &font);
            paint::build_display_list(
                &tree,
                paint::DEFAULT_VIEWPORT_WIDTH,
                paint::DEFAULT_VIEWPORT_HEIGHT,
                false,
            )
        };
        let mut updates = Vec::new();
        let document = load_document(&location, false, Some(0), |document| {
            updates.push(display_list(document));
            Ok(())
        })
        .unwrap();

        assert!(updates.len() > 1);
        assert!(updates[..updates.len() - 1].iter().any(|items| {
            items.iter().any(
                |item| matches!(item, paint::DisplayItem::Text { text, .. } if !text.is_empty()),
            )
        }));
        assert_eq!(updates.last().unwrap(), &display_list(&document));
    }
}
