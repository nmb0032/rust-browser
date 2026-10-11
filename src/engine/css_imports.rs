use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io,
    sync::Arc,
};

use crate::{
    css::{self, Rule, Stylesheet},
    dom::NodeId,
    net,
    style::{self, Diagnostic},
};

use super::{loader::check_cancelled, urls::check_page_reference};

const MAX_IMPORT_DEPTH: usize = 16;

#[derive(Clone)]
pub(super) struct CachedCss {
    pub parsed: css::ParseOutput<Arc<Stylesheet>>,
    pub url: url::Url,
    pub source: Arc<str>,
}

pub(super) struct CssLoader<'a, C> {
    pub page: &'a url::Url,
    pub cancelled: &'a C,
    pub declaration_context: style::DeclarationContext,
    pub cache: HashMap<url::Url, Result<Arc<CachedCss>, String>>,
    pub graphs: HashMap<(url::Url, Vec<url::Url>, usize), Arc<CachedCss>>,
    pub bytes: usize,
    pub graph_bytes: usize,
    pub imports: usize,
}

impl<C: Fn() -> bool> CssLoader<'_, C> {
    pub fn fetch(
        &mut self,
        url: &url::Url,
        node: NodeId,
        diagnostics: &mut Vec<Diagnostic>,
        path: &mut HashSet<url::Url>,
        depth: usize,
    ) -> Result<Arc<CachedCss>, Box<dyn Error>> {
        check_cancelled(self.cancelled)?;
        check_page_reference(self.page, url)?;
        if depth > MAX_IMPORT_DEPTH {
            return Err(io::Error::other("CSS import depth exceeds 16").into());
        }
        if path.contains(url) {
            return Err(io::Error::other(format!("CSS import cycle: {url}")).into());
        }
        let raw = if let Some(cached) = self.cache.get(url) {
            cached.clone().map_err(io::Error::other)?
        } else {
            let result = self.fetch_uncached(url);
            check_cancelled(self.cancelled)?;
            self.cache.insert(
                url.clone(),
                result.as_ref().map(Arc::clone).map_err(ToString::to_string),
            );
            if let Ok(sheet) = &result {
                self.cache.insert(sheet.url.clone(), Ok(Arc::clone(sheet)));
            }
            result?
        };
        if path.contains(&raw.url) {
            return Err(io::Error::other(format!("CSS import redirect cycle: {}", raw.url)).into());
        }
        let mut ancestors: Vec<_> = path.iter().cloned().collect();
        ancestors.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let key = (raw.url.clone(), ancestors, depth);
        if let Some(graph) = self.graphs.get(&key) {
            return Ok(Arc::clone(graph));
        }
        // Hydration is path-sensitive: B imported by A must not cache a
        // truncated A/B cycle as the independent top-level B stylesheet.
        self.graph_bytes = self.graph_bytes.saturating_add(raw.source.len());
        if self.graph_bytes > 64 * 1024 * 1024 {
            return Err(
                io::Error::other("retained CSS import graphs exceed 64 MiB of source").into(),
            );
        }
        let mut sheet = (*raw.parsed.value).clone();
        path.insert(url.clone());
        path.insert(raw.url.clone());
        let result = self.imports(&mut sheet, &raw.url, node, diagnostics, path, depth);
        path.remove(url);
        path.remove(&raw.url);
        result?;
        let graph = Arc::new(CachedCss {
            parsed: css::ParseOutput {
                value: Arc::new(sheet),
                errors: raw.parsed.errors.clone(),
                dropped_errors: raw.parsed.dropped_errors,
            },
            url: raw.url.clone(),
            source: Arc::clone(&raw.source),
        });
        self.graphs.insert(key, Arc::clone(&graph));
        Ok(graph)
    }

    fn fetch_uncached(&mut self, url: &url::Url) -> Result<Arc<CachedCss>, Box<dyn Error>> {
        let resource = net::fetch_resource(url, net::MAX_CSS_BYTES)?;
        check_cancelled(self.cancelled)?;
        check_page_reference(self.page, &resource.metadata.url)?;
        let mut final_url = resource.metadata.url;
        final_url.set_fragment(None);
        if resource
            .metadata
            .content_type
            .as_deref()
            .is_none_or(|mime| {
                !mime
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .eq_ignore_ascii_case("text/css")
            })
            && final_url.scheme() != "file"
        {
            return Err(io::Error::other("stylesheet response is not text/css").into());
        }
        self.bytes = self.bytes.saturating_add(resource.bytes.len());
        if self.bytes > 16 * 1024 * 1024 {
            return Err(io::Error::other("page CSS exceeds 16 MiB").into());
        }
        let source = std::str::from_utf8(&resource.bytes)?;
        let source: Arc<str> = Arc::from(source.strip_prefix('\u{feff}').unwrap_or(source));
        let parsed = css::parse_recovering(&source);
        Ok(Arc::new(CachedCss {
            parsed: css::ParseOutput {
                value: Arc::new(parsed.value),
                errors: parsed.errors,
                dropped_errors: parsed.dropped_errors,
            },
            url: final_url,
            source,
        }))
    }

    pub fn imports(
        &mut self,
        sheet: &mut Stylesheet,
        base: &url::Url,
        node: NodeId,
        diagnostics: &mut Vec<Diagnostic>,
        path: &mut HashSet<url::Url>,
        depth: usize,
    ) -> Result<(), Box<dyn Error>> {
        let mut allowed = true;
        let mut preamble_started = false;
        for rule in sheet.rules_mut() {
            let Rule::At(rule) = rule else {
                allowed = false;
                continue;
            };
            if rule.name().eq_ignore_ascii_case("charset") && rule.block().is_none() {
                continue;
            }
            if rule.name().eq_ignore_ascii_case("layer") && rule.block().is_none() {
                if preamble_started {
                    allowed = false;
                }
                continue;
            }
            if !rule.name().eq_ignore_ascii_case("import") {
                allowed = false;
                continue;
            }
            if !allowed || rule.block().is_some() {
                continue;
            }
            let Some(import) = css::control::import(rule.prelude()) else {
                continue;
            };
            preamble_started = true;
            if !style::import_supported(&import, self.declaration_context) {
                continue;
            }
            self.imports += 1;
            if self.imports > 128 {
                diagnostics.push(Diagnostic {
                    node_id: Some(node),
                    message: "page CSS import count exceeds 128".into(),
                });
                break;
            }
            check_cancelled(self.cancelled)?;
            let result = (|| {
                let mut url = base.join(&import.url)?;
                url.set_fragment(None);
                self.fetch(&url, node, diagnostics, path, depth + 1)
            })();
            check_cancelled(self.cancelled)?;
            match result {
                Ok(loaded) => {
                    style::report_css_errors(
                        &loaded.parsed,
                        node,
                        &format!("import {}: ", loaded.url),
                        diagnostics,
                    );
                    rule.imported = Some(css::ImportedStylesheet {
                        stylesheet: Arc::clone(&loaded.parsed.value),
                        url: loaded.url.clone(),
                        source: Arc::clone(&loaded.source),
                    });
                }
                Err(error) => diagnostics.push(Diagnostic {
                    node_id: Some(node),
                    message: format!("import {:?} from {base}: {error}", import.url),
                }),
            }
        }
        Ok(())
    }
}
