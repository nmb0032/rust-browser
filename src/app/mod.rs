use std::error::Error;

use crate::engine::{Frame, LoadOptions, Navigation, load_page};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DumpFormat {
    Dom,
    Layout,
    DisplayList,
    Stylesheets,
}

pub fn run(
    location: &str,
    dump: Option<DumpFormat>,
    trace: bool,
    throttle_ms: Option<u64>,
) -> Result<(), Box<dyn Error>> {
    let url = crate::net::location_url(location)?;
    let options = LoadOptions { trace, throttle_ms };
    if let Some(format) = dump {
        let frame = load_page(&url, options, |_| Ok(()), || false)?;
        return dump_frame(frame, format);
    }
    let font = crate::paint::load_bundled_font()?;
    let mut navigation = Navigation::new(url, options);
    navigation.start_load()?;
    crate::ui::window::run(navigation, font)
}

fn dump_frame(frame: Frame, dump: DumpFormat) -> Result<(), Box<dyn Error>> {
    if matches!(dump, DumpFormat::Dom | DumpFormat::Stylesheets) {
        for diagnostic in &frame.stylesheets.diagnostics {
            eprintln!(
                "CSS/resource at {:?}: {}",
                diagnostic.node_id, diagnostic.message
            );
        }
    }
    match dump {
        DumpFormat::Stylesheets => {
            crate::devtools::dump_stylesheets(&frame.stylesheets.stylesheets);
        }
        DumpFormat::Dom => crate::devtools::dump_dom(&frame.document),
        DumpFormat::Layout | DumpFormat::DisplayList => {
            let font = crate::paint::load_bundled_font()?;
            let mut tree = frame.layout_tree();
            tree.layout(crate::paint::DEFAULT_VIEWPORT_WIDTH, &font);
            if dump == DumpFormat::Layout {
                crate::devtools::dump_layout(&tree);
            } else {
                let list = crate::paint::build_display_list(
                    &tree,
                    crate::paint::DEFAULT_VIEWPORT_WIDTH,
                    crate::paint::DEFAULT_VIEWPORT_HEIGHT,
                    false,
                );
                crate::devtools::dump_display_list(&list);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout, paint};

    #[test]
    fn partial_documents_produce_paintable_content_before_finish() {
        let location = format!("{}/tests/pages/basic.html", env!("CARGO_MANIFEST_DIR"));
        let url = crate::net::location_url(&location).unwrap();
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
        let mut counts = Vec::new();
        let frame = load_page(
            &url,
            LoadOptions {
                trace: false,
                throttle_ms: Some(0),
            },
            |frame| {
                counts.push(frame.document.node_count());
                updates.push(display_list(&frame.document));
                Ok(())
            },
            || false,
        )
        .unwrap();
        assert!(counts.len() > 1);
        assert!(counts[0] < frame.document.node_count());
        assert_eq!(counts.last(), Some(&frame.document.node_count()));
        assert!(updates[..updates.len() - 1].iter().any(|items| {
            items.iter().any(
                |item| matches!(item, paint::DisplayItem::Text { text, .. } if !text.is_empty()),
            )
        }));
        assert_eq!(updates.last().unwrap(), &display_list(&frame.document));
    }
}
