mod decoration;
mod display_list;
mod rasterizer;

pub use display_list::DisplayItem;
#[cfg(test)]
pub(crate) use display_list::load_bundled_font;
pub(crate) use display_list::{
    DEFAULT_VIEWPORT_HEIGHT, DEFAULT_VIEWPORT_WIDTH, LINE_HEIGHT, build_display_list,
    max_scroll_offset,
};
pub(crate) use rasterizer::rasterize_display_list;
