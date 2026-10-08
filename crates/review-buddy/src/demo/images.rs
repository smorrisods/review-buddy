//! The pictures in the demo descriptions. They are drawn here, in code, as small PNGs, so demo
//! mode shows images with no network, no files and nothing to license.

use std::io::Cursor;
use std::sync::Arc;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};

use crate::images::fetch::{decode, Failure, Limits};

/// The `.invalid` top-level domain never resolves, so a demo address can't reach anything.
pub const BEFORE: &str = "https://demo.invalid/screenshots/contrast-before.png";
pub const AFTER: &str = "https://demo.invalid/screenshots/contrast-after.png";

const WIDTH: u32 = 240;
const HEIGHT: u32 = 120;

/// A mock of the light surface with rows of muted text; `shade` is how dark the text is.
fn screenshot(shade: u8) -> Vec<u8> {
    let image = ImageBuffer::from_fn(WIDTH, HEIGHT, |x, y| {
        let surface = Rgb([0xf6, 0xf3, 0xec]);
        let title_bar = y < 16;
        let text_row = y >= 28 && (y - 28) % 18 < 7 && y < HEIGHT - 10;
        let length = 60 + ((y - 28.min(y)) / 18 * 37) % 130;
        if title_bar {
            Rgb([0x3d, 0x4f, 0x5c])
        } else if text_row && (12..12 + length).contains(&x) {
            Rgb([shade, shade, shade.saturating_add(8)])
        } else {
            surface
        }
    });
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut out, ImageFormat::Png)
        .expect("a PNG encodes into memory");
    out.into_inner()
}

/// The bytes behind a demo address, if it is one.
pub fn bytes(url: &str) -> Option<Vec<u8>> {
    match url {
        BEFORE => Some(screenshot(0xc4)),
        AFTER => Some(screenshot(0x6b)),
        _ => None,
    }
}

/// A demo address as a decoded image. Anything else is a calm failure, never a request.
pub fn load(url: &str) -> Result<Arc<DynamicImage>, Failure> {
    let bytes = bytes(url).ok_or(Failure::Status(404))?;
    decode(&bytes, &Limits::default()).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_pictures_are_tiny_valid_pngs() {
        for url in [BEFORE, AFTER] {
            let bytes = bytes(url).unwrap();
            assert!(bytes.len() < 4096, "{url} is {} bytes", bytes.len());
            let img = load(url).unwrap();
            assert_eq!((img.width(), img.height()), (WIDTH, HEIGHT));
        }
        assert_ne!(bytes(BEFORE), bytes(AFTER));
    }

    #[test]
    fn other_addresses_are_not_found_and_never_fetched() {
        assert_eq!(
            load("https://example.com/a.png").unwrap_err(),
            Failure::Status(404)
        );
    }
}
