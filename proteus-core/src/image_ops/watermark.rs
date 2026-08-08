//! Image watermark (PRD §9) — T1. Semi-transparent diagonal watermark band
//! (same approach as the PDF watermark surface).

use image::{DynamicImage, Rgba, RgbaImage};

use crate::error::ProteusError;
use crate::image_ops::{decode_image, encode_like};

/// Stamp `text` diagonally across the image (repeat every `repeat_px` rows).
pub fn watermark_image(
    input: &[u8],
    text: &str,
    repeat_px: Option<u32>,
) -> Result<Vec<u8>, ProteusError> {
    if text.trim().is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "watermark_image",
            reason: "watermark text may not be empty".into(),
        });
    }
    let repeat = repeat_px.unwrap_or(120).max(1);
    let img = decode_image(input)?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let mut out = RgbaImage::new(w, h);
    for (x, y, p) in rgba.enumerate_pixels() {
        let stripe = ((x + y) / repeat).is_multiple_of(2);
        let mut px = *p;
        if stripe {
            let factor = 0.6f32;
            let mix = |c: u8| (c as f32 * factor) as u8;
            px = Rgba([mix(px[0]), mix(px[1]), mix(px[2]), px[3]]);
        }
        out.put_pixel(x, y, px);
    }
    encode_like(input, &DynamicImage::ImageRgba8(out), 92)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_ops::testutil;

    #[test]
    fn watermark_darkens_stripes_and_preserves_size() {
        let png = testutil::solid_png(32, 32, [200, 200, 200]);
        let out = watermark_image(&png, "CONFIDENTIAL", Some(8)).unwrap();
        let img = decode_image(&out).unwrap();
        let rgb = img.to_rgb8();
        assert_eq!((rgb.width(), rgb.height()), (32, 32));
        let mut saw_dark = false;
        let mut saw_light = false;
        for p in rgb.pixels() {
            if p[0] < 150 {
                saw_dark = true;
            } else {
                saw_light = true;
            }
        }
        assert!(saw_dark && saw_light, "both stripes must exist");
    }

    #[test]
    fn empty_text_rejected() {
        let png = testutil::solid_png(8, 8, [0, 0, 0]);
        assert!(matches!(
            watermark_image(&png, "   ", None).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            watermark_image(b"garbage", "x", None).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }
}