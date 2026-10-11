use std::{
    error::Error,
    fmt,
    fs::File,
    io::{self, Read},
    time::{Duration, Instant},
};

use url::Url;

use super::location::check_scheme;

pub const MAX_HTML_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_CSS_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_FONT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ResponseMetadata {
    pub url: Url,
    pub content_type: Option<String>,
}

pub struct Resource {
    pub metadata: ResponseMetadata,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct FetchError {
    pub url: Url,
    source: Box<dyn Error>,
}

impl fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.url, self.source)
    }
}

impl Error for FetchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

fn fetch_error(url: &Url, source: impl Into<Box<dyn Error>>) -> FetchError {
    FetchError {
        url: url.clone(),
        source: source.into(),
    }
}

pub fn fetch_resource(url: &Url, limit: u64) -> Result<Resource, Box<dyn Error>> {
    with_response(url, limit, |metadata, reader| {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(Resource {
            metadata: metadata.clone(),
            bytes,
        })
    })
}

pub(crate) fn with_response<T>(
    url: &Url,
    limit: u64,
    on_response: impl FnOnce(&ResponseMetadata, &mut dyn Read) -> Result<T, Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    with_response_policy(url, limit, None, on_response)
}

/// Anonymous font fetch: CORS, mixed-content/local-file restrictions and
/// cancellation at each redirect and each body chunk. Not a full Fetch API.
pub(crate) fn fetch_font(
    url: &Url,
    page: &Url,
    cancelled: &impl Fn() -> bool,
) -> Result<Resource, Box<dyn Error>> {
    with_response_policy(
        url,
        MAX_FONT_BYTES,
        Some((page, cancelled)),
        |metadata, reader| {
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                font_cancelled(cancelled)?;
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            Ok(Resource {
                metadata: metadata.clone(),
                bytes,
            })
        },
    )
}

fn font_cancelled(cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "page load cancelled",
        ))
    } else {
        Ok(())
    }
}

fn check_font_url(page: &Url, url: &Url) -> io::Result<()> {
    if !matches!(url.scheme(), "http" | "https" | "file")
        || (url.scheme() == "file" && page.scheme() != "file")
        || (page.scheme() == "https" && url.scheme() == "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "blocked font URL (scheme, credentials, local file or mixed content)",
        ));
    }
    Ok(())
}

fn with_response_policy<T>(
    url: &Url,
    limit: u64,
    font: Option<(&Url, &dyn Fn() -> bool)>,
    on_response: impl FnOnce(&ResponseMetadata, &mut dyn Read) -> Result<T, Box<dyn Error>>,
) -> Result<T, Box<dyn Error>> {
    if let Some((page, cancelled)) = font {
        font_cancelled(cancelled)?;
        check_font_url(page, url)?;
    }
    check_scheme(url)?;
    if url.scheme() == "file" {
        let path = url
            .to_file_path()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file URL"))?;
        let content_type = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "xml" => Some("application/xml"),
                "xhtml" | "xht" => Some("application/xhtml+xml"),
                "svg" => Some("image/svg+xml"),
                "html" | "htm" => Some("text/html"),
                "css" => Some("text/css"),
                _ => None,
            })
            .map(str::to_owned);
        let mut reader = LimitedReader {
            inner: File::open(path)?,
            remaining: limit,
        };
        return on_response(
            &ResponseMetadata {
                url: url.clone(),
                content_type,
            },
            &mut reader,
        );
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut final_url = url.clone();
    let mut redirects = 0;
    let mut response = loop {
        if let Some((page, cancelled)) = font {
            font_cancelled(cancelled)?;
            check_font_url(page, &final_url)?;
        }
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request timed out"))?;
        let mut request_url = final_url.clone();
        request_url.set_fragment(None);
        let mut request = ureq::get(request_url.as_str());
        if let Some((page, _)) = font {
            request = request.header("Origin", page.origin().ascii_serialization());
        }
        let response = request
            .config()
            .timeout_global(Some(timeout))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|error| fetch_error(&final_url, error))?;
        if let Some((page, cancelled)) = font {
            font_cancelled(cancelled)?;
            if page.origin() != final_url.origin() {
                let origin = page.origin().ascii_serialization();
                let allowed = response
                    .headers()
                    .get("access-control-allow-origin")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v == "*" || v == origin);
                if !allowed {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "font CORS response denied",
                    )
                    .into());
                }
            }
        }
        if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            if redirects == 10 {
                return Err(io::Error::other("too many redirects (limit 10)").into());
            }
            let location = response
                .headers()
                .get("location")
                .ok_or_else(|| io::Error::other("redirect response has no Location header"))?
                .to_str()?;
            let mut next = final_url.join(location)?;
            // Fetch's HTTP-redirect fetch preserves the fragment only when absent.
            if next.fragment().is_none() {
                next.set_fragment(final_url.fragment());
            }
            if !matches!(next.scheme(), "http" | "https") {
                return Err(io::Error::other(format!("blocked redirect URL: {next}")).into());
            }
            final_url = next;
            redirects += 1;
            continue;
        }
        if !response.status().is_success() {
            return Err(fetch_error(
                &final_url,
                io::Error::other(format!("HTTP {}", response.status())),
            )
            .into());
        }
        break response;
    };
    let metadata = ResponseMetadata {
        url: final_url,
        content_type: response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()?,
    };
    let mut reader = LimitedReader {
        inner: response.body_mut().as_reader(),
        remaining: limit,
    };
    on_response(&metadata, &mut reader)
        .map_err(|error| Box::new(fetch_error(&metadata.url, error)) as Box<dyn Error>)
}

struct LimitedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::other("resource exceeds byte limit")),
            };
        }
        let length = (buffer.len() as u64).min(self.remaining) as usize;
        let count = self.inner.read(&mut buffer[..length])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn byte_limit_accepts_exact_size_and_rejects_overflow() {
        for (source, succeeds) in [(b"abc".as_slice(), true), (b"abcd".as_slice(), false)] {
            let mut reader = LimitedReader {
                inner: Cursor::new(source),
                remaining: 3,
            };
            let mut bytes = Vec::new();
            assert_eq!(reader.read_to_end(&mut bytes).is_ok(), succeeds);
            assert_eq!(bytes, b"abc");
        }
    }

    #[test]
    fn font_policy_blocks_mixed_content_credentials_and_local_file_transitions_before_request() {
        let page = Url::parse("https://font.test/").unwrap();
        for reference in [
            "http://font.test/font.ttf",
            "file:///private/font.ttf",
            "https://name:password@font.test/font.ttf",
            "ftp://font.test/font.ttf",
        ] {
            assert!(fetch_font(&Url::parse(reference).unwrap(), &page, &|| false).is_err());
        }
        let cancelled = fetch_font(&page, &page, &|| true).err().unwrap();
        assert!(cancelled.to_string().contains("cancelled"));
    }
}
