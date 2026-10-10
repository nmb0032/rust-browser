//! Raster images are decoded on the loader thread and shared by layout/paint.

mod images;

pub use images::{DecodedImage, ImageMap};
