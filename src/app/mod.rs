use std::error::Error;

use crate::engine::{Frame, LoadOptions, Navigation, load_page};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DumpFormat {
    Dom,
    Layout,
    DisplayList,
    Stylesheets,
    Style,
    Values,
    CssTokens,
    CssCoverage,
    ParseErrors,
}

pub fn run(
    location: &str,
    dump: Option<DumpFormat>,
    trace: bool,
    throttle_ms: Option<u64>,
) -> Result<(), Box<dyn Error>> {
    run_with_fonts(location, dump, trace, throttle_ms, false)
}

pub fn run_with_fonts(
    location: &str,
    dump: Option<DumpFormat>,
    trace: bool,
    throttle_ms: Option<u64>,
    bundled_fonts: bool,
) -> Result<(), Box<dyn Error>> {
    if dump == Some(DumpFormat::CssCoverage) {
        print!("{}", crate::style::metadata::css_coverage());
        return Ok(());
    }
    let url = crate::net::location_url(location)?;
    let options = LoadOptions { trace, throttle_ms };
    let fonts = if bundled_fonts {
        crate::text::FontContext::bundled()
    } else {
        crate::text::FontContext::system()
    };
    if let Some(format) = dump {
        let frame = load_page(&url, options, |_| Ok(()), || false)?;
        return dump_frame(frame, format, &fonts);
    }
    let mut navigation = Navigation::new(url, options);
    navigation.start_load()?;
    crate::ui::window::run(navigation, fonts)
}

fn dump_frame(
    mut frame: Frame,
    dump: DumpFormat,
    fonts: &crate::text::FontContext,
) -> Result<(), Box<dyn Error>> {
    let tree = if matches!(
        dump,
        DumpFormat::Style | DumpFormat::Layout | DumpFormat::DisplayList | DumpFormat::Values
    ) {
        Some(frame.layout_with_container_queries(fonts))
    } else {
        None
    };
    if dump != DumpFormat::ParseErrors {
        for diagnostic in &frame.stylesheets.diagnostics {
            eprintln!(
                "CSS/resource at {:?}: {}",
                diagnostic.node_id, diagnostic.message
            );
        }
    }
    match dump {
        DumpFormat::CssCoverage => print!("{}", crate::style::metadata::css_coverage()),
        DumpFormat::Style => {
            print!(
                "{}",
                crate::devtools::format_style(&frame.document, &frame.styles)
            );
        }
        DumpFormat::CssTokens => {
            print!("{}", crate::devtools::format_document_css_tokens(&frame));
        }
        DumpFormat::Stylesheets => {
            crate::devtools::dump_stylesheets(&frame.stylesheets.stylesheets);
        }
        DumpFormat::Dom => crate::devtools::dump_dom(&frame.document),
        DumpFormat::ParseErrors => {
            for error in &frame.parse_errors {
                println!(
                    "{}:{} (byte {}): {}",
                    error.position.line,
                    error.position.column,
                    error.position.byte_offset,
                    error.code
                );
            }
            if frame.dropped_parse_errors > 0 {
                println!(
                    "{} additional parse errors omitted",
                    frame.dropped_parse_errors
                );
            }
        }
        DumpFormat::Layout | DumpFormat::DisplayList | DumpFormat::Values => {
            let tree = tree.expect("geometry dumps complete container feedback");
            let environment = frame.media_environment();
            if dump == DumpFormat::Values {
                print!("{}", crate::devtools::format_values(&tree, 1.0));
            } else if dump == DumpFormat::Layout {
                crate::devtools::dump_layout(&tree);
            } else {
                let list = crate::paint::build_display_list(
                    &tree,
                    environment.viewport_width,
                    environment.viewport_height,
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
