use std::{error::Error, fs, io, path::PathBuf};
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
    let file_path = if path.starts_with("file://") {
        Url::parse(path)?
            .to_file_path()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file URL"))?
    } else {
        PathBuf::from(path)
    };

    Ok(fs::read_to_string(file_path)?)
}

#[cfg(test)]
mod tests {
    use super::fetch_html;

    #[test]
    fn rejects_unsupported_url_schemes() {
        for location in ["ftp://example.com/page.html", "FTP://example.com/page.html"] {
            let error = fetch_html(location).unwrap_err();
            assert!(error.to_string().contains("unsupported URL scheme:"));
        }
    }
}
