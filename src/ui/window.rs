use std::{error::Error, num::NonZeroU32, rc::Rc};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    window::{Window, WindowId},
};

pub fn run() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())?;

    // Load font data
    let font = load_font_data()?;

    let mut app = BrowserWindow {
        context,
        window: None,
        surface: None,
        font,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

fn load_font_data() -> Result<fontdue::Font, Box<dyn Error>> {
    let font_data = include_bytes!("..\\..\\assets\\fonts\\noto-sans\\NotoSans.ttf");
    let font = fontdue::Font::from_bytes(font_data.as_slice(), fontdue::FontSettings::default())
        .map_err(|error| std::io::Error::other(format!("failed to load bundled font: {error}")))?;
    Ok(font)
}

fn draw_text(
    font: &fontdue::Font,
    pixels: &mut [u32],
    width: usize,
    height: usize,
    text: &str,
    start_x: i32,
    baseline_y: i32,
    px: f32,
) {
    let mut pen_x = start_x;

    for character in text.chars() {
        let (metrics, bitmap) = font.rasterize(character, px);
        let top = baseline_y - (metrics.ymin + metrics.height as i32);

        for glyph_y in 0..metrics.height {
            for glyph_x in 0..metrics.width {
                let x = pen_x + metrics.xmin + glyph_x as i32;
                let y = top + glyph_y as i32;

                if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                    continue;
                }

                let coverage = bitmap[glyph_y * metrics.width + glyph_x] as u32;
                let gray = 255 - coverage;
                pixels[y as usize * width + x as usize] = (gray << 16) | (gray << 8) | gray;
            }
        }

        pen_x += metrics.advance_width.round() as i32;
    }
}

struct BrowserWindow {
    context: Context<OwnedDisplayHandle>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    font: fontdue::Font,
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

        if let Some(window) = &self.window {
            window.request_redraw();
        }
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
                .create_window(Window::default_attributes().with_title("rust-browser"))
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
            WindowEvent::RedrawRequested => {
                if let Some(surface) = &mut self.surface {
                    let mut buffer = surface
                        .buffer_mut()
                        .expect("failed to get software framebuffer");

                    let width = buffer.width().get() as usize;
                    let height = buffer.height().get() as usize;

                    buffer.fill(0x00FF_FFFF);
                    draw_text(
                        &self.font,
                        &mut buffer,
                        width,
                        height,
                        "Hello, browser",
                        24,
                        64,
                        32.0,
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
