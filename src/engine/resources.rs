use std::{collections::HashMap, error::Error, io, sync::Arc};

use crate::{
    css,
    dom::{NodeId, NodeKind},
    media::DecodedImage,
    net,
    style::{self, AuthorStylesheet, Diagnostic},
};

use super::{
    Frame, StylesheetSource, document_base_url, loader::check_cancelled, urls::check_page_reference,
};

const MAX_RESOURCES: usize = 128;
const MAX_PAGE_IMAGE_BYTES: usize = 128 * 1024 * 1024;
const MAX_PAGE_CSS_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn load_resources(
    frame: &mut Frame,
    on_progress: &mut impl FnMut(&Frame) -> Result<(), Box<dyn Error>>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), Box<dyn Error>> {
    let base = document_base_url(&frame.document, &frame.url);
    let mut external = HashMap::new();
    let mut image_cache = HashMap::new();
    let mut css_cache = HashMap::new();
    let mut diagnostics = Vec::new();
    let mut resource_count = 0;
    let mut image_bytes = 0;
    let mut css_bytes = 0;
    let ids = frame.document.preorder().collect::<Vec<_>>();
    for id in ids {
        check_cancelled(cancelled)?;
        let Some(NodeKind::Element(element)) = frame.document.node(id).map(|node| node.kind())
        else {
            continue;
        };
        let is_sheet = element.tag_name().eq_ignore_ascii_case("link")
            && element.attribute("rel").is_some_and(|rel| {
                rel.split_ascii_whitespace()
                    .any(|token| token.eq_ignore_ascii_case("stylesheet"))
                    && !rel
                        .split_ascii_whitespace()
                        .any(|token| token.eq_ignore_ascii_case("alternate"))
            })
            && element.attribute("disabled").is_none();
        let reference = if is_sheet {
            if element.attribute("type").is_some_and(|value| {
                !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("text/css")
            }) {
                continue;
            }
            if element.attribute("media").is_some_and(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "" | "all" | "screen"
                )
            }) {
                diagnostic(
                    &mut diagnostics,
                    id,
                    "unsupported or non-screen stylesheet media".into(),
                );
                continue;
            }
            element.attribute("href")
        } else if element.tag_name().eq_ignore_ascii_case("img") {
            element.attribute("src")
        } else {
            continue;
        };
        let Some(reference) = reference else { continue };
        if reference.trim().is_empty() {
            diagnostic(&mut diagnostics, id, "empty resource URL".into());
            continue;
        }
        resource_count += 1;
        if resource_count > MAX_RESOURCES {
            diagnostic(
                &mut diagnostics,
                id,
                "page resource count exceeds 128".into(),
            );
            break;
        }
        let result = (|| -> Result<(), Box<dyn Error>> {
            let mut url = base.join(reference)?;
            check_page_reference(&frame.url, &url)?;
            url.set_fragment(None);
            if is_sheet {
                let sheet = if let Some(sheet) = css_cache.get(&url) {
                    Arc::clone(sheet)
                } else {
                    let resource = net::fetch_resource(&url, net::MAX_CSS_BYTES)?;
                    if resource.metadata.url.scheme() != "file"
                        && !resource
                            .metadata
                            .content_type
                            .as_deref()
                            .is_some_and(|mime| {
                                mime.split(';')
                                    .next()
                                    .unwrap_or_default()
                                    .trim()
                                    .eq_ignore_ascii_case("text/css")
                            })
                    {
                        return Err(io::Error::other("stylesheet response is not text/css").into());
                    }
                    css_bytes += resource.bytes.len();
                    if css_bytes > MAX_PAGE_CSS_BYTES {
                        return Err(io::Error::other("page CSS exceeds 16 MiB").into());
                    }
                    let source = std::str::from_utf8(&resource.bytes)?;
                    let source: Arc<str> =
                        Arc::from(source.strip_prefix('\u{feff}').unwrap_or(source));
                    frame.external_stylesheet_sources.insert(
                        id,
                        StylesheetSource {
                            source: Arc::clone(&source),
                            url: resource.metadata.url.clone(),
                        },
                    );
                    let sheet = Arc::new((
                        Arc::new(css::parse(&source)?),
                        resource.metadata.url,
                        source,
                    ));
                    css_cache.insert(url, Arc::clone(&sheet));
                    sheet
                };
                frame.external_stylesheet_sources.insert(
                    id,
                    StylesheetSource {
                        source: Arc::clone(&sheet.2),
                        url: sheet.1.clone(),
                    },
                );
                external.insert(
                    id,
                    AuthorStylesheet {
                        node_id: id,
                        stylesheet: Arc::clone(&sheet.0),
                        url: Some(sheet.1.clone()),
                    },
                );
            } else {
                let image = if let Some(image) = image_cache.get(&url) {
                    Arc::clone(image)
                } else {
                    if image_bytes >= MAX_PAGE_IMAGE_BYTES {
                        return Err(io::Error::other("page decoded images exceed 128 MiB").into());
                    }
                    let resource = net::fetch_resource(&url, net::MAX_IMAGE_BYTES)?;
                    let image = Arc::new(DecodedImage::decode(&resource.bytes)?);
                    let next_bytes = image_bytes + image.rgba().len();
                    if next_bytes > MAX_PAGE_IMAGE_BYTES {
                        return Err(io::Error::other("page decoded images exceed 128 MiB").into());
                    }
                    image_bytes = next_bytes;
                    image_cache.insert(url, Arc::clone(&image));
                    image
                };
                frame.images.insert(id, image);
            }
            Ok(())
        })();
        check_cancelled(cancelled)?;
        if let Err(error) = result {
            diagnostic(
                &mut diagnostics,
                id,
                format!("resource {reference:?}: {error}"),
            );
        }
        let mut stylesheets = style::collect_stylesheets_with_external(&frame.document, &external);
        stylesheets.diagnostics.extend(diagnostics.iter().cloned());
        frame.set_stylesheets(stylesheets);
        on_progress(frame)?;
    }
    let mut stylesheets = style::collect_stylesheets_with_external(&frame.document, &external);
    stylesheets.diagnostics.extend(diagnostics);
    frame.set_stylesheets(stylesheets);
    Ok(())
}

fn diagnostic(diagnostics: &mut Vec<Diagnostic>, node_id: NodeId, message: String) {
    diagnostics.push(Diagnostic {
        node_id: Some(node_id),
        message,
    });
}
