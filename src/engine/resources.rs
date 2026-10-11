use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io,
    sync::Arc,
};

use crate::{
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

pub(super) fn load_resources(
    frame: &mut Frame,
    on_progress: &mut impl FnMut(&Frame) -> Result<(), Box<dyn Error>>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), Box<dyn Error>> {
    let base = document_base_url(&frame.document, &frame.url);
    let page_url = frame.url.clone();
    let policy = frame.resource_policy.clone();
    let mut external = HashMap::new();
    let mut image_cache = HashMap::new();
    let mut css_loader = super::css_imports::CssLoader {
        page: &page_url,
        policy: &policy,
        cancelled,
        declaration_context: style::DeclarationContext::for_document(&frame.document),
        cache: HashMap::new(),
        graphs: HashMap::new(),
        bytes: 0,
        graph_bytes: 0,
        imports: 0,
    };
    let mut diagnostics = Vec::new();
    let mut resource_count = 0;
    let mut image_bytes = 0;
    // Retain embedded ASTs as well: every progressive update and resize reuses
    // their loaded import graph rather than reparsing or fetching again.
    let embedded = style::collect_stylesheets(&frame.document);
    diagnostics.extend(embedded.diagnostics);
    for mut sheet in embedded.stylesheets {
        css_loader.imports(
            Arc::make_mut(&mut sheet.stylesheet),
            &base,
            sheet.node_id,
            &mut diagnostics,
            &mut HashSet::new(),
            0,
        )?;
        external.insert(sheet.node_id, sheet);
    }
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
            element.attribute("href")
        } else if element.tag_name().eq_ignore_ascii_case("img") {
            element.attribute("src")
        } else if element.tag_name().eq_ignore_ascii_case("video") {
            element.attribute("poster")
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
            frame.resource_policy.check(&url)?;
            url.set_fragment(None);
            if is_sheet {
                let sheet = css_loader.fetch(&url, id, &mut diagnostics, &mut HashSet::new(), 0)?;
                frame.external_stylesheet_sources.insert(
                    id,
                    StylesheetSource {
                        source: Arc::clone(&sheet.source),
                        url: sheet.url.clone(),
                    },
                );
                style::report_css_errors(
                    &sheet.parsed,
                    id,
                    &format!("resource {reference:?} ({}): ", sheet.url),
                    &mut diagnostics,
                );
                external.insert(
                    id,
                    AuthorStylesheet {
                        node_id: id,
                        stylesheet: Arc::clone(&sheet.parsed.value),
                        url: Some(sheet.url.clone()),
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
                    check_cancelled(cancelled)?;
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
        super::web_fonts::load(frame, on_progress, cancelled)?;
    }
    let mut stylesheets = style::collect_stylesheets_with_external(&frame.document, &external);
    stylesheets.diagnostics.extend(diagnostics);
    frame.set_stylesheets(stylesheets);
    super::web_fonts::load(frame, on_progress, cancelled)?;
    let mut marker_diagnostics = Vec::new();
    let mut marker_bytes = 0usize;
    for id in frame.document.preorder().collect::<Vec<_>>() {
        check_cancelled(cancelled)?;
        let Some(style) = frame.styles.get(id).filter(|s| s.display.is_list_item()) else {
            continue;
        };
        let style::typography::Value::Image(Some(reference)) =
            style.typography.get(style::typography::Property::ListImage)
        else {
            continue;
        };
        resource_count += 1;
        if resource_count > MAX_RESOURCES {
            diagnostic(
                &mut marker_diagnostics,
                id,
                "marker image resource count exceeds 128; text fallback".into(),
            );
            break;
        }
        let result = (|| -> Result<Arc<DecodedImage>, Box<dyn Error>> {
            let url = base.join(&reference)?;
            check_page_reference(&frame.url, &url)?;
            frame.resource_policy.check(&url)?;
            let resource = net::fetch_resource(&url, net::MAX_IMAGE_BYTES)?;
            frame.resource_policy.check(&resource.metadata.url)?;
            check_page_reference(&frame.url, &resource.metadata.url)?;
            let image = Arc::new(DecodedImage::decode(&resource.bytes)?);
            marker_bytes += image.rgba().len();
            if image_bytes + marker_bytes > MAX_PAGE_IMAGE_BYTES {
                return Err(io::Error::other("marker decoded image byte limit").into());
            }
            Ok(image)
        })();
        check_cancelled(cancelled)?;
        match result {
            Ok(image) => {
                frame.marker_images.insert(id, image);
                on_progress(frame)?;
            }
            Err(error) => diagnostic(
                &mut marker_diagnostics,
                id,
                format!("marker image: {error}; text fallback"),
            ),
        }
    }
    if !marker_diagnostics.is_empty() {
        let mut sheets = frame.stylesheets.clone();
        sheets.diagnostics.extend(marker_diagnostics);
        frame.set_stylesheets(sheets);
    }
    frame.image_cache = image_cache;
    frame.image_resource_count = resource_count;
    frame.image_resource_bytes = image_bytes + marker_bytes;
    load_effect_images(frame, on_progress, cancelled)?;
    check_cancelled(cancelled)?;
    frame.web_fonts.ready = true;
    Ok(())
}

pub(super) fn load_effect_images(
    frame: &mut Frame,
    on_progress: &mut impl FnMut(&Frame) -> Result<(), Box<dyn Error>>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), Box<dyn Error>> {
    let base = document_base_url(&frame.document, &frame.url);
    let mut effect_diagnostics = Vec::new();
    for id in frame.document.preorder().collect::<Vec<_>>() {
        let mut references = frame
            .styles
            .get(id)
            .map(|s| s.effects.urls())
            .unwrap_or_default();
        for pseudo in style::PseudoElement::ALL {
            if let Some(s) = frame.styles.pseudo(id, pseudo) {
                references.extend(s.effects.urls());
            }
        }
        for reference in references {
            check_cancelled(cancelled)?;
            if frame.effect_images.contains_key(&reference)
                || frame.failed_effect_images.contains(&reference)
            {
                continue;
            }
            frame.image_resource_count += 1;
            if frame.image_resource_count > MAX_RESOURCES {
                frame.failed_effect_images.insert(reference);
                diagnostic(
                    &mut effect_diagnostics,
                    id,
                    "mask resource count exceeds 128; transparent mask fallback".into(),
                );
                break;
            }
            let result = (|| -> Result<Arc<DecodedImage>, Box<dyn Error>> {
                let mut url = base.join(&reference)?;
                check_page_reference(&frame.url, &url)?;
                frame.resource_policy.check(&url)?;
                url.set_fragment(None);
                if let Some(image) = frame.image_cache.get(&url) {
                    return Ok(Arc::clone(image));
                }
                let resource = net::fetch_resource(&url, net::MAX_IMAGE_BYTES)?;
                frame.resource_policy.check(&resource.metadata.url)?;
                check_page_reference(&frame.url, &resource.metadata.url)?;
                let image = Arc::new(DecodedImage::decode(&resource.bytes)?);
                frame.image_resource_bytes += image.rgba().len();
                if frame.image_resource_bytes > MAX_PAGE_IMAGE_BYTES {
                    return Err(io::Error::other("mask decoded image byte limit").into());
                }
                frame.image_cache.insert(url, image.clone());
                Ok(image)
            })();
            match result {
                Ok(image) => {
                    frame.effect_images.insert(reference, image);
                    on_progress(frame)?;
                }
                Err(error) => {
                    frame.failed_effect_images.insert(reference);
                    diagnostic(
                        &mut effect_diagnostics,
                        id,
                        format!("mask image: {error}; transparent mask fallback"),
                    );
                }
            }
        }
    }
    if !effect_diagnostics.is_empty() {
        let mut sheets = frame.stylesheets.clone();
        sheets.diagnostics.extend(effect_diagnostics);
        frame.set_stylesheets(sheets);
        on_progress(frame)?;
    }
    check_cancelled(cancelled)?;
    Ok(())
}

fn diagnostic(diagnostics: &mut Vec<Diagnostic>, node_id: NodeId, message: String) {
    diagnostics.push(Diagnostic {
        node_id: Some(node_id),
        message,
    });
}
