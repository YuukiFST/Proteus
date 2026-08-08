//! Image crop (PRD §9) — T1. Normalized rectangle crop with validation.

use crate::error::ProteusError;
use crate::image_ops::{decode_image, encode_like};

/// Crop rect is normalized absolute pixel coordinates
/// (0..=1 fractions or absolute pixels from top-left).
pub fn crop_image(
    input: &[u8],
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ProteusError> {
    if width == 0 || height == 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "crop_image",
            reason: "crop width and height must be nonzero".into(),
        });
    }
    let img = decode_image(input)?;
    let (iw, ih) = (img.width(), img.height());
    if x >= iw || y >= ih {
        return Err(ProteusError::InvalidArgument {
            surface: "crop_image",
            reason: format!("crop origin ({x},{y}) outside image ({iw}x{ih})"),
        });
    }
    let w = width.min(iw.saturating_sub(x));
    let h = height.min(ih.saturating_sub(y));
    if w == 0 || h == 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "crop_image",
            reason: "crop fully outside image".into(),
        });
    }
    let cropped = img.crop_imm(x, y, w, h);
    encode_like(input, &cropped, 92)
}

#[cfg(test)]
mod tests {
    
    use super::*;
    use crate::image_ops::testutil;

    #[test]
    fn crop_of_two_tone_image_shows_origin_offset() {
        // prepare 4x4: top-left white, bottom-right black
        let mut img = image::RgbImage::new(4, 4);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([if x < 2 && y < 2 { 250u8 } else { 10u8 }, 0, 0]);
        }
        let png = crate::image_ops::encode(
            &image::DynamicImage::ImageRgb8(img),
            crate::image_ops::TargetFormat::Png,
            90,
        )
        .unwrap();
        let out = crop(&png, 2, 2, 2, 2).unwrap();
        let cropped = decode_image(&out).unwrap().to_rgb8();
        assert_eq!((cropped.width(), cropped.height()), (2, 2));
        assert!(cropped.iter().all(|&c| c < 30), "must be the dark quadrant");
    }

    #[test]
    fn crop_clamped_to_image_bounds() {
        let png = testutil::solid_png(10, 10, [3, 3, 3]);
        let out = crop(&png, 8, 8, 20, 20).unwrap();
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (2, 2));
    }

    #[test]
    fn invalid_crop_rects_rejected() {
        let png = testutil::solid_png(10, 10, [0, 0, 0]);
        for args in [(0, 0, 0, 5), (5, 5, 0, 5), (10, 0, 1, 1), (0, 10, 1, 1)] {
            assert!(crop(&png, args.0, args.1, args.2, args.3).is_err());
        }
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            crop(b"garbage", 0, 0, 1, 1).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }

    fn crop(input: &[u8], x: u32, y: u32, w: u32, h: u32) -> Result<Vec<u8>, ProteusError> {
        super::crop_image(input, x, y, w, h)
    }
}