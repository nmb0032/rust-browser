use std::{
    num::NonZeroU32,
    rc::Rc,
    sync::{Arc, mpsc::TryRecvError},
    time::{Duration, Instant},
};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowId},
};

use super::input::{self, HistoryDirection};
use super::render_worker::{RenderInputs, RenderJob, RenderOutput, RenderWorker, render_job};
use crate::paint::{self, DisplayItem};
use crate::{
    css::selectors::InteractionState,
    dom::{Document, NodeId},
    engine::{Frame, LoadEvent, Navigation, link_url},
    layout::LayoutTree,
};

pub fn run(
    navigation: Navigation,
    font: crate::text::FontContext,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_view(navigation, font, 1.0, crate::display::ColorProfile::Srgb)
}

pub fn run_with_view(
    navigation: Navigation,
    font: crate::text::FontContext,
    page_zoom: f64,
    profile: crate::display::ColorProfile,
) -> Result<(), Box<dyn std::error::Error>> {
    profile.validate_native()?;
    crate::display::ViewScale::new(1.0, page_zoom)?;
    eprintln!(
        "Display: softbuffer untagged RGB8, assumed sRGB; monitor ICC detection/tagged wide-gamut presentation unavailable"
    );
    let event_loop = EventLoop::new()?;
    let proxy = event_loop.create_proxy();
    let renderer = RenderWorker::spawn(font.clone(), move || {
        let _ = proxy.send_event(());
    })
    .inspect_err(|error| eprintln!("Render worker unavailable, rendering inline: {error}"))
    .ok();
    let context = Context::new(event_loop.owned_display_handle())?;
    let frame = Arc::new(Frame::new(
        navigation.history.current().url.clone(),
        Document::new(),
    ));
    let layout_tree = frame.layout_tree();

    let mut app = BrowserWindow {
        context,
        window: None,
        surface: None,
        font,
        layout_tree,
        display_list: Vec::new(),
        scroll_offset: 0.0,
        show_layout_outlines: false,
        navigation,
        frame,
        cursor: None,
        pressed_link: None,
        scrollbar_drag: None,
        modifiers: ModifiersState::empty(),
        restore_scroll: None,
        fragment_scroll_pending: true,
        is_loading: true,
        animation_origin: Instant::now(),
        animation_deadline: None,
        page_zoom,
        pointer_dirty: false,
        scroll_dirty: false,
        cursor_icon: None,
        pending_snapshot: None,
        next_snapshot_at: None,
        snapshot_forced: false,
        snapshot_unaccepted: false,
        renderer,
        render_generation: 0,
        in_flight: None,
        render_dirty: false,
        frame_epoch: 0,
        requested_interaction: InteractionState::default(),
        interaction_feedback: 0,
        pending_refresh_target: false,
        pending_resources: false,
        rendered_scale: page_zoom,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct BrowserWindow {
    context: Context<OwnedDisplayHandle>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    font: crate::text::FontContext,
    layout_tree: LayoutTree,
    display_list: Vec<DisplayItem>,
    scroll_offset: f32,
    show_layout_outlines: bool,
    navigation: Navigation,
    frame: Arc<Frame>,
    cursor: Option<(f64, f64)>,
    pressed_link: Option<url::Url>,
    scrollbar_drag: Option<(crate::layout::Scrollbar, f32)>,
    modifiers: ModifiersState,
    restore_scroll: Option<f32>,
    fragment_scroll_pending: bool,
    is_loading: bool,
    animation_origin: Instant,
    animation_deadline: Option<Instant>,
    page_zoom: f64,
    /// Pointer moved since the last flush; hover and drags apply once per
    /// event-loop turn rather than once per `CursorMoved`.
    pointer_dirty: bool,
    /// The retained layout scrolled since the last flush; the display list,
    /// hover and cursor catch up once per event-loop turn, not per wheel event.
    scroll_dirty: bool,
    /// Last icon sent to the window, so unchanged cursors skip the platform call.
    cursor_icon: Option<CursorIcon>,
    /// Latest loader snapshot not yet rendered; intermediate ones are dropped.
    pending_snapshot: Option<(Arc<Frame>, bool)>,
    /// Earliest time the next loader snapshot may be rendered.
    next_snapshot_at: Option<Instant>,
    /// The loader finished: render the pending snapshot without throttling.
    snapshot_forced: bool,
    /// A loader snapshot was submitted but its render is not yet accepted.
    snapshot_unaccepted: bool,
    /// Off-thread renderer; `None` renders inline on the UI thread.
    renderer: Option<RenderWorker>,
    render_generation: u64,
    /// At most one render runs at a time; requests made meanwhile coalesce
    /// into `render_dirty` and are submitted from the latest state.
    in_flight: Option<InFlight>,
    render_dirty: bool,
    /// Bumped when `frame` is replaced or mutated outside of render inputs,
    /// so results computed from an older base are discarded.
    frame_epoch: u64,
    /// Latest hover/active/focus from input; rendered asynchronously.
    requested_interaction: InteractionState,
    /// Consecutive renders caused only by geometry-dependent hover feedback.
    interaction_feedback: u8,
    pending_refresh_target: bool,
    /// Web fonts or effect images of `frame` still to load.
    pending_resources: bool,
    /// Device scale x zoom of the accepted layout and display list.
    rendered_scale: f64,
}

struct InFlight {
    generation: u64,
    frame_epoch: u64,
    /// UI scroll state at submission; a difference at acceptance means the
    /// user scrolled the retained layout meanwhile.
    scroll: crate::layout::ScrollState,
    snapshot: bool,
    /// Fragment/restore scroll applied to a finished load.
    settles_fragment: bool,
    scale: f64,
    submitted: Instant,
}

pub(crate) fn zoom_shortcut(key: &Key, modifiers: ModifiersState, zoom: f64) -> Option<f64> {
    if !(modifiers.control_key() || modifiers.super_key()) || modifiers.alt_key() {
        return None;
    }
    match key {
        Key::Character(key) if key == "+" || key == "=" => Some(zoom * 1.25),
        Key::Character(key) if key == "-" => Some(zoom / 1.25),
        Key::Character(key) if key == "0" => Some(1.0),
        _ => None,
    }
}

impl BrowserWindow {
    /// Scale of the accepted layout/display list; input mapping and raster
    /// must agree with it even while a re-render at a new scale is pending.
    fn effective_scale(&self) -> f64 {
        self.rendered_scale
    }

    fn zoom_to(&mut self, zoom: f64) {
        let dpr = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
        let zoom = zoom.clamp((0.125 / dpr).max(0.25), (8.0 / dpr).min(4.0));
        if zoom == self.page_zoom {
            return;
        }
        self.page_zoom = zoom;
        self.scrollbar_drag = None;
        self.layout_tree.cancel_scroll_animations();
        Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
        self.request_render();
        self.request_redraw();
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.scrollbar_drag = None;
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return;
        };

        if let Some(surface) = &mut self.surface {
            surface
                .resize(width, height)
                .expect("failed to resize software surface");
        }

        self.request_render();
        self.update_interaction(InteractionEvent::Pointer);
        self.request_redraw();
    }

    /// Ask for a full restyle/layout/paint of the current state. Never blocks:
    /// the request coalesces with any render already running.
    fn request_render(&mut self) {
        self.render_dirty = true;
        self.pump_render();
    }

    fn render_inputs(&mut self) -> Option<RenderInputs> {
        let window = self.window.as_ref()?;
        let fragment_scroll = self.restore_scroll.is_none() && self.fragment_scroll_pending;
        Some(RenderInputs {
            size: window.inner_size(),
            scale_factor: window.scale_factor(),
            zoom: self.page_zoom,
            color_scheme: window.theme().map(|theme| match theme {
                winit::window::Theme::Light => crate::css::media::ColorScheme::Light,
                winit::window::Theme::Dark => crate::css::media::ColorScheme::Dark,
            }),
            animation_time: self.animation_origin.elapsed().as_secs_f64(),
            interaction: self.requested_interaction.clone(),
            outlines: self.show_layout_outlines,
            scroll: self.scroll_offset,
            refresh_target: std::mem::take(&mut self.pending_refresh_target),
            fragment_scroll,
            restore_scroll: if self.is_loading {
                None
            } else {
                self.restore_scroll.take()
            },
        })
    }

    /// Submit the latest state when the renderer is idle and something is due.
    fn pump_render(&mut self) {
        if self.in_flight.is_some() || self.window.is_none() {
            return;
        }
        let snapshot_due = self.pending_snapshot.is_some()
            && (self.snapshot_forced
                || self
                    .next_snapshot_at
                    .is_none_or(|deadline| Instant::now() >= deadline));
        if !snapshot_due && !self.render_dirty {
            return;
        }
        let base = match self.pending_snapshot.take().filter(|_| snapshot_due) {
            Some((frame, font_loading)) => {
                self.snapshot_forced = false;
                self.snapshot_unaccepted = true;
                self.prepare_snapshot(frame, font_loading)
            }
            None => Arc::clone(&self.frame),
        };
        let Some(inputs) = self.render_inputs() else {
            return;
        };
        self.render_dirty = false;
        self.render_generation += 1;
        self.in_flight = Some(InFlight {
            generation: self.render_generation,
            frame_epoch: self.frame_epoch,
            scroll: self.layout_tree.scroll_state().clone(),
            snapshot: snapshot_due,
            settles_fragment: !self.is_loading
                && (inputs.fragment_scroll || inputs.restore_scroll.is_some()),
            scale: inputs.scale_factor * inputs.zoom,
            submitted: Instant::now(),
        });
        let job = RenderJob {
            generation: self.render_generation,
            frame: base,
            inputs,
        };
        let job = match &self.renderer {
            Some(renderer) => match renderer.submit(job) {
                Ok(()) => return,
                Err(job) => {
                    eprintln!("Render worker exited; rendering inline");
                    self.renderer = None;
                    *job
                }
            },
            None => job,
        };
        let started = Instant::now();
        let result = render_job(job.frame, &job.inputs, &self.font);
        self.accept_render(RenderOutput {
            generation: job.generation,
            elapsed: started.elapsed(),
            result,
        });
    }

    /// Synchronous render for trivial documents (blank and error pages) that
    /// replace the frame; any in-flight result for the old frame is dropped.
    fn render_now(&mut self) {
        self.frame_epoch += 1;
        self.render_dirty = false;
        let Some(inputs) = self.render_inputs() else {
            return;
        };
        match render_job(Arc::clone(&self.frame), &inputs, &self.font) {
            Ok(rendered) => {
                self.frame = rendered.frame;
                self.layout_tree = rendered.tree;
                self.display_list = rendered.list;
                self.scroll_offset = rendered.scroll;
                self.pending_resources = rendered.pending_resources;
                self.rendered_scale = inputs.scale_factor * inputs.zoom;
                self.update_cursor();
            }
            Err(error) => eprintln!("{error}"),
        }
    }

    fn receive_render_results(&mut self) {
        while let Some(output) = self.renderer.as_ref().and_then(RenderWorker::try_recv) {
            self.accept_render(output);
        }
    }

    /// Install a finished render as the presented frame/layout/display list,
    /// carrying over any scrolling done on the old layout meanwhile.
    fn accept_render(&mut self, output: RenderOutput) {
        let Some(flight) = self
            .in_flight
            .take_if(|flight| flight.generation == output.generation)
        else {
            return;
        };
        if flight.snapshot {
            self.snapshot_unaccepted = false;
            self.next_snapshot_at =
                Some(Instant::now() + output.elapsed.max(Duration::from_millis(100)));
        }
        let rendered = match output.result {
            _ if flight.frame_epoch != self.frame_epoch => {
                self.render_dirty = true;
                self.pump_render();
                return;
            }
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("Render failed: {error}");
                self.pump_render();
                return;
            }
        };
        let user_scrolled = self.layout_tree.scroll_state() != &flight.scroll;
        let ui_scroll = user_scrolled.then(|| self.layout_tree.scroll_state().clone());
        self.frame = rendered.frame;
        self.layout_tree = rendered.tree;
        self.display_list = rendered.list;
        self.scroll_offset = rendered.scroll;
        self.pending_resources = rendered.pending_resources;
        self.rendered_scale = flight.scale;
        if flight.settles_fragment && !user_scrolled {
            self.fragment_scroll_pending = rendered.pending_resources;
        }
        if let Some(state) = ui_scroll
            && self.layout_tree.adopt_scroll_state(&state)
        {
            self.repaint_list();
        }
        if crate::engine::frame_trace_enabled() {
            eprintln!(
                "[ui] render accepted: worker={:.1}ms latency={:.1}ms display_items={} scroll_adopted={} {}",
                output.elapsed.as_secs_f64() * 1e3,
                flight.submitted.elapsed().as_secs_f64() * 1e3,
                self.display_list.len(),
                user_scrolled,
                self.frame.stage_timings().summary()
            );
        }
        self.update_title();
        self.request_redraw();
        // Geometry-dependent hover rules can move the hit target; reconcile
        // against the new layout with a bounded number of follow-up renders.
        self.update_pointer_feedback(true);
        self.pump_render();
    }

    fn scroll_by(&mut self, delta: f32) {
        self.scroll_key_by(crate::layout::ScrollOffset { x: 0.0, y: delta });
    }

    fn scroll_key_by(&mut self, delta: crate::layout::ScrollOffset) {
        let Some(_) = &self.window else { return };
        let scale = self.effective_scale();
        let (x, y) = self.cursor.map_or((-1.0, -1.0), |(x, y)| {
            input::document_point(x, y, scale, 0.0)
        });
        if let Err(error) = self
            .layout_tree
            .sample_scrolling(self.animation_origin.elapsed().as_secs_f64())
        {
            eprintln!("Scroll clock: {error}");
        }
        match self.layout_tree.scroll_key(x, y, delta) {
            Ok(true) => {
                self.animation_deadline = None;
                self.fragment_scroll_pending = false;
                self.mark_scrolled();
            }
            Ok(false) => {}
            Err(error) => eprintln!("Keyboard scroll: {error}"),
        }
    }

    fn scroll_by_axes(&mut self, delta: crate::layout::ScrollOffset) {
        if self.window.is_none() {
            return;
        }
        let scale_factor = self.effective_scale();
        let (x, y) = self.cursor.map_or((-1.0, -1.0), |(x, y)| {
            input::document_point(x, y, scale_factor, 0.0)
        });
        let sampled = match self
            .layout_tree
            .sample_scrolling(self.animation_origin.elapsed().as_secs_f64())
        {
            Ok(changed) => changed,
            Err(error) => {
                eprintln!("Scroll clock: {error}");
                false
            }
        };
        let changed = self.layout_tree.scroll_wheel(x, y, delta);
        self.animation_deadline = None;
        Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
        if changed || sampled {
            self.fragment_scroll_pending = false;
            self.mark_scrolled();
        }
    }

    /// The retained layout scrolled. Scroll offsets and frame state follow
    /// immediately (input and render submission read them); the repaint and
    /// pointer feedback are deferred to `flush_scroll`, so a burst of wheel
    /// events costs one display list and one hit test.
    fn mark_scrolled(&mut self) {
        self.scroll_offset = self.layout_tree.viewport_scroll().y;
        Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
        self.scroll_dirty = true;
        self.request_redraw();
    }

    fn flush_scroll(&mut self) {
        if std::mem::take(&mut self.scroll_dirty) {
            self.repaint_scrolled();
        }
    }

    /// Scrolling never changes style or layout, so repaint from the retained
    /// tree unless scroll-driven effects need a restyle.
    fn repaint_scrolled(&mut self) {
        self.repaint_list();
        self.update_pointer_feedback(false);
        self.request_redraw();
    }

    fn repaint_list(&mut self) {
        self.scroll_offset = self.layout_tree.viewport_scroll().y;
        Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
        if self.frame.animations().has_scroll_effects() {
            // Until the restyle lands, raster offsets the current list.
            self.request_render();
            return;
        }
        let Some(window) = &self.window else {
            return;
        };
        let logical = window
            .inner_size()
            .to_logical::<f32>(self.effective_scale());
        let started = Instant::now();
        self.display_list = paint::build_display_list(
            &self.layout_tree,
            logical.width,
            logical.height,
            self.show_layout_outlines,
        );
        if crate::engine::frame_trace_enabled() {
            eprintln!(
                "[ui] scroll repaint={:.1}ms display_items={}",
                started.elapsed().as_secs_f64() * 1e3,
                self.display_list.len()
            );
        }
    }

    /// Apply the latest pointer position: scrollbar drag or hover/cursor.
    fn flush_pointer(&mut self) {
        if !std::mem::take(&mut self.pointer_dirty) {
            return;
        }
        if let Some((bar, grab)) = self.scrollbar_drag.clone() {
            let Some((x, y)) = self.cursor else { return };
            let (x, y) = input::document_point(x, y, self.effective_scale(), 0.0);
            if self.layout_tree.drag_scrollbar(&bar, x, y, grab) {
                self.mark_scrolled();
            }
            return;
        }
        self.update_pointer_feedback(false);
    }

    /// Apply all deferred input: scrolling first, so hover and cursor see
    /// the latest offsets, then pointer movement.
    fn flush_input(&mut self) {
        self.flush_scroll();
        self.flush_pointer();
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn toggle_layout_outlines(&mut self) {
        self.show_layout_outlines = !self.show_layout_outlines;
        self.request_render();
        self.request_redraw();
    }

    fn receive_load_events(&mut self) {
        loop {
            let font_loading = self.navigation.is_font_loading();
            match self.navigation.try_recv() {
                Ok(LoadEvent::Progress(frame)) => {
                    // Keep draining so the loader is never blocked on paint;
                    // only the latest snapshot is rendered.
                    self.pending_snapshot = Some((frame, self.navigation.is_font_loading()));
                }
                Ok(LoadEvent::Finished) => {
                    self.snapshot_forced = self.pending_snapshot.is_some();
                    if !font_loading {
                        self.is_loading = false;
                        self.update_title();
                    }
                    if self.restore_scroll.is_some() || self.fragment_scroll_pending {
                        self.render_dirty = true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.snapshot_forced = self.pending_snapshot.is_some();
                    if self.is_loading {
                        self.is_loading = false;
                        self.show_load_error("Page loader disconnected before completion".into());
                    }
                    break;
                }
            }
        }
        self.pump_render();
    }

    /// Fix up a loader snapshot against the presented frame before it becomes
    /// the base of the next render. Style-affecting UI state (interaction,
    /// media environment, viewport) travels in the render inputs instead.
    fn prepare_snapshot(&mut self, mut frame: Arc<Frame>, font_loading: bool) -> Arc<Frame> {
        if !frame.same_animation_document(&self.frame) {
            self.animation_origin = Instant::now();
            self.animation_deadline = None;
            self.requested_interaction = frame.interaction_state().clone();
            self.interaction_feedback = 0;
        }
        let next = Arc::make_mut(&mut frame);
        next.copy_scroll_state_from(&self.frame);
        if font_loading {
            let url = self.frame.url.clone();
            next.url = url.clone();
            self.navigation.history.current_mut().url = url;
        }
        for diagnostic in &frame.stylesheets.diagnostics {
            eprintln!(
                "CSS/resource at {:?}: {}",
                diagnostic.node_id, diagnostic.message
            );
        }
        frame
    }

    fn update_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&input::window_title(
                &self.navigation.history.current().url,
                self.is_loading,
                self.frame.error.is_some(),
            ));
        }
    }

    /// Node under the pointer in the presented layout.
    fn cursor_hit(&self) -> Option<NodeId> {
        let (Some(_), Some((x, y))) = (&self.window, self.cursor) else {
            return None;
        };
        let (x, y) = input::document_point(x, y, self.effective_scale(), 0.0);
        self.layout_tree.hit_test_viewport(x, y, self.scroll_offset)
    }

    fn link_at(&self, hit: Option<NodeId>) -> Result<Option<url::Url>, Box<dyn std::error::Error>> {
        match hit {
            Some(node) => link_url(&self.frame.document, node, &self.frame.url),
            None => Ok(None),
        }
    }

    fn update_cursor(&mut self) {
        let hit = self.cursor_hit();
        self.update_cursor_at(hit);
    }

    fn update_cursor_at(&mut self, hit: Option<NodeId>) {
        let icon = if matches!(self.link_at(hit), Ok(Some(_))) {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        };
        if let Some(window) = &self.window
            && self.cursor_icon != Some(icon)
        {
            window.set_cursor(icon);
            self.cursor_icon = Some(icon);
        }
    }

    /// Cursor icon and hover from a single hit test of the presented layout.
    fn update_pointer_feedback(&mut self, feedback: bool) {
        let hit = self.cursor_hit();
        self.update_cursor_at(hit);
        self.reconcile_interaction_at(InteractionEvent::Pointer, hit, feedback);
    }

    fn update_interaction(&mut self, event: InteractionEvent) {
        let hit = self.cursor_hit();
        self.reconcile_interaction_at(event, hit, false);
    }

    /// Apply `event` at the hit-tested node and request a render only if a
    /// state selector can observe the change. `feedback` marks re-checks
    /// after a render, which are capped so oscillating hover rules settle.
    fn reconcile_interaction_at(
        &mut self,
        event: InteractionEvent,
        hit: Option<NodeId>,
        feedback: bool,
    ) {
        let next = next_interaction_state_at(
            &self.frame,
            &self.layout_tree,
            hit,
            event,
            &self.requested_interaction,
        );
        if next == self.requested_interaction {
            return;
        }
        let affected = self.frame.styles.state_affected(
            &self.frame.document,
            &self.requested_interaction,
            &next,
        );
        if !feedback {
            self.interaction_feedback = 0;
        } else if affected {
            if self.interaction_feedback >= 3 {
                let message = "pointer/style/layout feedback did not stabilize within 4 passes; retaining the last consistent state/layout pair";
                eprintln!("CSS: {message}");
                Arc::make_mut(&mut self.frame).stylesheets.diagnostics.push(
                    crate::style::Diagnostic {
                        node_id: None,
                        message: message.into(),
                    },
                );
                return;
            }
            self.interaction_feedback += 1;
        }
        self.requested_interaction = next;
        if affected {
            self.request_render();
        }
    }

    fn navigate(&mut self, url: url::Url) {
        self.navigation.history.current_mut().scroll_offset = self.scroll_offset;
        let same_document =
            self.frame.is_same_document(&url) && url.fragment().is_some() && !self.is_loading;
        if self.navigation.history.current().url != url || !same_document {
            self.navigation.history.push(url.clone());
        }
        if same_document {
            // The URL is frame state outside the render inputs: results based
            // on the old URL are stale.
            Arc::make_mut(&mut self.frame).url = url;
            self.frame_epoch += 1;
            self.pending_refresh_target = true;
            self.fragment_scroll_pending = true;
            self.animation_deadline = None;
            self.request_render();
            self.update_title();
        } else {
            self.begin_navigation(None);
        }
    }
}

