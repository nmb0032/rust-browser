//! Raster images are decoded on the loader thread and shared by layout/paint.

mod color;
mod images;

pub use color::ImageColorSource;
pub use images::{DecodedImage, ImageMap};
