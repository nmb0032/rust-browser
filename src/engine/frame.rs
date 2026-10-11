use std::{collections::HashMap, sync::Arc};

use url::Url;

use crate::{
    css::media::MediaEnvironment,
    dom::{Attribute, Document, ElementData, NodeId},
    layout::{self, LayoutTree},
    media::ImageMap,
    style::{self, ComputedStyles, InteractionState, StylesheetCollection},
};

#[derive(Clone, Debug)]
pub struct StylesheetSource {
    pub source: Arc<str>,
    pub url: Url,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub url: Url,
    pub document: Document,
    pub stylesheets: StylesheetCollection,
    pub external_stylesheet_sources: HashMap<NodeId, StylesheetSource>,
    pub styles: ComputedStyles,
    pub images: ImageMap,
    pub marker_images: ImageMap,
    pub error: Option<String>,
    pub parse_errors: Vec<crate::html::tokenizer::ParseError>,
    pub dropped_parse_errors: usize,
    media_environment: MediaEnvironment,
    interaction_state: InteractionState,
    scroll_state: layout::ScrollState,
    // Keep collection/resource diagnostics separate from resolution diagnostics,
    // so repeated restyles do not append the same errors to previous results.
    collected_stylesheets: StylesheetCollection,
    fonts: crate::text::FontContext,
    pub(super) web_fonts: super::web_fonts::WebFonts,
    pub(crate) resource_policy: super::resource_policy::ResourcePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentTarget {
    Top,
    Node(NodeId),
}

impl Frame {
    pub fn new(url: Url, document: Document) -> Self {
        Self::new_with_environment(url, document, MediaEnvironment::default())
    }

    /// Build a document snapshot at an explicit CSS-pixel viewport.
    pub fn new_with_environment(
        url: Url,
        document: Document,
        media_environment: MediaEnvironment,
    ) -> Self {
        let collected_stylesheets = style::collect_stylesheets(&document);
        let resolution = style::resolve_with_stylesheets_and_environment(
            &document,
            collected_stylesheets.clone(),
            media_environment,
        );
        let mut frame = Self {
            url,
            document,
            stylesheets: resolution.stylesheets,
            external_stylesheet_sources: HashMap::new(),
            styles: resolution.styles,
            images: ImageMap::new(),
            marker_images: ImageMap::new(),
            error: None,
            parse_errors: Vec::new(),
            dropped_parse_errors: 0,
            media_environment,
            interaction_state: InteractionState::default(),
            scroll_state: layout::ScrollState::default(),
            collected_stylesheets,
            fonts: crate::text::FontContext::bundled(),
            web_fonts: super::web_fonts::WebFonts::default(),
            resource_policy: super::resource_policy::ResourcePolicy::default(),
        };
        frame.refresh_target();
        frame
    }

    pub fn media_environment(&self) -> MediaEnvironment {
        self.media_environment
    }

    pub fn interaction_state(&self) -> &InteractionState {
        &self.interaction_state
    }

    /// Snapshot-local input state; no DOM or history mutation. Reuses retained
    /// sheets and the normal cascade, then callers rebuild layout and paint.
    pub fn set_interaction_state(&mut self, state: InteractionState) -> bool {
        if state == self.interaction_state {
            return false;
        }
        self.interaction_state = state;
        self.set_stylesheets(self.collected_stylesheets.clone());
        true
    }

    pub fn refresh_target(&mut self) -> bool {
        let mut state = self.interaction_state.clone();
        state.target =
            self.url
                .fragment()
                .and_then(|fragment| match self.fragment_target(fragment) {
                    Some(FragmentTarget::Node(id)) => Some(id),
                    _ => None,
                });
        self.set_interaction_state(state)
    }

    /// Restyle the retained document and sheet ASTs without fetching resources.
    /// Returns whether it changed. Callers must rebuild layout and paint after
    /// a change. This is a full restyle, not incremental invalidation.
    pub fn set_media_environment(&mut self, environment: MediaEnvironment) -> bool {
        if self.media_environment == environment {
            return false;
        }
        self.media_environment = environment;
        self.set_stylesheets(self.collected_stylesheets.clone());
        true
    }

    pub fn fragment_target(&self, fragment: &str) -> Option<FragmentTarget> {
        if fragment.is_empty() {
            return Some(FragmentTarget::Top);
        }
        let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
        for by_id in [true, false] {
            let target = self.document.preorder().find(|id| {
                self.document
                    .node(*id)
                    .and_then(|node| node.as_element())
                    .is_some_and(|element| {
                        if by_id {
                            element.attribute("id") == Some(decoded.as_ref())
                        } else {
                            element.tag_name().eq_ignore_ascii_case("a")
                                && element.attribute("name") == Some(decoded.as_ref())
                        }
                    })
            });
            if let Some(id) = target {
                return Some(FragmentTarget::Node(id));
            }
        }
        decoded
            .eq_ignore_ascii_case("top")
            .then_some(FragmentTarget::Top)
    }

