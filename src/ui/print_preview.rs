//! Read-only extension of the native window: physical white sheets on a neutral
//! work surface, wheel/keyboard navigation, no simulated HTML print widgets.
use crate::{engine::PrintLayout, screenshot};
use softbuffer::{Context, Surface};
use std::{num::NonZeroU32, rc::Rc};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, OwnedDisplayHandle},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

pub fn run(layout: PrintLayout) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())?;
    let mut preview = Preview {
        layout,
        context,
        window: None,
        surface: None,
        scroll: 0.0,
    };
    event_loop.run_app(&mut preview)?;
    Ok(())
}
struct Preview {
    layout: PrintLayout,
    context: Context<OwnedDisplayHandle>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    scroll: f32,
}
impl Preview {
    fn fit(&self) -> f32 {
        let width = self.window.as_ref().map_or(800.0, |w| {
            w.inner_size().width as f32 / w.scale_factor() as f32
        });
        let sheet = self
            .layout
            .pages
            .iter()
            .map(|p| p.sheet.width)
            .fold(1.0, f32::max);
        ((width - 48.0).max(1.0) / sheet).clamp(0.125, 1.0)
    }
    fn move_scroll(&mut self, delta: f32) {
        let viewport = self.window.as_ref().map_or(0.0, |w| {
            w.inner_size().height as f32 / w.scale_factor() as f32
        });
        let fit = self.fit();
        let extent = self
            .layout
            .pages
            .iter()
            .map(|p| p.sheet.height * fit + 24.0)
            .sum::<f32>()
            + 24.0;
        self.scroll = (self.scroll + delta).clamp(0.0, (extent - viewport).max(0.0));
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let fit = f64::from(self.fit());
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        surface.resize(width, height)?;
        let scale = window.scale_factor();
        let mut buffer = surface.buffer_mut()?;
        buffer.fill(0x55585d);
        let mut top = (24.0 - f64::from(self.scroll)) * scale;
        for page in &self.layout.pages {
            let page_height = (f64::from(page.sheet.height) * scale * fit).ceil();
            if top + page_height > 0.0 && top < f64::from(size.height) {
                let image = screenshot::render_print_page(&self.layout, page.number, scale * fit)?;
                let left = ((i64::from(size.width) - i64::from(image.width)) / 2)
                    .max((24.0 * scale) as i64);
                let y = top.round() as i64;
                for sy in 0..image.height {
                    let dy = y + i64::from(sy);
                    if !(0..i64::from(size.height)).contains(&dy) {
                        continue;
                    }
                    for sx in 0..image.width.min(size.width.saturating_sub(left as u32)) {
                        let source = (sy as usize * image.width as usize + sx as usize) * 4;
                        let target =
                            dy as usize * size.width as usize + left as usize + sx as usize;
                        buffer[target] = (u32::from(image.rgba[source]) << 16)
                            | (u32::from(image.rgba[source + 1]) << 8)
                            | u32::from(image.rgba[source + 2]);
                    }
                }
            }
            top += page_height + 24.0 * scale;
        }
        buffer.present()?;
        Ok(())
    }
}
impl ApplicationHandler for Preview {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let first = &self.layout.pages[0];
        let window = Rc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!(
                            "Print preview — {} pages · wheel / PageUp / PageDown",
                            self.layout.pages.len()
                        ))
                        .with_inner_size(LogicalSize::new(
                            (first.sheet.width + 48.0).min(1200.0),
                            (first.sheet.height + 48.0).min(900.0),
                        )),
                )
                .expect("create native print preview"),
        );
        self.surface =
            Some(Surface::new(&self.context, window.clone()).expect("create native print surface"));
        self.window = Some(window.clone());
        window.request_redraw();
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    eprintln!("Print preview: {error}");
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.move_scroll(0.0)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 40.0,
                    MouseScrollDelta::PixelDelta(p) => {
                        -p.y as f32
                            / self
                                .window
                                .as_ref()
                                .map_or(1.0, |w| w.scale_factor() as f32)
                    }
                };
                self.move_scroll(delta);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let page = self.window.as_ref().map_or(600.0, |w| {
                    w.inner_size().height as f32 / w.scale_factor() as f32
                });
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Named(NamedKey::PageDown) => self.move_scroll(page * 0.9),
                    Key::Named(NamedKey::PageUp) => self.move_scroll(-page * 0.9),
                    Key::Named(NamedKey::ArrowDown) => self.move_scroll(40.0),
                    Key::Named(NamedKey::ArrowUp) => self.move_scroll(-40.0),
                    Key::Named(NamedKey::Home) => self.move_scroll(-f32::MAX),
                    Key::Named(NamedKey::End) => self.move_scroll(f32::MAX),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
