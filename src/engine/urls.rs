use std::{error::Error, io};

use url::Url;

use crate::dom::{Document, NodeId};

/// HTML's first base element with an href wins, even when its URL is invalid.
/// https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url
pub fn document_base_url(document: &Document, document_url: &Url) -> Url {
    for id in document.preorder() {
        if let Some(element) = document.node(id).and_then(|node| node.as_element())
            && element.tag_name().eq_ignore_ascii_case("base")
            && let Some(href) = element.attribute("href")
        {
            return match document_url.join(href) {
                Ok(url) if !matches!(url.scheme(), "data" | "javascript") => url,
                _ => document_url.clone(),
            };
        }
    }
    document_url.clone()
}

pub fn resolve_reference(base: &Url, reference: &str) -> Result<Url, url::ParseError> {
    base.join(reference)
}

pub fn link_url(
    document: &Document,
    node_id: NodeId,
    document_url: &Url,
) -> Result<Option<Url>, Box<dyn Error>> {
    let mut current = Some(node_id);
    while let Some(id) = current {
        let Some(node) = document.node(id) else {
            break;
        };
        if let Some(element) = node.as_element()
            && element.tag_name().eq_ignore_ascii_case("a")
            && let Some(href) = element.attribute("href")
        {
            let url = resolve_reference(&document_base_url(document, document_url), href)?;
            check_page_reference(document_url, &url)?;
            return Ok(Some(url));
        }
        current = node.parent();
    }
    Ok(None)
}

pub(super) fn check_page_reference(document_url: &Url, url: &Url) -> Result<(), Box<dyn Error>> {
    if !matches!(url.scheme(), "http" | "https" | "file")
        || (url.scheme() == "file" && document_url.scheme() != "file")
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("blocked page URL: {url}"),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::parser;

    #[test]
    fn resolves_relative_absolute_query_fragment_and_file_references() {
        let base = Url::parse("https://example.org/docs/page.html?old=1").unwrap();
        for (reference, expected) in [
            ("../next", "https://example.org/next"),
            ("/root", "https://example.org/root"),
            ("//other.org/a", "https://other.org/a"),
            ("https://other.org/", "https://other.org/"),
            ("?new=2", "https://example.org/docs/page.html?new=2"),
            ("#part", "https://example.org/docs/page.html?old=1#part"),
            ("", "https://example.org/docs/page.html?old=1"),
            ("a b", "https://example.org/docs/a%20b"),
        ] {
            assert_eq!(
                resolve_reference(&base, reference).unwrap().as_str(),
                expected
            );
        }
        let file = Url::parse("file:///tmp/site/index.html").unwrap();
        assert_eq!(
            file.join("image.png").unwrap().as_str(),
            "file:///tmp/site/image.png"
        );
    }

    #[test]
    fn first_base_href_wins_and_invalid_base_uses_document_url() {
        let url = Url::parse("https://example.org/docs/page").unwrap();
        for (source, expected) in [
            (
                "<base><base href='../assets/'><base href='/ignored/'>",
                "https://example.org/assets/",
            ),
            (
                "<base href='http://['><base href='/ignored/'>",
                url.as_str(),
            ),
            ("<base href='javascript:alert(1)'>", url.as_str()),
            ("<base href=''>", url.as_str()),
        ] {
            assert_eq!(
                document_base_url(&parser::parse(source).unwrap(), &url).as_str(),
                expected
            );
        }
    }

    #[test]
    fn enclosing_anchor_uses_base_and_blocks_local_file_access_from_http() {
        let document = parser::parse("<base href='/assets/'><a href='next'><b>go</b></a>").unwrap();
        let text = document.preorder().last().unwrap();
        let url = Url::parse("https://example.org/page").unwrap();
        assert_eq!(
            link_url(&document, text, &url).unwrap().unwrap().as_str(),
            "https://example.org/assets/next"
        );
        assert!(check_page_reference(&url, &Url::parse("file:///tmp/private").unwrap()).is_err());
    }
}
