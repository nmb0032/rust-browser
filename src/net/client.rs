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
    check_scheme(url)?;
    if url.scheme() == "file" {
        let path = url
            .to_file_path()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file URL"))?;
        let mut reader = LimitedReader {
            inner: File::open(path)?,
            remaining: limit,
        };
        return on_response(
            &ResponseMetadata {
                url: url.clone(),
                content_type: None,
            },
            &mut reader,
        );
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut final_url = url.clone();
    let mut redirects = 0;
    let mut response = loop {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request timed out"))?;
        let mut request_url = final_url.clone();
        request_url.set_fragment(None);
        let response = ureq::get(request_url.as_str())
            .config()
            .timeout_global(Some(timeout))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|error| fetch_error(&final_url, error))?;
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
}
