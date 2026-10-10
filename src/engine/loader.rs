use std::{error::Error, io, thread, time::Duration};

use url::Url;

use crate::{html::parser::HtmlParser, net};

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
    let mut parser = HtmlParser::new();
    let mut last_node_count = parser.node_count();
    let metadata = net::stream_document(url, chunk_size, |metadata, chunk| {
        check_cancelled(&cancelled)?;
        parser
            .feed(chunk)
            .map_err(|error| io::Error::other(format!("HTML parse failed: {error:?}")))?;
        let node_count = parser.node_count();
        if options.trace {
            eprintln!("[trace] {node_count} DOM nodes after {} bytes", chunk.len());
        }
        if node_count != last_node_count {
            on_progress(&Frame::new(metadata.url.clone(), parser.document().clone()))?;
            last_node_count = node_count;
        }
        if let Some(delay) = options.throttle_ms {
            thread::sleep(Duration::from_millis(delay));
        }
        Ok(())
    })?;
    check_cancelled(&cancelled)?;
    let document = parser
        .finish()
        .map_err(|error| io::Error::other(format!("HTML parse failed: {error:?}")))?;
    if options.trace {
        eprintln!(
            "[trace] parsing finished with {} DOM nodes",
            document.node_count()
        );
    }
    let mut frame = Frame::new(metadata.url, document);
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