    pub fn is_same_document(&self, url: &Url) -> bool {
        let mut current = self.url.clone();
        let mut next = url.clone();
        current.set_fragment(None);
        next.set_fragment(None);
        current == next && self.error.is_none()
    }

    /// Low-level box construction from current styles. For converged container
    /// conditions/units use `layout_with_container_queries` instead.
    pub fn layout_tree(&self) -> LayoutTree {
        let mut tree = layout::build_layout_tree_with_marker_resources(
            &self.document,
            &self.styles,
            &self.images,
            &self.marker_images,
        );
        tree.restore_scroll_state(self.scroll_state.clone());
        tree
    }

    pub fn scroll_state(&self) -> &layout::ScrollState {
        &self.scroll_state
    }

    /// Adopt input state from this document's latest laid-out snapshot.
    pub fn retain_scroll_state(&mut self, tree: &LayoutTree) {
        if self.scroll_state.same_document(tree.scroll_state()) {
            self.scroll_state = tree.scroll_state().clone();
        }
    }

    pub(crate) fn copy_scroll_state_from(&mut self, frame: &Frame) {
        if self.scroll_state.same_document(&frame.scroll_state) {
            self.scroll_state = frame.scroll_state.clone();
        }
    }

    /// Resolve and lay out a fresh snapshot to bounded container-query
    /// convergence. Restarting the bootstrap on every render avoids reusing
    /// geometry from an earlier viewport, interaction state, or navigation.
    /// Retained sheets/images are reused; no external fetch occurs here.
    pub fn layout_with_container_queries(&mut self, font: &crate::text::FontContext) -> LayoutTree {
        self.layout_with_container_query_limit(font, 16)
    }

    pub(crate) fn layout_with_container_query_limit(
        &mut self,
        font: &crate::text::FontContext,
        limit: usize,
    ) -> LayoutTree {
        self.fonts = self.web_fonts.context(self, font);
        let font = self.fonts.clone();
        let mut snapshot = style::ContainerSnapshot::default();
        let mut last = None;
        for _ in 0..limit.max(1) {
            let mut sheets = self.collected_stylesheets.clone();
            sheets
                .diagnostics
                .extend(self.web_fonts.diagnostics().iter().cloned());
            let resolution = style::resolve_with_fonts(
                &self.document,
                sheets,
                self.media_environment,
                &self.interaction_state,
                &snapshot,
                &font,
            );
            self.stylesheets = resolution.stylesheets;
            self.styles = resolution.styles;
            let mut tree = self.layout_tree();
            tree.layout_viewport(
                self.media_environment.viewport_width,
                self.media_environment.viewport_height,
                &font,
            );
            let next = style::ContainerSnapshot::from_layout(&tree, self.media_environment);
            if next == snapshot {
                self.retain_scroll_state(&tree);
                return tree;
            }
            snapshot = next;
            last = Some(tree);
        }
        let message = format!(
            "container style/layout feedback did not converge within {} passes; retaining the last consistent style/layout pair (queries may be unresolved)",
            limit.max(1)
        );
        eprintln!("CSS: {message}");
        self.stylesheets.diagnostics.push(style::Diagnostic {
            node_id: None,
            message,
        });
        let tree = last.expect("at least one feedback layout pass");
        self.retain_scroll_state(&tree);
        tree
    }

    pub(super) fn set_stylesheets(&mut self, stylesheets: StylesheetCollection) {
        self.collected_stylesheets = stylesheets.clone();
        self.stylesheets = stylesheets;
        self.refresh_fonts();
    }

    /// Selected document overlay, including downloaded sources. No resource I/O.
    pub fn font_context(&self) -> &crate::text::FontContext {
        &self.fonts
    }

    /// A viewport change can activate previously unrequested fonts. Render
    /// fallback immediately, then start a separate cancellable resource phase.
    pub fn has_pending_web_fonts(&self) -> bool {
        self.web_fonts.pending(self)
    }

    pub fn load_pending_web_fonts(
        &mut self,
        mut progress: impl FnMut(&Frame) -> Result<(), Box<dyn std::error::Error>>,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        super::web_fonts::load(self, &mut progress, &cancelled)?;
        self.web_fonts.ready = true;
        Ok(())
    }

    pub(super) fn refresh_fonts(&mut self) {
        self.fonts = self.web_fonts.context(self, &self.fonts);
        let mut sheets = self.collected_stylesheets.clone();
        sheets
            .diagnostics
            .extend(self.web_fonts.diagnostics().iter().cloned());
        let resolution = style::resolve_with_fonts(
            &self.document,
            sheets,
            self.media_environment,
            &self.interaction_state,
            &style::ContainerSnapshot::default(),
            &self.fonts,
        );
        self.stylesheets = resolution.stylesheets;
        self.styles = resolution.styles;
    }