#[derive(Clone, Copy)]
enum InteractionEvent {
    Pointer,
    Press,
    Release,
    Leave,
    Blur,
    Tab(bool),
}

/// The GUI and tests use the same hit-test/input/restyle boundary. Coordinates
/// have already been converted from device pixels to CSS viewport coordinates.
#[cfg(test)]
fn update_interaction_for_window(
    frame: &mut Frame,
    tree: &LayoutTree,
    point: Option<(f32, f32)>,
    event: InteractionEvent,
) -> bool {
    update_interaction_for_window_scrolled(frame, tree, point, 0.0, event)
}

#[cfg(test)]
fn update_interaction_for_window_scrolled(
    frame: &mut Frame,
    tree: &LayoutTree,
    point: Option<(f32, f32)>,
    scroll: f32,
    event: InteractionEvent,
) -> bool {
    let state =
        next_interaction_state(frame, tree, point, scroll, event, frame.interaction_state());
    frame.set_interaction_state(state)
}

/// Pure input step: the state `event` at `point` produces from `current`,
/// hit-testing `tree`, which must lay out `frame`'s document.
#[cfg(test)]
fn next_interaction_state(
    frame: &Frame,
    tree: &LayoutTree,
    point: Option<(f32, f32)>,
    scroll: f32,
    event: InteractionEvent,
    current: &InteractionState,
) -> InteractionState {
    let hit = point.and_then(|(x, y)| tree.hit_test_viewport(x, y, scroll));
    next_interaction_state_at(frame, tree, hit, event, current)
}

