use std::{collections::HashMap, error::Error, fmt, io::Cursor, sync::Arc};

use crate::dom::NodeId;

const MAX_DIMENSION: u32 = 4096;
const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;

pub type ImageMap = HashMap<NodeId, Arc<DecodedImage>>;

#[derive(PartialEq)]
pub struct DecodedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl fmt::Debug for DecodedImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl DecodedImage {
    pub fn decode(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some(MAX_DECODED_BYTES);
        reader.limits(limits);
        let image = reader.decode()?.to_rgba8();
        Ok(Self {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgba};

    #[test]
    fn enabled_raster_formats_decode_to_shared_rgba_shape() {
        let source = image::DynamicImage::new_rgb8(3, 2);
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::WebP,
        ] {
            let mut bytes = Cursor::new(Vec::new());
            source.write_to(&mut bytes, format).unwrap();
            let decoded = DecodedImage::decode(&bytes.into_inner()).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (3, 2), "{format:?}");
            assert_eq!(decoded.rgba().len(), 3 * 2 * 4);
        }
    }

    #[test]
    fn decodes_pixels_and_rejects_corrupt_or_oversized_images() {
        let encode = |width| {
            let image = ImageBuffer::from_pixel(width, 1, Rgba([255u8, 0, 0, 128]));
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, ImageFormat::Png).unwrap();
            bytes.into_inner()
        };
        let decoded = DecodedImage::decode(&encode(2)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        assert_eq!(decoded.rgba(), &[255, 0, 0, 128, 255, 0, 0, 128]);
        assert!(DecodedImage::decode(b"not an image").is_err());
        assert!(DecodedImage::decode(&encode(MAX_DIMENSION + 1)).is_err());
    }
}