    pub fn error_page(url: Url, message: String) -> Self {
        let mut document = Document::new();
        let root = document.root_id();
        for (tag, text) in [
            ("h1", "Could not load this page".to_owned()),
            ("p", url.as_str().to_owned()),
            ("p", message.clone()),
            (
                "p",
                "Check the address or connection. Use Back to return to the previous page."
                    .to_owned(),
            ),
        ] {
            let node = document
                .append_element(root, ElementData::new(tag.into(), Vec::new()))
                .expect("new error-page elements have a valid document parent");
            document
                .append_text(node, text)
                .expect("new error-page elements can contain text");
        }
        let retry = document
            .append_element(
                root,
                ElementData::new(
                    "a".into(),
                    vec![Attribute::new("href".into(), url.to_string())],
                ),
            )
            .expect("retry link has a valid parent");
        document
            .append_text(retry, "Try again".into())
            .expect("retry link can contain text");
        let mut frame = Self::new(url, document);
        frame.error = Some(message);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dom::NodeKind, html::parser};

    #[test]
    fn fragments_decode_percent_escapes_prioritize_ids_and_fall_back_to_named_anchors() {
        let frame = Frame::new(Url::parse("https://example.org/").unwrap(),
            parser::parse("<a name='top'></a><p id='top'>target</p><p id='a b'>space</p><a name='legacy'></a>").unwrap());
        let top = frame.fragment_target("top").unwrap();
        assert!(
            matches!(top, FragmentTarget::Node(id) if frame.document.node(id).unwrap().as_element().unwrap().tag_name() == "p")
        );
        assert!(matches!(
            frame.fragment_target("a%20b"),
            Some(FragmentTarget::Node(_))
        ));
        assert!(matches!(
            frame.fragment_target("legacy"),
            Some(FragmentTarget::Node(_))
        ));
        assert_eq!(frame.fragment_target(""), Some(FragmentTarget::Top));
        assert_eq!(frame.fragment_target("TOP"), Some(FragmentTarget::Top));
        assert_eq!(frame.fragment_target("missing"), None);
    }

    #[test]
    fn error_details_are_text_not_markup_and_retry_requires_a_reload() {
        let url = Url::parse("https://example.org/page").unwrap();
        let frame = Frame::error_page(url.clone(), "<script>bad()</script>".into());
        assert!(frame.document.preorder().any(|id| matches!(frame.document.node(id).unwrap().kind(), NodeKind::Text(text) if text == "<script>bad()</script>")));
        assert!(!frame.document.preorder().any(|id| {
            frame
                .document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|element| element.tag_name() == "script")
        }));
        assert!(!frame.is_same_document(&url));
        let good = Frame::new(url.clone(), Document::new());
        assert!(good.is_same_document(&url.join("#part").unwrap()));
        assert!(!good.is_same_document(&url.join("?query").unwrap()));
    }

    #[test]
    fn restyling_keeps_ast_urls_resources_and_diagnostic_counts_stable() {
        let document = parser::parse(
            "<style>p {color:red; color:bogus; broken}\
            @media (min-width:600px) {p {color:blue}}\
            @media not (future-height:1px) {p {color:purple}}</style>\
            <p id='target' style='padding:bad'>target</p>",
        )
        .unwrap();
        let mut frame = Frame::new_with_environment(
            Url::parse("https://example.test/#target").unwrap(),
            document,
            MediaEnvironment::new(400.0, 600.0),
        );
        let target = frame
            .document
            .preorder()
            .find(|&id| {
                frame
                    .document
                    .node(id)
                    .unwrap()
                    .as_element()
                    .is_some_and(|e| e.attribute("id") == Some("target"))
            })
            .unwrap();
        let sheet = Arc::clone(&frame.stylesheets.stylesheets[0].stylesheet);
        let diagnostics = frame
            .stylesheets
            .diagnostics
            .iter()
            .map(|d| (d.node_id, d.message.clone()))
            .collect::<Vec<_>>();
        for width in [800.0, 400.0, 800.0, 400.0] {
            let environment = MediaEnvironment::new(width, 600.0);
            assert!(frame.set_media_environment(environment));
            assert!(!frame.set_media_environment(environment));
            assert_eq!(
                frame.styles.get(target).unwrap().color,
                Some(if width >= 600.0 { 0x0000FF } else { 0xFF0000 })
            );
            assert_eq!(
                frame
                    .stylesheets
                    .diagnostics
                    .iter()
                    .map(|d| (d.node_id, d.message.clone()))
                    .collect::<Vec<_>>(),
                diagnostics
            );
            assert!(Arc::ptr_eq(
                &sheet,
                &frame.stylesheets.stylesheets[0].stylesheet
            ));
        }
    }
}