/// `next_interaction_state` for an already hit-tested node of `tree`.
fn next_interaction_state_at(
    frame: &Frame,
    tree: &LayoutTree,
    mut hit: Option<NodeId>,
    event: InteractionEvent,
    current: &InteractionState,
) -> InteractionState {
    let mut state = current.clone();
    while let Some(id) = hit {
        let node = frame
            .document
            .node(id)
            .expect("layout and DOM snapshot agree");
        if node.as_element().is_some() {
            break;
        }
        hit = node.parent();
    }
    match event {
        InteractionEvent::Pointer => state.hovered = hit,
        InteractionEvent::Press => {
            state.hovered = hit;
            state.active = hit;
            let mut focused = hit;
            while let Some(id) = focused {
                if crate::css::selectors::focusable(&frame.document, id) {
                    break;
                }
                focused = frame.document.node(id).and_then(|n| n.parent());
            }
            state.focused = focused;
            state.focus_visible = false;
        }
        InteractionEvent::Release => {
            state.hovered = hit;
            state.active = None;
        }
        InteractionEvent::Leave => {
            state.hovered = None;
            state.active = None;
        }
        InteractionEvent::Blur => {
            state.hovered = None;
            state.active = None;
            state.focused = None;
            state.focus_visible = false;
        }
        InteractionEvent::Tab(reverse) => {
            let mut candidates: Vec<(i32, usize, NodeId)> = frame
                .document
                .preorder()
                .enumerate()
                .filter_map(|(order, id)| {
                    if !crate::css::selectors::focusable(&frame.document, id)
                        || tree.node_rect(id).is_none()
                    {
                        return None;
                    }
                    let tab = frame
                        .document
                        .node(id)?
                        .as_element()?
                        .attribute("tabindex")
                        .and_then(|v| v.trim().parse::<i32>().ok())
                        .unwrap_or(0);
                    (tab >= 0).then_some((if tab == 0 { i32::MAX } else { tab }, order, id))
                })
                .collect();
            candidates.sort_unstable_by_key(|&(tab, order, _)| (tab, order));
            let current = candidates
                .iter()
                .position(|&(_, _, id)| Some(id) == state.focused);
            state.focused = if candidates.is_empty() {
                None
            } else {
                let index = match (current, reverse) {
                    (Some(index), true) => (index + candidates.len() - 1) % candidates.len(),
                    (Some(index), false) => (index + 1) % candidates.len(),
                    (None, true) => candidates.len() - 1,
                    (None, false) => 0,
                };
                Some(candidates[index].2)
            };
            state.focus_visible = state.focused.is_some();
        }
    }
    state
}

