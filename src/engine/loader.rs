use std::{error::Error, io, thread, time::Duration};

use url::Url;

use crate::{html::encoding::HtmlByteParser, net};

use super::{Frame, resources::load_resources};

#[derive(Clone, Copy, Debug, Default)]
pub struct LoadOptions {
    pub trace: bool,
    pub throttle_ms: Option<u64>,
}

pub fn load_page(
    url: &Url,
    options: LoadOptions,
    mut on_progress: impl FnMut(&Frame) -> Result<(), Box<dyn Error>>,
    cancelled: impl Fn() -> bool,
) -> Result<Frame, Box<dyn Error>> {
    check_cancelled(&cancelled)?;
    let chunk_size = if options.throttle_ms.is_some() {
        256
    } else {
        8192
    };
    let mut parser = None;
    let mut xml_bytes = Vec::new();
    let mut last_revision = 0;
    let metadata = net::stream_document_bytes(url, chunk_size, |metadata, chunk| {
        check_cancelled(&cancelled)?;
        if metadata
            .content_type
            .as_deref()
            .is_some_and(crate::xml::is_xml_mime)
        {
            xml_bytes.extend_from_slice(chunk);
            return Ok(());
        }
        let parser =
            parser.get_or_insert_with(|| HtmlByteParser::new(metadata.content_type.as_deref()));
        parser
            .feed(chunk)
            .map_err(|error| io::Error::other(format!("HTML parse failed: {error:?}")))?;
        let node_count = parser.document().node_count();
        if options.trace {
            eprintln!("[trace] {node_count} DOM nodes after {} bytes", chunk.len());
        }
        if parser.revision() != last_revision {
            on_progress(&Frame::new(metadata.url.clone(), parser.document().clone()))?;
            last_revision = parser.revision();
        }
        if let Some(delay) = options.throttle_ms {
            thread::sleep(Duration::from_millis(delay));
        }
        Ok(())
    })?;
    check_cancelled(&cancelled)?;
    if metadata
        .content_type
        .as_deref()
        .is_some_and(crate::xml::is_xml_mime)
    {
        let (document, failure) =
            match crate::xml::parse_bytes(&xml_bytes, metadata.content_type.as_deref()) {
                Ok(document) => (document, None),
                Err(failure) => {
                    eprintln!("{failure}");
                    (crate::xml::error_document(&failure), Some(failure))
                }
            };
        let mut frame = Frame::new(metadata.url, document);
        if let Some(failure) = failure {
            frame.parse_errors.push(crate::html::tokenizer::ParseError {
                code: "xml-well-formedness-error",
                position: failure.position,
            });
            frame.error = Some(failure.to_string());
        } else {
            load_resources(&mut frame, &mut on_progress, &cancelled)?;
        }
        check_cancelled(&cancelled)?;
        on_progress(&frame)?;
        return Ok(frame);
    }
    let decoded = parser
        .unwrap_or_else(|| HtmlByteParser::new(metadata.content_type.as_deref()))
        .finish()
        .map_err(|error| io::Error::other(format!("HTML parse failed: {error:?}")))?;
    if decoded.had_replacements {
        eprintln!(
            "HTML: malformed {} byte sequences replaced",
            decoded.encoding.name()
        );
    }
    let parsed = decoded.parsed;
    for error in &parsed.errors {
        eprintln!(
            "HTML at {}:{} (byte {}): {}",
            error.position.line, error.position.column, error.position.byte_offset, error.code
        );
    }
    if parsed.dropped_errors > 0 {
        eprintln!(
            "HTML: {} additional parse errors omitted",
            parsed.dropped_errors
        );
    }
    let document = parsed.document;
    if options.trace {
        eprintln!(
            "[trace] parsing finished with {} DOM nodes",
            document.node_count()
        );
    }
    let mut frame = Frame::new(metadata.url, document);
    frame.parse_errors = parsed.errors;
    frame.dropped_parse_errors = parsed.dropped_errors;
    on_progress(&frame)?;
    load_resources(&mut frame, &mut on_progress, &cancelled)?;
    check_cancelled(&cancelled)?;
    on_progress(&frame)?;
    Ok(frame)
}

pub(super) fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), Box<dyn Error>> {
    if cancelled() {
        Err(io::Error::new(io::ErrorKind::Interrupted, "page load cancelled").into())
    } else {
        Ok(())
    }
}
