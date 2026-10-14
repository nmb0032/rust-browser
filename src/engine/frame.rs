use std::{collections::HashMap, sync::Arc, time::Instant};

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
    /// Immutable cascade snapshot for this update, never sampled effects.
    pub author_styles: ComputedStyles,
    animations: style::animation::Engine,
    pub images: ImageMap,
    pub marker_images: ImageMap,
    pub effect_images: HashMap<String, Arc<crate::media::DecodedImage>>,
    pub(super) image_cache: HashMap<Url, Arc<crate::media::DecodedImage>>,
    pub(super) image_resource_count: usize,
    pub(super) image_resource_bytes: usize,
    pub(super) failed_effect_images: std::collections::HashSet<String>,
    pub error: Option<String>,
    pub parse_errors: Vec<crate::html::tokenizer::ParseError>,
    pub dropped_parse_errors: usize,
    media_environment: MediaEnvironment,
    view_scale: crate::display::ViewScale,
    interaction_state: InteractionState,
    scroll_state: layout::ScrollState,
    // Keep collection/resource diagnostics separate from resolution diagnostics,
    // so repeated restyles do not append the same errors to previous results.
    collected_stylesheets: StylesheetCollection,
    fonts: crate::text::FontContext,
    /// Fonts of the latest eager restyle while `author_styles` still matches
    /// the first container pass (no snapshot). Consumed by the next layout.
    eager_fonts: Option<crate::text::FontContext>,
    pub(super) web_fonts: super::web_fonts::WebFonts,
    pub(crate) resource_policy: super::resource_policy::ResourcePolicy,
    timings: super::StageTimings,
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
            author_styles: resolution.styles.clone(),
            styles: resolution.styles,
            animations: style::animation::Engine::default(),
            images: ImageMap::new(),
            marker_images: ImageMap::new(),
            effect_images: HashMap::new(),
            image_cache: HashMap::new(),
            image_resource_count: 0,
            image_resource_bytes: 0,
            failed_effect_images: std::collections::HashSet::new(),
            error: None,
            parse_errors: Vec::new(),
            dropped_parse_errors: 0,
            media_environment,
            view_scale: crate::display::ViewScale::new(
                f64::from(media_environment.resolution_dppx.unwrap_or(1.0)),
                1.0,
            )
            .unwrap_or_default(),
            interaction_state: InteractionState::default(),
            scroll_state: layout::ScrollState::default(),
            collected_stylesheets,
            fonts: crate::text::FontContext::bundled(),
            eager_fonts: None,
            web_fonts: super::web_fonts::WebFonts::default(),
            resource_policy: super::resource_policy::ResourcePolicy::default(),
            timings: super::StageTimings::default(),
        };
        frame.refresh_target();
        frame
    }

    pub fn media_environment(&self) -> MediaEnvironment {
        self.media_environment
    }

    pub fn view_scale(&self) -> crate::display::ViewScale {
        self.view_scale
    }

    /// Unzoomed logical viewport (physical / DPR). Resource and scroll identity
    /// remain retained; CSS viewport, resolution queries and layout change once.
    pub fn set_viewport(
        &mut self,
        width: f32,
        height: f32,
        scale: crate::display::ViewScale,
    ) -> Result<bool, &'static str> {
        crate::display::ViewScale::new(scale.dpr, scale.zoom)?;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("viewport must have finite positive dimensions");
        }
        let ratio = (self.view_scale.zoom / scale.zoom) as f32;
        let resize = |viewport: Option<(f32, f32)>| viewport.map(|(w, h)| (w * ratio, h * ratio));
        self.view_scale = scale;
        Ok(self.set_media_environment(MediaEnvironment {
            viewport_width: width / scale.zoom as f32,
            viewport_height: height / scale.zoom as f32,
            resolution_dppx: Some(scale.effective() as f32),
            small_viewport: resize(self.media_environment.small_viewport),
            large_viewport: resize(self.media_environment.large_viewport),
            dynamic_viewport: resize(self.media_environment.dynamic_viewport),
            ..self.media_environment
        }))
    }

    pub fn set_page_zoom(&mut self, zoom: f64) -> Result<bool, &'static str> {
        let old = self.view_scale;
        let next = crate::display::ViewScale::new(old.dpr, zoom)?;
        self.set_viewport(
            self.media_environment.viewport_width * old.zoom as f32,
            self.media_environment.viewport_height * old.zoom as f32,
            next,
        )
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
        let old = std::mem::replace(&mut self.interaction_state, state);
        // No state selector can match a changed element: styles, layout, and
        // paint are unchanged, so skip the restyle.
        if !self
            .styles
            .state_affected(&self.document, &old, &self.interaction_state)
        {
            return false;
        }
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
        self.fragment_target_checked(fragment).ok().flatten()
    }

    pub fn fragment_target_checked(
        &self,
        fragment: &str,
    ) -> Result<Option<FragmentTarget>, &'static str> {
        if fragment.is_empty() {
            return Ok(Some(FragmentTarget::Top));
        }
        let bytes = fragment.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'%'
                && (!bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                    || !bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
            {
                return Err("fragment contains malformed percent encoding");
            }
        }
        let decoded = percent_encoding::percent_decode_str(fragment)
            .decode_utf8()
            .map_err(|_| "fragment is not UTF-8")?;
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
                return Ok(Some(FragmentTarget::Node(id)));
            }
        }
        Ok(decoded
            .eq_ignore_ascii_case("top")
            .then_some(FragmentTarget::Top))
    }

    /// Retained native/headless fragment boundary, after resource layout.
    pub fn scroll_to_fragment(
        &mut self,
        tree: &mut LayoutTree,
        behavior: layout::ScrollBehavior,
    ) -> Result<bool, &'static str> {
        if !self.scroll_state.same_document(tree.scroll_state()) {
            return Err("fragment layout belongs to another document");
        }
        let Some(fragment) = self.url.fragment() else {
            return Ok(false);
        };
        let changed = match self.fragment_target_checked(fragment)? {
            Some(FragmentTarget::Top) => {
                tree.request_scroll(None, layout::ScrollOffset::default(), behavior)?
            }
            Some(FragmentTarget::Node(node)) => tree.scroll_into_view_with_style(
                &self.document,
                node,
                self.styles.get(node),
                behavior,
            )?,
            None => false,
        };
        self.retain_scroll_state(tree);
        Ok(changed)
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
        tree.effect_images = self.effect_images.clone();
        tree
    }

    pub fn scroll_state(&self) -> &layout::ScrollState {
        &self.scroll_state
    }

    pub fn has_pending_effect_images(&self) -> bool {
        self.document.preorder().any(|id| {
            let styles = self.styles.get(id).into_iter().chain(
                style::PseudoElement::ALL
                    .into_iter()
                    .filter_map(|p| self.styles.pseudo(id, p)),
            );
            styles.flat_map(|s| s.effects.urls()).any(|u| {
                !self.effect_images.contains_key(&u) && !self.failed_effect_images.contains(&u)
            })
        })
    }

    pub fn load_pending_effect_images(
        &mut self,
        mut on_progress: impl FnMut(&Frame) -> Result<(), Box<dyn std::error::Error>>,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        super::resources::load_effect_images(self, &mut on_progress, &cancelled)
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
            self.animations = frame.animations.clone();
        }
    }

    pub fn animations(&self) -> &style::animation::Engine {
        &self.animations
    }
    pub fn animations_mut(&mut self) -> &mut style::animation::Engine {
        &mut self.animations
    }
    pub(crate) fn same_animation_document(&self, other: &Self) -> bool {
        self.scroll_state.same_document(&other.scroll_state)
    }
    pub(super) fn set_document_scroll_identity(&mut self, state: layout::ScrollState) {
        self.scroll_state = state;
    }

    /// Explicit document clock. Native UI supplies a monotonic Instant offset;
    /// headless callers supply deterministic seconds, never sleep.
    pub fn set_animation_time(&mut self, seconds: f64) -> Result<(), &'static str> {
        self.animations.set_time(seconds)?;
        self.scroll_state.set_time(seconds);
        Ok(())
    }

    /// One bounded scroll/layout feedback sample from completed geometry. The
    /// resulting tree owns the same scroll identity and sampled paint styles.
    pub fn sample_scroll_animations(
        &mut self,
        tree: LayoutTree,
        _font: &crate::text::FontContext,
    ) -> LayoutTree {
        if !self.animations.has_scroll_effects() {
            return tree;
        }
        self.retain_scroll_state(&tree);
        let containers = style::ContainerSnapshot::from_layout(&tree, self.media_environment);
        self.animations.timelines =
            style::animation::timelines::Snapshot::from_layout(&self.document, &self.styles, &tree);
        let resolution = style::resolve_animated(
            &self.document,
            self.collected_stylesheets.clone(),
            self.media_environment,
            &self.interaction_state,
            &containers,
            &self.fonts,
            &mut self.animations,
        );
        self.styles = resolution.styles;
        self.stylesheets = resolution.stylesheets;
        for d in self.animations.diagnostics() {
            if !self.stylesheets.diagnostics.contains(d) {
                self.stylesheets.diagnostics.push(d.clone());
            }
        }
        let mut sampled = self.layout_tree();
        let font = self.fonts.clone();
        sampled.layout_viewport(
            self.media_environment.viewport_width,
            self.media_environment.viewport_height,
            &font,
        );
        self.retain_scroll_state(&sampled);
        sampled
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
        let started = Instant::now();
        let mut timings = super::StageTimings {
            eager_restyle: self.timings.eager_restyle,
            ..Default::default()
        };
        let tree = self.layout_container_passes(font, limit, &mut timings);
        timings.total = started.elapsed();
        self.timings = timings;
        if super::frame_trace_enabled() {
            eprintln!("[frame] {}", timings.summary());
        }
        tree
    }

    fn layout_container_passes(
        &mut self,
        font: &crate::text::FontContext,
        limit: usize,
        timings: &mut super::StageTimings,
    ) -> LayoutTree {
        self.fonts = self.web_fonts.context(self, font);
        let font = self.fonts.clone();
        let mut snapshot = style::ContainerSnapshot::default();
        let mut last = None;
        let animation_base = self.animations.clone();
        let mut eager = self.eager_fonts.take().filter(|fonts| fonts.same(&font));
        for _ in 0..limit.max(1) {
            timings.passes += 1;
            let stage = Instant::now();
            self.animations = animation_base.clone();
            let reuse = eager.take().is_some()
                && !self.author_styles.has_motion()
                && !self.animations.has_targets();
            if reuse {
                // Without motion or retained targets the animated cascade
                // equals the eager base cascade; only the engine bookkeeping
                // of a resolution remains.
                self.animations.begin();
                self.animations.end();
                self.styles = self.author_styles.clone();
            } else {
                let resolution = style::resolve_animated(
                    &self.document,
                    self.collected_stylesheets.clone(),
                    self.media_environment,
                    &self.interaction_state,
                    &snapshot,
                    &font,
                    &mut self.animations,
                );
                self.styles = resolution.styles;
                self.stylesheets = resolution.stylesheets;
                for d in self.web_fonts.diagnostics() {
                    if !self.stylesheets.diagnostics.contains(d) {
                        self.stylesheets.diagnostics.push(d.clone());
                    }
                }
            }
            for d in self.animations.diagnostics() {
                if !self.stylesheets.diagnostics.contains(d) {
                    self.stylesheets.diagnostics.push(d.clone());
                }
            }
            timings.animated_cascade += stage.elapsed();
            let stage = Instant::now();
            if !reuse {
                if self.animations.has_effects() {
                    let mut sheets = self.collected_stylesheets.clone();
                    sheets
                        .diagnostics
                        .extend(self.web_fonts.diagnostics().iter().cloned());
                    self.author_styles = style::resolve_with_fonts(
                        &self.document,
                        sheets,
                        self.media_environment,
                        &self.interaction_state,
                        &snapshot,
                        &font,
                    )
                    .styles;
                } else {
                    // No sampled effect: the base cascade is identical.
                    self.author_styles = self.styles.clone();
                }
            }
            timings.cascade += stage.elapsed();
            let stage = Instant::now();
            let mut tree = self.layout_tree();
            timings.box_build += stage.elapsed();
            let stage = Instant::now();
            if self.media_environment.media_type == crate::css::media::MediaType::Print {
                tree.layout_page_area(
                    self.media_environment.viewport_width,
                    self.media_environment.viewport_height,
                    &font,
                );
            } else {
                tree.layout_viewport(
                    self.media_environment.viewport_width,
                    self.media_environment.viewport_height,
                    &font,
                );
            }
            timings.layout += stage.elapsed();
            let stage = Instant::now();
            let next = style::ContainerSnapshot::from_layout(&tree, self.media_environment);
            if next == snapshot {
                if let Err(error) = tree.sample_scrolling(self.animations.time()) {
                    eprintln!("Scroll clock: {error}");
                }
                self.retain_scroll_state(&tree);
                let tree = self.sample_scroll_animations(tree, &font);
                timings.feedback += stage.elapsed();
                return tree;
            }
            timings.feedback += stage.elapsed();
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
        let stage = Instant::now();
        let mut tree = last.expect("at least one feedback layout pass");
        if let Err(error) = tree.sample_scrolling(self.animations.time()) {
            eprintln!("Scroll clock: {error}");
        }
        self.retain_scroll_state(&tree);
        let tree = self.sample_scroll_animations(tree, &font);
        timings.feedback += stage.elapsed();
        tree
    }

    pub(super) fn set_stylesheets(&mut self, stylesheets: StylesheetCollection) {
        self.collected_stylesheets = stylesheets.clone();
        self.stylesheets = stylesheets;
        self.refresh_fonts();
    }

    /// Native CSSOM/resource-update boundary. Effects retain their creation
    /// times; the next layout performs normal cascade and transition matching.
    pub fn replace_author_stylesheets(&mut self, stylesheets: StylesheetCollection) {
        self.set_stylesheets(stylesheets);
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

    /// Timings of the latest eager restyle and `layout_with_container_queries`.
    pub fn stage_timings(&self) -> super::StageTimings {
        self.timings
    }

    pub(super) fn refresh_fonts(&mut self) {
        let started = Instant::now();
        self.refresh_fonts_untimed();
        self.timings.eager_restyle = started.elapsed();
    }

    fn refresh_fonts_untimed(&mut self) {
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
        self.author_styles = resolution.styles.clone();
        self.styles = resolution.styles;
        self.eager_fonts = Some(self.fonts.clone());
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
