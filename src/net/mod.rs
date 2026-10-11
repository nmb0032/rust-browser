//! Bounded HTTP/file loading with a GET-only redirect loop, not yet full Fetch.

mod client;
mod location;

pub(crate) use client::fetch_font;
pub use client::{
    FetchError, MAX_CSS_BYTES, MAX_FONT_BYTES, MAX_HTML_BYTES, MAX_IMAGE_BYTES, Resource,
    ResponseMetadata, fetch_resource,
};
pub use location::location_url;

use std::{
    error::Error,
    io::{self, Read},
};
use url::Url;

pub fn fetch_html(location: &str) -> Result<String, Box<dyn Error>> {
    let mut source = String::new();
    stream_html(location, 8192, |chunk| {
        source.push_str(chunk);
        Ok(())
    })?;
    Ok(source)
}

pub fn stream_html(
    location: &str,
    chunk_size: usize,
    mut on_chunk: impl FnMut(&str) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    stream_document(&location_url(location)?, chunk_size, |_, chunk| {
        on_chunk(chunk)
    })
    .map(|_| ())
}

pub fn stream_document(
    url: &Url,
    chunk_size: usize,
    mut on_chunk: impl FnMut(&ResponseMetadata, &str) -> Result<(), Box<dyn Error>>,
) -> Result<ResponseMetadata, Box<dyn Error>> {
    client::with_response(url, MAX_HTML_BYTES, |metadata, reader| {
        stream_reader(reader, chunk_size, &mut |chunk| on_chunk(metadata, chunk))?;
        Ok(metadata.clone())
    })
}

pub fn stream_document_bytes(
    url: &Url,
    chunk_size: usize,
    mut on_chunk: impl FnMut(&ResponseMetadata, &[u8]) -> Result<(), Box<dyn Error>>,
) -> Result<ResponseMetadata, Box<dyn Error>> {
    client::with_response(url, MAX_HTML_BYTES, |metadata, reader| {
        let mut buffer = vec![0; chunk_size.max(1)];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            on_chunk(metadata, &buffer[..count])?;
        }
        Ok(metadata.clone())
    })
}

fn stream_reader(
    mut reader: impl Read,
    chunk_size: usize,
    on_chunk: &mut impl FnMut(&str) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let mut read_buffer = vec![0; chunk_size.max(1)];
    let mut pending = Vec::new();

    loop {
        let bytes_read = reader.read(&mut read_buffer)?;
        if bytes_read == 0 {
            break;
        }
        pending.extend_from_slice(&read_buffer[..bytes_read]);

        let valid_bytes = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(error) => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, error).into());
            }
        };

        if valid_bytes > 0 {
            let text = std::str::from_utf8(&pending[..valid_bytes])?;
            on_chunk(text)?;
            pending.drain(..valid_bytes);
        }
    }

    if !pending.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "response ended in the middle of a UTF-8 character",
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{fetch_html, stream_html, stream_reader};
    use std::{
        error::Error,
        io::{self, BufRead, BufReader, Cursor, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    #[test]
    fn rejects_unsupported_url_schemes() {
        for location in ["ftp://example.com/page.html", "FTP://example.com/page.html"] {
            let error = fetch_html(location).unwrap_err();
            assert!(error.to_string().contains("unsupported URL scheme:"));
            let error = stream_html(location, 8192, |_| Ok(())).unwrap_err();
            assert!(error.to_string().contains("unsupported URL scheme:"));
        }
    }

    #[test]
    fn decodes_utf8_across_byte_boundaries() {
        let source = "A café 世界";
        for chunk_size in 1..=source.len() {
            let mut chunks = Vec::new();
            stream_reader(Cursor::new(source.as_bytes()), chunk_size, &mut |chunk| {
                chunks.push(chunk.to_string());
                Ok::<(), Box<dyn Error>>(())
            })
            .unwrap();

            assert_eq!(chunks.concat(), source, "chunk size {chunk_size}");
        }
    }

    #[test]
    fn streams_http_before_end_of_body() {
        for chunked in [false, true] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let location = format!("http://{}/", listener.local_addr().unwrap());
            let first = "<p>first</p>";
            let last = "<p>café 世界</p>";
            let (release, wait_for_chunk) = mpsc::channel();
            let server = thread::spawn(move || -> io::Result<()> {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                let mut request = BufReader::new(&stream);
                loop {
                    let mut line = String::new();
                    if request.read_line(&mut line)? == 0 {
                        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                if chunked {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\
                         Connection: close\r\n\r\n{:x}\r\n{first}\r\n",
                        first.len()
                    )?;
                } else {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{first}",
                        first.len() + last.len()
                    )?;
                }
                wait_for_chunk
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(io::Error::other)?;
                if chunked {
                    write!(stream, "{:x}\r\n{last}\r\n0\r\n\r\n", last.len())
                } else {
                    stream.write_all(last.as_bytes())
                }
            });

            let mut received = String::new();
            let mut release = Some(release);
            let result = stream_html(&location, 8192, |chunk| {
                received.push_str(chunk);
                if received == first {
                    release.take().unwrap().send(())?;
                }
                Ok(())
            });
            server.join().unwrap().unwrap();
            result.unwrap();
            assert_eq!(received, format!("{first}{last}"), "chunked: {chunked}");
        }
    }

    #[test]
    fn streams_file_urls() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages/basic.html");
        let location = url::Url::from_file_path(path).unwrap();
        let mut received = String::new();
        stream_html(location.as_str(), 1, |chunk| {
            received.push_str(chunk);
            Ok(())
        })
        .unwrap();

        assert_eq!(received, include_str!("../../tests/pages/basic.html"));
    }

    #[test]
    fn empty_input_does_not_emit_chunks() {
        let mut calls = 0;
        stream_reader(Cursor::new([]), 8192, &mut |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();

        assert_eq!(calls, 0);
    }

    #[test]
    fn propagates_chunk_consumer_errors() {
        let mut calls = 0;
        let result = stream_reader(Cursor::new(b"some text"), 1, &mut |_| {
            calls += 1;
            Err(io::Error::other("consumer stopped").into())
        });

        assert_eq!(result.unwrap_err().to_string(), "consumer stopped");
        assert_eq!(calls, 1);
    }

    #[test]
    fn rejects_invalid_or_truncated_utf8() {
        let invalid = stream_reader(
            Cursor::new([0xFF]),
            1,
            &mut |_| Ok::<(), Box<dyn Error>>(()),
        );
        assert_eq!(
            invalid.unwrap_err().to_string(),
            "invalid utf-8 sequence of 1 bytes from index 0"
        );

        let truncated = stream_reader(
            Cursor::new([0xC3]),
            1,
            &mut |_| Ok::<(), Box<dyn Error>>(()),
        );
        assert_eq!(
            truncated.unwrap_err().to_string(),
            "response ended in the middle of a UTF-8 character"
        );
    }
}