impl BrowserWindow {
    fn traverse_history(&mut self, direction: HistoryDirection) {
        self.navigation.history.current_mut().scroll_offset = self.scroll_offset;
        let entry = match direction {
            HistoryDirection::Back => self.navigation.history.back(),
            HistoryDirection::Forward => self.navigation.history.forward(),
        };
        if let Some(entry) = entry {
            let scroll = entry.scroll_offset;
            self.begin_navigation(Some(scroll));
        }
    }

    fn begin_navigation(&mut self, restore_scroll: Option<f32>) {
        self.scrollbar_drag = None;
        self.restore_scroll = restore_scroll;
        self.fragment_scroll_pending = restore_scroll.is_none();
        self.scroll_offset = 0.0;
        self.pressed_link = None;
        self.is_loading = true;
        self.replace_frame(Frame::new(
            self.navigation.history.current().url.clone(),
            Document::new(),
        ));
        self.update_title();
        self.request_redraw();
        if let Err(error) = self.navigation.start_load() {
            self.is_loading = false;
            self.show_load_error(format!("Could not start page loader: {error}"));
        }
    }

    fn show_load_error(&mut self, error: String) {
        eprintln!("{error}");
        self.replace_frame(Frame::error_page(
            self.navigation.history.current().url.clone(),
            error,
        ));
        self.update_title();
        self.request_redraw();
    }

    /// Present a new trivial document immediately, dropping every snapshot,
    /// render and input state that belongs to the previous one.
    fn replace_frame(&mut self, frame: Frame) {
        self.pending_snapshot = None;
        self.next_snapshot_at = None;
        self.snapshot_forced = false;
        self.snapshot_unaccepted = false;
        self.pending_refresh_target = false;
        self.requested_interaction = frame.interaction_state().clone();
        self.interaction_feedback = 0;
        self.frame = Arc::new(frame);
        self.layout_tree = self.frame.layout_tree();
        self.display_list.clear();
        self.render_now();
    }
}

