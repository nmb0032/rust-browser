use std::{error::Error, io, path::Path};

use url::Url;

/// CLI paths are converted once; page references always use URL resolution.
pub fn location_url(location: &str) -> Result<Url, Box<dyn Error>> {
    if uri_scheme(location).is_some() {
        return Ok(Url::parse(location)?);
    }
    let path = Path::new(location);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Url::from_file_path(absolute)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file path").into())
}

pub(crate) fn check_scheme(url: &Url) -> Result<(), Box<dyn Error>> {
    match url.scheme() {
        "http" | "https" | "file" => Ok(()),
        scheme => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported URL scheme: {scheme}"),
        )
        .into()),
    }
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
