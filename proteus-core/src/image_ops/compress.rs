//! Image compress (PRD §9) — T1. Lossy re-encode in the source format when the
//! result is smaller; a re-encode that cannot beat the input returns the input
//! bytes untouched (size oracle: output never grows).

use image::ImageFormat;

use crate::error::ProteusError;
use crate::image_ops::{decode_image, encode, guess_format, TargetFormat};

/// Compress an image, optionally with an explicit quality (default 75).
pub fn compress_image(input: &[u8], quality: Option<u8>) -> Result<Vec<u8>, ProteusError> {
    let quality = quality.unwrap_or(75);
    if !(1..=100).contains(&quality) {
        return Err(ProteusError::InvalidArgument {
            surface: "compress_image",
            reason: format!("quality must be within 1..=100, got {quality}"),
        });
    }
    let img = decode_image(input)?;
    let format: TargetFormat = guess_format(input)?.into();
    let out = encode(&img, format, quality)?;
    if out.len() >= input.len() {
        Ok(input.to_vec())
    } else {
        Ok(out)
    }
}

impl From<ImageFormat> for TargetFormat {
    fn from(f: ImageFormat) -> Self {
        match f {
            ImageFormat::Jpeg => TargetFormat::Jpeg,
            ImageFormat::Png => TargetFormat::Png,
            ImageFormat::WebP => TargetFormat::WebP,
            ImageFormat::Avif => TargetFormat::Avif,
            f => {
                // Anything else re-encodes as the nearest lossy PNG.
                let _ = f;
                TargetFormat::Png
            }
        }
    }
}

#[cfg(test)]
mod tests {
    
    use super::*;
    use crate::image_ops::testutil;

    fn gradient_png(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x.wrapping_mul(7)) as u8, (y.wrapping_mul(11)) as u8, ((x + y).wrapping_mul(3)) as u8]);
        }
        crate::image_ops::encode(
            &image::DynamicImage::ImageRgb8(img),
            crate::image_ops::TargetFormat::Png,
            90,
        )
        .unwrap()
    }

    #[test]
    fn compress_result_is_smaller_or_input_untouched_and_decodes() {
        let input = gradient_png(64, 64);
        let out = compress_image(&input, Some(40)).unwrap();
        assert!(out.len() <= input.len(), "{} > {}", out.len(), input.len());
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (64, 64));
    }

    #[test]
    fn compress_of_tiny_image_never_grows() {
        let input = testutil::solid_png(4, 4, [200; 3]);
        let out = compress_image(&input, Some(1)).unwrap();
        assert!(out.len() <= input.len());
    }

    #[test]
    fn invalid_quality_is_rejected() {
        let input = testutil::solid_png(4, 4, [0, 0, 0]);
        assert!(matches!(
            compress_image(&input, Some(0)).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
        assert!(matches!(
            compress_image(&input, Some(101)).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn non_image_input_rejected() {
        assert!(matches!(
            compress_image(b"garbage", None).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }
}