/// Shared by progress, resize, and display-scale changes. Never touches the
/// loader or navigation history; all retained resources survive the restyle.
/// The UI render pipeline (layout, scroll clamp, display list) for a physical
/// window size. Shared with `benches/frame.rs` so benchmarks measure the
/// same work as the event loop.
pub fn render_for_window_size(
    frame: &mut Frame,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    font: &crate::text::FontContext,
    outlines: bool,
    scroll: f32,
) -> (LayoutTree, Vec<DisplayItem>, f32) {
    let scale = crate::display::ViewScale::new(scale_factor, frame.view_scale().zoom)
        .expect("validated window scale");
    let logical = size.to_logical::<f32>(scale.effective());
    frame
        .set_viewport(
            logical.width * scale.zoom as f32,
            logical.height * scale.zoom as f32,
            scale,
        )
        .expect("positive window viewport");
    let external_scroll = scroll != frame.scroll_state().viewport_scroll().y;
    let mut tree = frame.layout_with_container_queries(font);
    let range = tree.viewport_scroll_container().range;
    let scroll = scroll.clamp(
        range.min.y,
        range
            .max
            .y
            .min(paint::max_scroll_offset(&tree, logical.height)),
    );
    if external_scroll {
        tree.set_viewport_scroll(crate::layout::ScrollOffset {
            y: scroll,
            ..tree.viewport_scroll()
        });
    }
    let scroll = tree.viewport_scroll().y;
    frame.retain_scroll_state(&tree);
    tree = frame.sample_scroll_animations(tree, font);
    let list = paint::build_display_list(&tree, logical.width, logical.height, outlines);
    (tree, list, scroll)
}

