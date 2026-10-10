use std::{
    error::Error,
    fs,
    io::{self, Read},
    path::PathBuf,
};
use url::Url;

pub fn fetch_html(location: &str) -> Result<String, Box<dyn Error>> {
    match uri_scheme(location) {
        Some(scheme)
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            fetch_html_from_url(location)
        }
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => fetch_html_from_path(location),
        Some(scheme) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported URL scheme: {scheme}"),
        )
        .into()),
        None => fetch_html_from_path(location),
    }
}

pub fn stream_html(
    location: &str,
    chunk_size: usize,
    mut on_chunk: impl FnMut(&str) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    match uri_scheme(location) {
        Some(scheme)
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            let mut response = ureq::get(location).call()?;
            stream_reader(response.body_mut().as_reader(), chunk_size, &mut on_chunk)
        }
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let file_path = file_path(location)?;
            stream_reader(fs::File::open(file_path)?, chunk_size, &mut on_chunk)
        }
        Some(scheme) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported URL scheme: {scheme}"),
        )
        .into()),
        None => stream_reader(fs::File::open(location)?, chunk_size, &mut on_chunk),
    }
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

fn uri_scheme(location: &str) -> Option<&str> {
    let (scheme, rest) = location.split_once(':')?;
    let mut characters = scheme.chars();
    let first = characters.next()?;

    if !first.is_ascii_alphabetic()
        || !characters.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        || (scheme.len() == 1 && (rest.starts_with('/') || rest.starts_with('\\')))
    {
        return None;
    }

    Some(scheme)
}

fn fetch_html_from_url(url: &str) -> Result<String, Box<dyn Error>> {
    let mut response = ureq::get(url).call()?;
    Ok(response.body_mut().read_to_string()?)
}

fn fetch_html_from_path(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(file_path(path)?)?)
}

fn file_path(path: &str) -> Result<PathBuf, Box<dyn Error>> {
    if path.starts_with("file://") {
        Url::parse(path)?
            .to_file_path()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file URL").into())
    } else {
        Ok(PathBuf::from(path))
    }
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
