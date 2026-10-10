use std::{
    num::NonZeroU32,
    rc::Rc,
    sync::{Arc, mpsc::TryRecvError},
    time::{Duration, Instant},
};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowId},
};

use super::input::{self, HistoryDirection};
use crate::paint::{self, DisplayItem};
use crate::{
    dom::Document,
    engine::{FragmentTarget, Frame, LoadEvent, Navigation, link_url},
    layout::LayoutTree,
};

pub fn run(navigation: Navigation, font: fontdue::Font) -> Result<(), Box<dyn std::error::Error>> {
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
        modifiers: ModifiersState::empty(),
        restore_scroll: None,
        fragment_scroll_pending: true,
        is_loading: true,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct BrowserWindow {
    context: Context<OwnedDisplayHandle>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    font: fontdue::Font,
    layout_tree: LayoutTree,
    display_list: Vec<DisplayItem>,
    scroll_offset: f32,
    show_layout_outlines: bool,
    navigation: Navigation,
    frame: Arc<Frame>,
    cursor: Option<(f64, f64)>,
    pressed_link: Option<url::Url>,
    modifiers: ModifiersState,
    restore_scroll: Option<f32>,
    fragment_scroll_pending: bool,
    is_loading: bool,
}

impl BrowserWindow {
    fn resize_surface(&mut self, width: u32, height: u32) {
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return;
        };

        if let Some(surface) = &mut self.surface {
            surface
                .resize(width, height)
                .expect("failed to resize software surface");
        }

        self.rebuild_display_list();
        self.request_redraw();
    }

    fn rebuild_display_list(&mut self) {
        let Some(window) = &self.window else {
            return;
        };

        let scale_factor = window.scale_factor();
        let logical_size = window.inner_size().to_logical::<f32>(scale_factor);
        self.layout_tree.layout(logical_size.width, &self.font);
        self.display_list = paint::build_display_list(
            &self.layout_tree,
            logical_size.width,
            logical_size.height,
            self.show_layout_outlines,
        );
        self.scroll_offset = self.scroll_offset.min(paint::max_scroll_offset(
            &self.layout_tree,
            logical_size.height,
        ));
        self.update_cursor();
    }

    fn scroll_by(&mut self, delta: f32) {
        let Some(window) = &self.window else {
            return;
        };
        let scale_factor = window.scale_factor();
        let logical_height = window.inner_size().to_logical::<f32>(scale_factor).height;
        let max_scroll = paint::max_scroll_offset(&self.layout_tree, logical_height);
        let next_offset = (self.scroll_offset + delta).clamp(0.0, max_scroll);

        if next_offset != self.scroll_offset {
            self.fragment_scroll_pending = false;
            self.scroll_offset = next_offset;
            self.update_cursor();
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
            match self.navigation.try_recv() {
                Ok(LoadEvent::Progress(frame)) => {
                    self.layout_tree = frame.layout_tree();
                    self.frame = frame;
                    self.update_title();
                    document_changed = true;
                    // Give redraws a turn even when the loader keeps sending snapshots.
                    break;
                }
                Ok(LoadEvent::Finished) => {
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
        let (x, y) = input::document_point(x, y, window.scale_factor(), self.scroll_offset);
        let Some(node) = self.layout_tree.hit_test(x, y) else {
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

    fn set_scroll(&mut self, offset: f32) {
        let Some(window) = &self.window else {
            return;
        };
        let height = window
            .inner_size()
            .to_logical::<f32>(window.scale_factor())
            .height;
        self.scroll_offset = offset.clamp(0.0, paint::max_scroll_offset(&self.layout_tree, height));
        self.update_cursor();
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
            self.scroll_to_fragment();
            self.fragment_scroll_pending = false;
            self.update_title();
        } else {
            self.begin_navigation(None);
        }
    }

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
                self.update_cursor();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.pressed_link = None;
                self.update_cursor();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match self.cursor_link() {
                Ok(link) if state == ElementState::Pressed => self.pressed_link = link,
                Ok(Some(link)) if self.pressed_link.take().as_ref() == Some(&link) => {
                    self.navigate(link)
                }
                Ok(_) => self.pressed_link = None,
                Err(error) => {
                    self.pressed_link = None;
                    eprintln!("link navigation failed: {error}");
                }
            },
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize_surface(size.width, size.height),
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
                    MouseScrollDelta::LineDelta(_, y) => -y * paint::LINE_HEIGHT,
                    MouseScrollDelta::PixelDelta(position) => -(position.y as f32) / scale_factor,
                };
                self.scroll_by(scroll_delta);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(direction) = input::history_shortcut(&event.logical_key, self.modifiers)
                {
                    self.traverse_history(direction);
                    return;
                }
                match &event.logical_key {
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
        if self.is_loading {
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