impl ApplicationHandler for BrowserWindow {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);

        if self.window.is_some() {
            return;
        }

        let window = Rc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("rust-browser")
                        .with_inner_size(LogicalSize::new(
                            paint::DEFAULT_VIEWPORT_WIDTH as f64,
                            paint::DEFAULT_VIEWPORT_HEIGHT as f64,
                        )),
                )
                .expect("failed to create window"),
        );
        let size = window.inner_size();
        if let Err(error) = crate::display::ViewScale::new(window.scale_factor(), self.page_zoom) {
            eprintln!("Display scale: {error}");
            event_loop.exit();
            return;
        }
        let surface = Surface::new(&self.context, Rc::clone(&window))
            .expect("failed to create software surface");

        self.window = Some(window);
        self.cursor_icon = None;
        self.surface = Some(surface);
        self.resize_surface(size.width, size.height);
        self.update_title();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
            return;
        }

        match event {
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
                self.pointer_dirty = true;
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer_dirty = false;
                self.cursor = None;
                self.pressed_link = None;
                self.update_interaction(InteractionEvent::Leave);
                self.update_cursor();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.flush_input();
                if state == ElementState::Released && self.scrollbar_drag.take().is_some() {
                    self.update_interaction(InteractionEvent::Pointer);
                    return;
                }
                if state == ElementState::Pressed {
                    self.layout_tree.cancel_scroll_animations();
                    Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
                    self.fragment_scroll_pending = false;
                }
                if state == ElementState::Pressed
                    && let Some((x, y)) = self.cursor
                {
                    let scale = self.effective_scale();
                    let (x, y) = input::document_point(x, y, scale, 0.0);
                    if let Some(bar) = self.layout_tree.scrollbar_at(x, y) {
                        let grab = self.layout_tree.scrollbar_grab(&bar, x, y);
                        self.scrollbar_drag = Some((bar.clone(), grab));
                        self.pressed_link = None;
                        if self.layout_tree.drag_scrollbar(&bar, x, y, grab) {
                            self.mark_scrolled();
                        }
                        return;
                    }
                }
                let hit = self.cursor_hit();
                let link = self.link_at(hit);
                self.reconcile_interaction_at(
                    if state == ElementState::Pressed {
                        InteractionEvent::Press
                    } else {
                        InteractionEvent::Release
                    },
                    hit,
                    false,
                );
                match link {
                    Ok(link) if state == ElementState::Pressed => self.pressed_link = link,
                    Ok(Some(link)) if self.pressed_link.take().as_ref() == Some(&link) => {
                        self.navigate(link)
                    }
                    Ok(_) => self.pressed_link = None,
                    Err(error) => {
                        self.pressed_link = None;
                        eprintln!("link navigation failed: {error}");
                    }
                }
            }
            WindowEvent::Focused(false) => {
                self.scrollbar_drag = None;
                self.pressed_link = None;
                self.update_interaction(InteractionEvent::Blur);
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize_surface(size.width, size.height),
            WindowEvent::ThemeChanged(_) => {
                self.request_render();
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let dpr = self.window.as_ref().expect("active window").scale_factor();
                if let Err(error) = crate::display::ViewScale::new(dpr, 1.0) {
                    eprintln!("Display scale: {error}");
                    event_loop.exit();
                    return;
                }
                self.page_zoom = self
                    .page_zoom
                    .clamp((0.125 / dpr).max(0.25), (8.0 / dpr).min(4.0));
                let size = self
                    .window
                    .as_ref()
                    .expect("window event must belong to the active window")
                    .inner_size();
                self.resize_surface(size.width, size.height);
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let scale_factor = self.effective_scale() as f32;
                let scroll_delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => crate::layout::ScrollOffset {
                        x: -x * paint::LINE_HEIGHT,
                        y: -y * paint::LINE_HEIGHT,
                    },
                    MouseScrollDelta::PixelDelta(position) => crate::layout::ScrollOffset {
                        x: -(position.x as f32) / scale_factor,
                        y: -(position.y as f32) / scale_factor,
                    },
                };
                self.scroll_by_axes(scroll_delta);
                if phase == winit::event::TouchPhase::Ended
                    || phase == winit::event::TouchPhase::Cancelled
                {
                    self.layout_tree.finish_scroll_gesture();
                    self.mark_scrolled();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(zoom) =
                    zoom_shortcut(&event.logical_key, self.modifiers, self.page_zoom)
                {
                    self.zoom_to(zoom);
                    return;
                }
                if let Some(direction) = input::history_shortcut(&event.logical_key, self.modifiers)
                {
                    self.traverse_history(direction);
                    return;
                }
                match &event.logical_key {
                    Key::Named(NamedKey::Tab) => {
                        self.update_interaction(InteractionEvent::Tab(self.modifiers.shift_key()))
                    }
                    Key::Named(NamedKey::F12) => self.toggle_layout_outlines(),
                    Key::Character(key) if key.eq_ignore_ascii_case("o") => {
                        self.toggle_layout_outlines()
                    }
                    Key::Named(NamedKey::ArrowDown) => self.scroll_by(paint::LINE_HEIGHT),
                    Key::Named(NamedKey::ArrowUp) => self.scroll_by(-paint::LINE_HEIGHT),
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.scroll_key_by(crate::layout::ScrollOffset {
                            x: -paint::LINE_HEIGHT,
                            y: 0.0,
                        })
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        self.scroll_key_by(crate::layout::ScrollOffset {
                            x: paint::LINE_HEIGHT,
                            y: 0.0,
                        })
                    }
                    Key::Named(NamedKey::PageDown) => self
                        .scroll_by(self.layout_tree.viewport_scroll_container().port.height * 0.9),
                    Key::Named(NamedKey::PageUp) => self
                        .scroll_by(-self.layout_tree.viewport_scroll_container().port.height * 0.9),
                    Key::Named(NamedKey::Home) => self.scroll_by(-1.0e9),
                    Key::Named(NamedKey::End) => self.scroll_by(1.0e9),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                self.flush_scroll();
                let scale_factor = self.effective_scale();
                if let Err(error) = crate::display::ViewScale::new(
                    self.window.as_ref().expect("active window").scale_factor(),
                    self.page_zoom,
                ) {
                    eprintln!("Display scale: {error}; presentation suppressed");
                    return;
                }
                if let Some(surface) = &mut self.surface {
                    let started = Instant::now();
                    let mut buffer = surface
                        .buffer_mut()
                        .expect("failed to get software framebuffer");
                    let width = buffer.width().get() as usize;
                    let height = buffer.height().get() as usize;

                    buffer.fill(0x00FF_FFFF);
                    let raster_started = Instant::now();
                    paint::rasterize_display_list(
                        &self.display_list,
                        &self.font,
                        &mut buffer,
                        width,
                        height,
                        scale_factor,
                        self.scroll_offset,
                    );
                    let raster = raster_started.elapsed();
                    buffer.present().expect("failed to present framebuffer");
                    if crate::engine::frame_trace_enabled() {
                        eprintln!(
                            "[ui] redraw={:.1}ms raster={:.1}ms items={}",
                            started.elapsed().as_secs_f64() * 1e3,
                            raster.as_secs_f64() * 1e3,
                            self.display_list.len()
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _render_ready: ()) {
        self.receive_render_results();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.receive_render_results();
        self.receive_load_events();
        self.flush_input();
        if (self.frame.animations().needs_tick() || self.frame.scroll_state().needs_tick())
            && self.window.is_some()
        {
            let now = Instant::now();
            if self
                .animation_deadline
                .is_none_or(|deadline| now >= deadline)
            {
                let seconds = now.duration_since(self.animation_origin).as_secs_f64();
                let scroll_only = !self.frame.animations().needs_tick()
                    && !self.frame.animations().has_scroll_effects();
                if !scroll_only {
                    // The render samples the clock at submission.
                    self.request_render();
                } else if let Err(error) =
                    Arc::make_mut(&mut self.frame).set_animation_time(seconds)
                {
                    eprintln!("Animation clock: {error}");
                } else {
                    // Smooth scrolling samples the retained layout; no restyle.
                    if let Err(error) = self.layout_tree.sample_scrolling(seconds) {
                        eprintln!("Scroll clock: {error}");
                    }
                    self.mark_scrolled();
                }
                self.animation_deadline = if self.frame.animations().needs_tick()
                    || self.frame.scroll_state().has_animations()
                {
                    Some(now + Duration::from_millis(16))
                } else {
                    self.frame
                        .scroll_state()
                        .next_deadline()
                        .map(|t| self.animation_origin + Duration::from_secs_f64(t))
                };
            }
        } else {
            self.animation_deadline = None;
        }
        self.flush_scroll();
        // Fetch from the newest document state only: never while a loader
        // snapshot is still waiting to be rendered.
        if !self.is_loading
            && !self.navigation.is_font_loading()
            && self.pending_resources
            && self.pending_snapshot.is_none()
            && !self.snapshot_unaccepted
            && let Err(error) = self.navigation.start_font_load(Arc::clone(&self.frame))
        {
            eprintln!("Could not start font loader: {error}");
        }
        if (self.frame.animations().needs_tick() || self.frame.scroll_state().needs_tick())
            && self.window.is_some()
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                self.animation_deadline.unwrap_or_else(Instant::now),
            ));
        } else if self.is_loading || self.navigation.is_font_loading() {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.surface = None;
        self.window = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::media::MediaEnvironment;

    #[test]
    fn native_zoom_matches_headless_geometry_inverse_hits_and_retained_css_scroll() {
        let source = include_str!("../../tests/fixtures/display/zoom.html").replace(
            "</style>",
            "a{transform:translateX(5px);filter:opacity(1)}</style>",
        );
        let document = crate::html::parser::parse(&source).unwrap();
        let mut frame = Frame::new(url::Url::parse("https://example.test/").unwrap(), document);
        let font = crate::text::FontContext::bundled();
        let snapshot = frame.clone();
        frame.set_page_zoom(2.0).unwrap();
        for dpr in [1.0, 2.0] {
            let (tree, list, scroll) = render_for_window_size(
                &mut frame,
                PhysicalSize::new((200.0 * dpr) as u32, (160.0 * dpr) as u32),
                dpr,
                &font,
                false,
                100.0,
            );
            assert_eq!(scroll, 100.0);
            assert!(frame.same_animation_document(&snapshot));
            assert_eq!(frame.media_environment().viewport_width, 100.0);
            let point = input::document_point(32.0 * dpr, 22.0 * dpr, dpr * 2.0, 0.0);
            let hit = tree.hit_test_viewport(point.0, point.1, scroll).unwrap();
            assert_eq!(
                frame
                    .document
                    .node(hit)
                    .unwrap()
                    .as_element()
                    .unwrap()
                    .attribute("id"),
                Some("link")
            );
            let (width, height) = ((200.0 * dpr) as usize, (160.0 * dpr) as usize);
            let mut pixels = vec![0xffffff; width * height];
            paint::rasterize_display_list(
                &list,
                &font,
                &mut pixels,
                width,
                height,
                dpr * 2.0,
                scroll,
            );
            assert_eq!(
                pixels[(22.0 * dpr) as usize * width + (32.0 * dpr) as usize],
                0xff0000
            );
            assert_eq!(
                pixels[(22.0 * dpr) as usize * width + (82.0 * dpr) as usize],
                0x0000ff
            );
        }
        frame.set_page_zoom(1.0).unwrap();
        let (tree, _, scroll) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(400, 320),
            2.0,
            &font,
            false,
            100.0,
        );
        assert_eq!(scroll, 100.0);
        assert_eq!(tree.viewport_scroll().y, 100.0);
        assert_eq!(frame.media_environment().viewport_width, 200.0);
    }

    #[test]
    fn native_zoom_shortcuts_are_bounded_by_controller_and_have_reset() {
        let primary = ModifiersState::CONTROL;
        assert_eq!(
            zoom_shortcut(&Key::Character("+".into()), primary, 1.0),
            Some(1.25)
        );
        assert_eq!(
            zoom_shortcut(&Key::Character("-".into()), primary, 1.25),
            Some(1.0)
        );
        assert_eq!(
            zoom_shortcut(&Key::Character("0".into()), ModifiersState::SUPER, 4.0),
            Some(1.0)
        );
        assert_eq!(
            zoom_shortcut(&Key::Character("+".into()), ModifiersState::empty(), 1.0),
            None
        );
    }

    #[test]
    fn native_resize_renderer_samples_root_smooth_scroll_without_old_offset_overwrite() {
        let document = crate::html::parser::parse("<!doctype html><style>html,body{margin:0}html{scroll-behavior:smooth}#s{position:sticky;top:0;height:20px;background:red}#tail{height:800px}</style><div id=s></div><div id=tail></div>").unwrap();
        let mut frame = Frame::new(url::Url::parse("https://example.test/").unwrap(), document);
        let font = crate::text::FontContext::bundled();
        let (mut tree, _, _) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(300, 200),
            1.0,
            &font,
            false,
            0.0,
        );
        tree.request_scroll(
            None,
            crate::layout::ScrollOffset { x: 0.0, y: 200.0 },
            crate::layout::ScrollBehavior::Auto,
        )
        .unwrap();
        frame.retain_scroll_state(&tree);
        frame.set_animation_time(0.14).unwrap();
        let (tree, _, scroll) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(600, 400),
            2.0,
            &font,
            false,
            0.0,
        );
        assert!((scroll - 100.0).abs() < 0.01);
        assert_eq!(tree.viewport_scroll().y, scroll);
        let snapshot = frame.clone();
        frame.set_animation_time(1.0).unwrap();
        let (_, _, scroll) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(600, 400),
            2.0,
            &font,
            false,
            100.0,
        );
        assert_eq!(scroll, 200.0);
        assert!(!frame.scroll_state().needs_tick());
        assert_eq!(snapshot.scroll_state().viewport_scroll().y, 100.0);
    }
    use crate::{css::values::Length, html::parser};

    #[test]
    fn nested_scroll_input_survives_native_scale_resize_and_hover_rebuilds() {
        let doc = parser::parse(
            "<style>html,body{margin:0}\
            #p{overflow:auto;width:100px;height:60px}a{display:block;height:40px}\
            #last{height:200px}#tail{height:500px}#second:hover{background:blue}\
            @media(max-width:250px){#last{height:0}a{height:10px}}</style>\
            <div id=p><a href='#second'>First</a><a id=second href='#tail'>Second</a>\
            <div id=last></div></div><div id=tail></div>",
        )
        .unwrap();
        let mut frame = Frame::new(url::Url::parse("https://overflow.test/").unwrap(), doc);
        let find = |name: &str| {
            frame
                .document
                .preorder()
                .find(|&id| {
                    frame
                        .document
                        .node(id)
                        .unwrap()
                        .as_element()
                        .is_some_and(|e| e.attribute("id") == Some(name))
                })
                .unwrap()
        };
        let panel = find("p");
        let second = find("second");
        let font = crate::text::FontContext::bundled();
        let (mut tree, _, _) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(600, 400),
            2.0,
            &font,
            false,
            0.0,
        );
        assert!(tree.scroll_wheel(35.0, 35.0, crate::layout::ScrollOffset { x: 0.0, y: 40.5 }));
        frame.retain_scroll_state(&tree);
        let (tree, _, root) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(750, 500),
            2.5,
            &font,
            false,
            20.0,
        );
        assert_eq!(root, 20.0);
        assert_eq!(tree.scroll_offset(panel).y, 40.5);
        let hit = tree.hit_test_viewport(35.0, 10.0, root).unwrap();
        assert!(hit == second || frame.document.node(hit).unwrap().parent() == Some(second));
        assert!(update_interaction_for_window_scrolled(
            &mut frame,
            &tree,
            Some((35.0, 10.0)),
            root,
            InteractionEvent::Pointer,
        ));
        let (tree, _, root) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(750, 500),
            2.5,
            &font,
            false,
            root,
        );
        assert_eq!(tree.scroll_offset(panel).y, 40.5);
        assert_eq!(
            frame.styles.get(second).unwrap().background_color,
            Some(0x0000ff)
        );
        let (tree, _, _) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(600, 500),
            2.5,
            &font,
            false,
            root,
        );
        assert_eq!(
            tree.scroll_offset(panel),
            crate::layout::ScrollOffset::default()
        );
    }

    #[test]
    fn pointer_press_release_tab_blur_and_fragment_restyle_through_the_window_boundary() {
        let document = parser::parse(
            "<style>body {margin:0} #link, #second {color:black}
            main:where(:hover) #link {color:blue}
            #link:active {color:red}
            #link:focus {font-size:24px}
            #link:focus-visible::before {content:'Keyboard'; color:green}
            main:focus-within {background:yellow}
            #second:target {color:purple}
            </style><main><a id='link' href='#second'><span>Link</span></a>
            <a id='second' href='/' tabindex='2'>Second</a><a href='/' tabindex='-1'>Skip</a></main>",
        ).unwrap();
        let mut frame = Frame::new(
            url::Url::parse("https://example.test/#second").unwrap(),
            document,
        );
        let find = |frame: &Frame, name: &str| {
            frame
                .document
                .preorder()
                .find(|&id| {
                    frame
                        .document
                        .node(id)
                        .and_then(|n| n.as_element())
                        .is_some_and(|e| e.attribute("id") == Some(name))
                })
                .unwrap()
        };
        let link = find(&frame, "link");
        let second = find(&frame, "second");
        assert_eq!(frame.styles.get(second).unwrap().color, Some(0x800080));
        let font = paint::load_bundled_font().unwrap();
        let render = |frame: &mut Frame| {
            render_for_window_size(frame, PhysicalSize::new(800, 600), 1.0, &font, false, 0.0)
        };
        let (mut tree, _, _) = render(&mut frame);
        let rect = tree.node_rect(link).unwrap();
        let point = Some((rect.x + 2.0, rect.y + 2.0));
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            point,
            InteractionEvent::Pointer
        ));
        assert!(!update_interaction_for_window(
            &mut frame,
            &tree,
            point,
            InteractionEvent::Pointer
        ));
        assert_eq!(frame.styles.get(link).unwrap().color, Some(0x0000ff));
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            point,
            InteractionEvent::Press
        ));
        assert_eq!(frame.interaction_state().focused, Some(link));
        assert_eq!(frame.styles.get(link).unwrap().color, Some(0xff0000));
        assert_eq!(frame.styles.get(link).unwrap().font_size, 24.0);
        let (next_tree, list, _) = render(&mut frame);
        tree = next_tree;
        assert!(list.iter().any(|item| matches!(item, DisplayItem::Text {text,color:0xff0000,size:24.0,..} if text=="Link")));
        assert!(list.iter().any(|item| matches!(
            item,
            DisplayItem::Rect {
                color: 0xffff00,
                ..
            }
        )));
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            point,
            InteractionEvent::Release
        ));
        assert_eq!(frame.styles.get(link).unwrap().color, Some(0x0000ff));
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            None,
            InteractionEvent::Leave
        ));
        assert!(frame.interaction_state().hovered.is_none());
        assert_eq!(frame.styles.get(link).unwrap().color, Some(0));
        // Positive tabindex first; backwards from Link wraps to Second.
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            None,
            InteractionEvent::Tab(true)
        ));
        assert_eq!(frame.interaction_state().focused, Some(second));
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            None,
            InteractionEvent::Tab(false)
        ));
        assert_eq!(frame.interaction_state().focused, Some(link));
        let (_, list, _) = render(&mut frame);
        assert!(list.iter().any(
            |item| matches!(item, DisplayItem::Text {text,color:0x008000,..} if text=="Keyboard")
        ));
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            None,
            InteractionEvent::Blur
        ));
        assert!(frame.interaction_state().focused.is_none());
        assert!(
            frame
                .styles
                .pseudo(link, crate::style::PseudoElement::Before)
                .is_none()
        );
        frame.url.set_fragment(None);
        assert!(frame.refresh_target());
        assert_eq!(frame.styles.get(second).unwrap().color, Some(0));
    }

    #[test]
    fn window_pipeline_uses_container_geometry_and_explicit_preferences_at_resize_and_scale() {
        let mut frame = Frame::new(
            url::Url::parse("https://example.test/").unwrap(),
            parser::parse(
                "<style>body {margin:0}
              #host {container:component / inline-size; width:calc(50vw - 24px); padding:10px}
              #host:hover {width:600px}
              #card {width:10cqi; background:red}
              @container component (width >= 320px) {#card {background:blue}}
              @media (resolution >= 2dppx) {#card {color:blue}}
              @media (prefers-color-scheme:dark) {#card {margin-top:7px}}
              </style><div id='host'><div id='card'>Responsive</div></div>",
            )
            .unwrap(),
        );
        frame.set_media_environment(MediaEnvironment {
            color_scheme: Some(crate::css::media::ColorScheme::Dark),
            ..frame.media_environment()
        });
        let find = |frame: &Frame, name| {
            frame
                .document
                .preorder()
                .find(|&id| {
                    frame
                        .document
                        .node(id)
                        .and_then(|n| n.as_element())
                        .is_some_and(|e| e.attribute("id") == Some(name))
                })
                .unwrap()
        };
        let host = find(&frame, "host");
        let card = find(&frame, "card");
        let font = paint::load_bundled_font().unwrap();
        for (size, scale, expected_width, background, color) in [
            (PhysicalSize::new(1600, 1200), 2.0, 37.6, 0x0000ff, 0x0000ff),
            (PhysicalSize::new(600, 600), 1.0, 27.6, 0xff0000, 0x000000),
            (PhysicalSize::new(1600, 1200), 2.0, 37.6, 0x0000ff, 0x0000ff),
        ] {
            let (tree, _, _) = render_for_window_size(&mut frame, size, scale, &font, false, 0.0);
            assert!((tree.node_rect(card).unwrap().width - expected_width).abs() < 0.0001);
            let computed = frame.styles.get(card).unwrap();
            assert_eq!(computed.background_color, Some(background));
            assert_eq!(computed.color, Some(color));
            assert_eq!(computed.margin.top, Length::Px(7.0).into());
            assert_eq!(
                frame.media_environment().resolution_dppx,
                Some(scale as f32)
            );
        }
        let (tree, _, _) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(600, 600),
            1.0,
            &font,
            false,
            0.0,
        );
        let rect = tree.node_rect(host).unwrap();
        assert!(update_interaction_for_window(
            &mut frame,
            &tree,
            Some((rect.x + 1.0, rect.y + 1.0)),
            InteractionEvent::Pointer
        ));
        let (tree, _, _) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(600, 600),
            1.0,
            &font,
            false,
            0.0,
        );
        assert_eq!(tree.node_rect(card).unwrap().width, 60.0);
        assert_eq!(
            frame.styles.get(card).unwrap().background_color,
            Some(0x0000ff)
        );
        assert!(frame.stylesheets.diagnostics.is_empty());
    }

    #[test]
    fn math_geometry_and_viewport_lengths_recompute_at_logical_resize_and_scale() {
        let document = parser::parse(
            "<style>:root {font-size:20px} body {margin:0}
            #target {width:calc(100% - 2rem); background:red}
            #viewport {width:clamp(100px,50vw,300px); background:blue}</style>
            <div id='target'>math</div><div id='viewport'>viewport</div>",
        )
        .unwrap();
        let mut frame = Frame::new(url::Url::parse("https://example.test/").unwrap(), document);
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
        let viewport = frame
            .document
            .preorder()
            .find(|&id| {
                frame
                    .document
                    .node(id)
                    .unwrap()
                    .as_element()
                    .is_some_and(|e| e.attribute("id") == Some("viewport"))
            })
            .unwrap();
        let font = paint::load_bundled_font().unwrap();
        for (pixels, scale, width) in [(800, 2.0, 400.0), (800, 1.0, 800.0), (400, 2.0, 200.0)] {
            let (tree, list, _) = render_for_window_size(
                &mut frame,
                PhysicalSize::new(pixels, 1200),
                scale,
                &font,
                false,
                0.0,
            );
            assert!(frame.stylesheets.diagnostics.is_empty());
            assert_eq!(tree.node_rect(target).unwrap().width, width - 48.0 - 40.0);
            assert_eq!(
                tree.node_rect(viewport).unwrap().width,
                (width * 0.5).clamp(100.0, 300.0)
            );
            assert!(list.iter().any(
                |item| matches!(item,DisplayItem::Rect{w,color:0xff0000,..} if *w==width-48.0-40.0)
            ));
        }
    }

    #[test]
    fn resize_and_scale_pipeline_restyles_rebuilds_boxes_and_paint_without_resetting_scroll() {
        let document = parser::parse(
            "<!doctype html><style>\
            #target {color:blue; background:yellow; width:300px; padding-bottom:1400px}\
            @media (max-width:600px) {\
              #target {color:red; background:green; width:100px; padding-bottom:2000px}\
              #hidden {display:none}\
            }\
            @media (max-width:300px) {#target {padding-bottom:0}}\
            @media not (future-height:20px) {#target {color:purple}}\
            </style><p id='target'>responsive</p><p id='hidden'>hidden at narrow widths</p>",
        )
        .unwrap();
        let url = url::Url::parse("https://example.test/page#target").unwrap();
        let mut frame = Frame::new(url.clone(), document);
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
        let font = paint::load_bundled_font().unwrap();
        let mut scroll = 120.0;
        for (physical, scale, width, color, box_width, hidden) in [
            ((1600, 1200), 2.0, 800.0, 0x0000FF, 300.0, false),
            ((800, 1200), 2.0, 400.0, 0xFF0000, 100.0, true),
            ((800, 600), 1.0, 800.0, 0x0000FF, 300.0, false),
        ] {
            let (tree, list, next_scroll) = render_for_window_size(
                &mut frame,
                PhysicalSize::new(physical.0, physical.1),
                scale,
                &font,
                false,
                scroll,
            );
            assert_eq!(frame.media_environment().viewport_width, width);
            assert_eq!(frame.media_environment().viewport_height, 600.0);
            assert_eq!(
                frame.styles.get(target).unwrap().width,
                Some(Length::Px(box_width))
            );
            assert_eq!(tree.node_rect(target).unwrap().width, box_width);
            assert!(list.iter().any(|item| matches!(item, DisplayItem::Text {text, color: c, ..} if text == "responsive" && *c == color)));
            assert_eq!(
                list.iter()
                    .any(|item| matches!(item, DisplayItem::Text {text, ..} if text == "hidden")),
                !hidden
            );
            assert_eq!(next_scroll, scroll);
            scroll = next_scroll;
            assert_eq!(frame.stylesheets.diagnostics.len(), 1);
            assert!(Arc::ptr_eq(
                &sheet,
                &frame.stylesheets.stylesheets[0].stylesheet
            ));
            assert_eq!(frame.url, url);
        }
        let (_, _, clamped) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(500, 1200),
            2.0,
            &font,
            false,
            scroll,
        );
        assert_eq!(clamped, 0.0);
        assert_eq!(frame.stylesheets.diagnostics.len(), 1);
    }

    #[test]
    fn progressive_snapshot_is_restyled_for_current_logical_window_before_paint() {
        let document = parser::parse(
            "<style>p {color:blue} @media (max-width:600px) {p {color:red}}</style><p>progress</p>",
        )
        .unwrap();
        let mut frame = Frame::new(url::Url::parse("https://example.test/").unwrap(), document);
        assert_eq!(frame.media_environment(), MediaEnvironment::default());
        let font = paint::load_bundled_font().unwrap();
        let (_, list, _) = render_for_window_size(
            &mut frame,
            PhysicalSize::new(1000, 1200),
            2.0,
            &font,
            false,
            0.0,
        );
        assert!(list.iter().any(|item| matches!(item, DisplayItem::Text {text, color, ..} if text == "progress" && *color == 0xFF0000)));
        assert_eq!(frame.media_environment().viewport_width, 500.0);
    }
}
