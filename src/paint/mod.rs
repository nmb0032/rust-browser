pub mod compositor;
mod decoration;
mod display_list;
pub(crate) mod rasterizer;

pub use display_list::DisplayItem;
pub use display_list::build_display_list;
#[cfg(test)]
pub(crate) use display_list::load_bundled_font;
pub(crate) use display_list::{
    DEFAULT_VIEWPORT_HEIGHT, DEFAULT_VIEWPORT_WIDTH, LINE_HEIGHT, max_scroll_offset,
};
pub use rasterizer::rasterize_display_list;
pub(crate) use rasterizer::rasterize_display_list_profile;
