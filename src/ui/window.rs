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
use crate::paint::{self, DisplayItem};
use crate::{
    css::media::MediaEnvironment,
    dom::{Document, NodeId},
    engine::{FragmentTarget, Frame, LoadEvent, Navigation, link_url},
    layout::LayoutTree,
};

pub fn run(
    navigation: Navigation,
    font: crate::text::FontContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
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
        reconciling_interaction: false,
        animation_origin: Instant::now(),
        animation_deadline: None,
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
    reconciling_interaction: bool,
    animation_origin: Instant,
    animation_deadline: Option<Instant>,
}

impl BrowserWindow {
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

        self.rebuild_display_list();
        self.update_interaction(InteractionEvent::Pointer);
        self.request_redraw();
    }

    fn rebuild_display_list(&mut self) {
        let Some(window) = &self.window else {
            return;
        };

        let scale_factor = window.scale_factor();
        let size = window.inner_size();
        let frame = Arc::make_mut(&mut self.frame);
        if let Err(error) = frame.set_animation_time(self.animation_origin.elapsed().as_secs_f64())
        {
            eprintln!("Animation clock: {error}");
        }
        frame.set_media_environment(MediaEnvironment {
            color_scheme: window.theme().map(|theme| match theme {
                winit::window::Theme::Light => crate::css::media::ColorScheme::Light,
                winit::window::Theme::Dark => crate::css::media::ColorScheme::Dark,
            }),
            ..frame.media_environment()
        });
        let (tree, list, scroll) = render_for_window_size(
            Arc::make_mut(&mut self.frame),
            size,
            scale_factor,
            &self.font,
            self.show_layout_outlines,
            self.scroll_offset,
        );
        self.layout_tree = tree;
        self.display_list = list;
        self.scroll_offset = scroll;
        self.update_cursor();
        self.update_interaction(InteractionEvent::Pointer);
    }

    fn scroll_by(&mut self, delta: f32) {
        self.scroll_by_axes(crate::layout::ScrollOffset { x: 0.0, y: delta });
    }

    fn scroll_by_axes(&mut self, delta: crate::layout::ScrollOffset) {
        let Some(window) = &self.window else {
            return;
        };
        let scale_factor = window.scale_factor();
        let (x, y) = self.cursor.map_or((-1.0, -1.0), |(x, y)| {
            input::document_point(x, y, scale_factor, 0.0)
        });
        self.layout_tree
            .set_viewport_scroll(crate::layout::ScrollOffset {
                y: self.scroll_offset,
                ..self.layout_tree.viewport_scroll()
            });
        if self.layout_tree.scroll_wheel(x, y, delta) {
            self.fragment_scroll_pending = false;
            self.scroll_offset = self.layout_tree.viewport_scroll().y;
            Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
            if self.frame.animations().has_scroll_effects() {
                self.rebuild_display_list();
                self.request_redraw();
                return;
            }
            let logical = window.inner_size().to_logical::<f32>(scale_factor);
            self.display_list = paint::build_display_list(
                &self.layout_tree,
                logical.width,
                logical.height,
                self.show_layout_outlines,
            );
            self.update_cursor();
            self.update_interaction(InteractionEvent::Pointer);
            self.request_redraw();
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn toggle_layout_outlines(&mut self) {
        self.show_layout_outlines = !self.show_layout_outlines;
        self.rebuild_display_list();
        self.request_redraw();
    }

    fn receive_load_events(&mut self) {
        let mut document_changed = false;

        loop {
            let font_loading = self.navigation.is_font_loading();
            match self.navigation.try_recv() {
                Ok(LoadEvent::Progress(frame)) => {
                    let mut frame = frame;
                    if !frame.same_animation_document(&self.frame) {
                        self.animation_origin = Instant::now();
                        self.animation_deadline = None;
                    }
                    Arc::make_mut(&mut frame).copy_scroll_state_from(&self.frame);
                    if self.navigation.is_font_loading() {
                        let state = self.frame.interaction_state().clone();
                        let environment = self.frame.media_environment();
                        let url = self.frame.url.clone();
                        let next = Arc::make_mut(&mut frame);
                        next.url = url.clone();
                        self.navigation.history.current_mut().url = url;
                        next.set_media_environment(environment);
                        next.set_interaction_state(state);
                    }
                    for diagnostic in &frame.stylesheets.diagnostics {
                        eprintln!(
                            "CSS/resource at {:?}: {}",
                            diagnostic.node_id, diagnostic.message
                        );
                    }
                    self.frame = frame;
                    self.update_title();
                    document_changed = true;
                    // Give redraws a turn even when the loader keeps sending snapshots.
                    break;
                }
                Ok(LoadEvent::Finished) => {
                    if font_loading {
                        continue;
                    }
                    self.is_loading = false;
                    if let Some(scroll) = self.restore_scroll.take() {
                        self.set_scroll(scroll);
                    } else if self.fragment_scroll_pending {
                        self.scroll_to_fragment();
                    }
                    self.fragment_scroll_pending = false;
                    self.update_title();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.is_loading {
                        self.is_loading = false;
                        self.show_load_error("Page loader disconnected before completion".into());
                    }
                    break;
                }
            }
        }

        if document_changed {
            self.rebuild_display_list();
            self.update_interaction(InteractionEvent::Pointer);
            if self.restore_scroll.is_none() && self.fragment_scroll_pending {
                self.scroll_to_fragment();
            }
            self.request_redraw();
        }
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

    fn cursor_link(&self) -> Result<Option<url::Url>, Box<dyn std::error::Error>> {
        let (Some(window), Some((x, y))) = (&self.window, self.cursor) else {
            return Ok(None);
        };
        let (x, y) = input::document_point(x, y, window.scale_factor(), 0.0);
        let Some(node) = self.layout_tree.hit_test_viewport(x, y, self.scroll_offset) else {
            return Ok(None);
        };
        link_url(&self.frame.document, node, &self.frame.url)
    }

    fn update_cursor(&self) {
        if let Some(window) = &self.window {
            window.set_cursor(if matches!(self.cursor_link(), Ok(Some(_))) {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
        }
    }

    fn update_interaction(&mut self, event: InteractionEvent) {
        if self.reconciling_interaction {
            return;
        }
        self.reconciling_interaction = true;
        // Geometry-dependent hover rules can move the hit target back and
        // forth. Reconcile iteratively rather than recursively re-rendering.
        for pass in 0..4 {
            let point = self
                .window
                .as_ref()
                .zip(self.cursor)
                .map(|(window, (x, y))| input::document_point(x, y, window.scale_factor(), 0.0));
            if !update_interaction_for_window_scrolled(
                Arc::make_mut(&mut self.frame),
                &self.layout_tree,
                point,
                self.scroll_offset,
                if pass == 0 {
                    event
                } else {
                    InteractionEvent::Pointer
                },
            ) {
                self.reconciling_interaction = false;
                return;
            }
            self.rebuild_display_list();
            self.request_redraw();
            if matches!(event, InteractionEvent::Leave | InteractionEvent::Blur) {
                self.reconciling_interaction = false;
                return;
            }
        }
        self.reconciling_interaction = false;
        let message = "pointer/style/layout feedback did not stabilize within 4 passes; retaining the last consistent state/layout pair";
        eprintln!("CSS: {message}");
        Arc::make_mut(&mut self.frame)
            .stylesheets
            .diagnostics
            .push(crate::style::Diagnostic {
                node_id: None,
                message: message.into(),
            });
    }

    fn set_scroll(&mut self, offset: f32) {
        let Some(window) = &self.window else {
            return;
        };
        let height = window
            .inner_size()
            .to_logical::<f32>(window.scale_factor())
            .height;
        self.scroll_offset = offset.clamp(0.0, paint::max_scroll_offset(&self.layout_tree, height));
        self.layout_tree
            .set_viewport_scroll(crate::layout::ScrollOffset {
                y: self.scroll_offset,
                ..self.layout_tree.viewport_scroll()
            });
        self.scroll_offset = self.layout_tree.viewport_scroll().y;
        Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
        if self.frame.animations().has_scroll_effects() {
            self.rebuild_display_list();
        }
        self.update_cursor();
        self.update_interaction(InteractionEvent::Pointer);
        self.request_redraw();
    }

    fn scroll_to_fragment(&mut self) {
        let Some(fragment) = self.navigation.history.current().url.fragment() else {
            return;
        };
        match self.frame.fragment_target(fragment) {
            Some(FragmentTarget::Top) => self.set_scroll(0.0),
            Some(FragmentTarget::Node(id)) => {
                if let Some(rect) = self.layout_tree.node_rect(id) {
                    self.set_scroll(rect.y);
                }
            }
            None => {}
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
            Arc::make_mut(&mut self.frame).url = url;
            Arc::make_mut(&mut self.frame).refresh_target();
            self.rebuild_display_list();
            self.scroll_to_fragment();
            self.fragment_scroll_pending = false;
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

fn update_interaction_for_window_scrolled(
    frame: &mut Frame,
    tree: &LayoutTree,
    point: Option<(f32, f32)>,
    scroll: f32,
    event: InteractionEvent,
) -> bool {
    let mut state = frame.interaction_state().clone();
    let mut hit = point.and_then(|(x, y)| tree.hit_test_viewport(x, y, scroll));
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
    frame.set_interaction_state(state)
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
        self.frame = Arc::new(Frame::new(
            self.navigation.history.current().url.clone(),
            Document::new(),
        ));
        self.layout_tree = self.frame.layout_tree();
        self.is_loading = true;
        self.rebuild_display_list();
        self.update_title();
        self.request_redraw();
        if let Err(error) = self.navigation.start_load() {
            self.is_loading = false;
            self.show_load_error(format!("Could not start page loader: {error}"));
        }
    }

    fn show_load_error(&mut self, error: String) {
        eprintln!("{error}");
        self.frame = Arc::new(Frame::error_page(
            self.navigation.history.current().url.clone(),
            error,
        ));
        self.layout_tree = self.frame.layout_tree();
        self.rebuild_display_list();
        self.update_title();
        self.request_redraw();
    }
}

/// Shared by progress, resize, and display-scale changes. Never touches the
/// loader or navigation history; all retained resources survive the restyle.
fn render_for_window_size(
    frame: &mut Frame,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    font: &crate::text::FontContext,
    outlines: bool,
    scroll: f32,
) -> (LayoutTree, Vec<DisplayItem>, f32) {
    let logical = size.to_logical::<f32>(scale_factor);
    let environment = MediaEnvironment {
        viewport_width: logical.width,
        viewport_height: logical.height,
        resolution_dppx: Some(scale_factor as f32),
        ..frame.media_environment()
    };
    frame.set_media_environment(environment);
    let mut tree = frame.layout_with_container_queries(font);
    let scroll = scroll.clamp(0.0, paint::max_scroll_offset(&tree, logical.height));
    tree.set_viewport_scroll(crate::layout::ScrollOffset {
        y: scroll,
        ..tree.viewport_scroll()
    });
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
        let surface = Surface::new(&self.context, Rc::clone(&window))
            .expect("failed to create software surface");

        self.window = Some(window);
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
                if let Some((bar, grab)) = self.scrollbar_drag {
                    let scale = self.window.as_ref().expect("active window").scale_factor();
                    let (x, y) = input::document_point(position.x, position.y, scale, 0.0);
                    if self.layout_tree.drag_scrollbar(bar, x, y, grab) {
                        Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
                        self.rebuild_display_list();
                        self.request_redraw();
                    }
                    return;
                }
                self.update_interaction(InteractionEvent::Pointer);
                self.update_cursor();
            }
            WindowEvent::CursorLeft { .. } => {
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
                if state == ElementState::Released && self.scrollbar_drag.take().is_some() {
                    self.update_interaction(InteractionEvent::Pointer);
                    return;
                }
                if state == ElementState::Pressed
                    && let Some((x, y)) = self.cursor
                {
                    let scale = self.window.as_ref().expect("active window").scale_factor();
                    let (x, y) = input::document_point(x, y, scale, 0.0);
                    if let Some(bar) = self.layout_tree.scrollbar_at(x, y) {
                        let grab = self.layout_tree.scrollbar_grab(bar, x, y);
                        self.scrollbar_drag = Some((bar, grab));
                        self.pressed_link = None;
                        if self.layout_tree.drag_scrollbar(bar, x, y, grab) {
                            Arc::make_mut(&mut self.frame).retain_scroll_state(&self.layout_tree);
                            self.rebuild_display_list();
                            self.request_redraw();
                        }
                        return;
                    }
                }
                let link = self.cursor_link();
                self.update_interaction(if state == ElementState::Pressed {
                    InteractionEvent::Press
                } else {
                    InteractionEvent::Release
                });
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
                self.rebuild_display_list();
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = self
                    .window
                    .as_ref()
                    .expect("window event must belong to the active window")
                    .inner_size();
                self.resize_surface(size.width, size.height);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale_factor = self
                    .window
                    .as_ref()
                    .expect("window event must belong to the active window")
                    .scale_factor() as f32;
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
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
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
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let scale_factor = self
                    .window
                    .as_ref()
                    .expect("window event must belong to the active window")
                    .scale_factor();
                if let Some(surface) = &mut self.surface {
                    let mut buffer = surface
                        .buffer_mut()
                        .expect("failed to get software framebuffer");
                    let width = buffer.width().get() as usize;
                    let height = buffer.height().get() as usize;

                    buffer.fill(0x00FF_FFFF);
                    paint::rasterize_display_list(
                        &self.display_list,
                        &self.font,
                        &mut buffer,
                        width,
                        height,
                        scale_factor,
                        self.scroll_offset,
                    );
                    buffer.present().expect("failed to present framebuffer");
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.receive_load_events();
        if self.frame.animations().needs_tick() && self.window.is_some() {
            let now = Instant::now();
            if self
                .animation_deadline
                .is_none_or(|deadline| now >= deadline)
            {
                let seconds = now.duration_since(self.animation_origin).as_secs_f64();
                if let Err(error) = Arc::make_mut(&mut self.frame).set_animation_time(seconds) {
                    eprintln!("Animation clock: {error}");
                } else {
                    self.rebuild_display_list();
                    self.request_redraw();
                }
                self.animation_deadline = Some(now + Duration::from_millis(16));
            }
        } else {
            self.animation_deadline = None;
        }
        if !self.is_loading
            && !self.navigation.is_font_loading()
            && (self.frame.has_pending_web_fonts() || self.frame.has_pending_effect_images())
            && let Err(error) = self.navigation.start_font_load(Arc::clone(&self.frame))
        {
            eprintln!("Could not start font loader: {error}");
        }
        if self.frame.animations().needs_tick() && self.window.is_some() {
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
