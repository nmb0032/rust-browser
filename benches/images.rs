use std::{
    error::Error,
    hint::black_box,
    io::Cursor,
    time::{Duration, Instant},
};

use image::{ImageBuffer, ImageFormat, Rgba};
use rust_browser::media::DecodedImage;

const SAMPLES: usize = 9;

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "Image decoding medians of {SAMPLES} samples; encoding, networking, and destruction excluded."
    );
    for (width, height) in [(64, 64), (1024, 768)] {
        let source = ImageBuffer::from_fn(width, height, |x, y| Rgba([x as u8, y as u8, 128, 255]));
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, ImageFormat::Png)?;
        let bytes = bytes.into_inner();
        let reference = DecodedImage::decode(&bytes)?;
        assert_eq!((reference.width(), reference.height()), (width, height));
        let mut samples = Vec::new();
        for _ in 0..SAMPLES {
            let start = Instant::now();
            let image = DecodedImage::decode(black_box(&bytes))?;
            black_box(&image);
            samples.push(start.elapsed());
        }
        samples.sort_unstable();
        println!("{width}x{height}: {:.3} ms", millis(samples[SAMPLES / 2]));
    }
    Ok(())
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
