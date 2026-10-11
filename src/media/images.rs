use std::{collections::HashMap, error::Error, fmt, io::Cursor, sync::Arc};

use crate::dom::NodeId;
use image::ImageDecoder;

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
        let mut decoder = reader.into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut image = image::DynamicImage::from_decoder(decoder)?;
        image.apply_orientation(orientation);
        let image = image.to_rgba8();
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
    #[test]
    fn jpeg_exif_orientation_changes_natural_dimensions_before_layout() {
        let source = image::DynamicImage::new_rgb8(3, 2);
        let mut bytes = Cursor::new(Vec::new());
        source
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .unwrap();
        // Original minimal little-endian Exif IFD: orientation=6 (90° CW).
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut encoded = vec![0xff, 0xd8, 0xff, 0xe1];
        encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        encoded.extend_from_slice(exif);
        encoded.extend_from_slice(&bytes.get_ref()[2..]);
        let image = DecodedImage::decode(&encoded).unwrap();
        assert_eq!((image.width(), image.height()), (2, 3));
        assert_eq!(image.rgba().len(), 24);
    }
}
