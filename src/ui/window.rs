use std::{num::NonZeroU32, rc::Rc};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use crate::layout::LayoutTree;
use crate::paint::{self, DisplayItem};

pub fn run(layout_tree: LayoutTree, font: fontdue::Font) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())?;

    let mut app = BrowserWindow {
        context,
        window: None,
        surface: None,
        font,
        layout_tree,
        display_list: Vec::new(),
        scroll_offset: 0.0,
        show_layout_outlines: false,
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
            self.scroll_offset = next_offset;
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

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.surface = None;
        self.window = None;
    }
}
