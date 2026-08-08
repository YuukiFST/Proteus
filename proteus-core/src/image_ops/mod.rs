//! Image operations (PRD §5,§9) — `image` crate. Every surface is T1 and
//! follows the same pipeline: cap check → decode (MalformedInput on garbage)
//! → transform → encode (same format unless converting) → return bytes.

pub mod compress;
pub mod convert;
pub mod crop;
pub mod resize;
pub mod rotate;
pub mod watermark;

use crate::error::ProteusError;
use crate::check_input_size;
use image::DynamicImage;

pub use compress::compress_image;
pub use convert::convert_image;
pub use convert::ImageFormat;
pub use convert::ImageFormat as TargetFormat;
pub use crop::crop_image;
pub use resize::resize_image;
pub use rotate::rotate_image;
pub use watermark::watermark_image;

/// Decode any supported image format from bytes (500 MB cap enforced).
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, ProteusError> {
    check_input_size(bytes)?;
    image::load_from_memory(bytes)
        .map_err(|e| ProteusError::MalformedInput(format!("cannot decode image: {e}")))
}

/// Encode to the format of the source image (round-trip default for edits).
pub fn encode_like(original: &[u8], img: &DynamicImage, quality: u8) -> Result<Vec<u8>, ProteusError> {
    let format = guess_format(original)?;
    encode(img, format.into(), quality)
}

/// Encode an image to the given format with a quality flavor (JPEG only).
pub fn encode(img: &DynamicImage, format: TargetFormat, quality: u8) -> Result<Vec<u8>, ProteusError> {
    let mut out = std::io::Cursor::new(Vec::new());
    match format {
        TargetFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
                .encode_image(img)
                .map_err(proteus_image_err)?;
        }
        other => {
            // write_buffer_with_format demands the buffer length match the
            // declared color type exactly — normalize to RGBA8 first.
            let rgba = img.to_rgba8();
            image::write_buffer_with_format(
                &mut out,
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
                other.into(),
            )
            .map_err(proteus_image_err)?;
        }
    }
    Ok(out.into_inner())
}

/// The image format of the input bytes.
pub fn guess_format(bytes: &[u8]) -> Result<image::ImageFormat, ProteusError> {
    image::guess_format(bytes)
        .map_err(|e| ProteusError::MalformedInput(format!("unknown image format: {e}")))
}

fn proteus_image_err(e: image::ImageError) -> ProteusError {
    ProteusError::Image(Box::new(e))
}

/// Non-blank pixel census (oracles about rendered content).
pub fn non_blank_ratio(img: &DynamicImage) -> f64 {
    let rgb = img.to_rgb8();
    let mut non_blank = 0u64;
    for p in rgb.pixels() {
        if p[0] < 250 || p[1] < 250 || p[2] < 250 {
            non_blank += 1;
        }
    }
    let w = rgb.width().max(1) as u64;
    let h = rgb.height().max(1) as u64;
    non_blank as f64 / (w * h) as f64
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// Solid-color PNG of the given size (rgb channel constants).
    pub fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb(rgb));
        super::encode(&image::DynamicImage::ImageRgb8(img), TargetFormat::Png, 90).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_non_images_cleanly() {
        assert!(matches!(
            decode_image(b"not an image").unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }

    #[test]
    fn decode_accepts_png_bytes() {
        let png = testutil::solid_png(4, 4, [10, 20, 30]);
        let img = decode_image(&png).unwrap();
        assert_eq!((img.width(), img.height()), (4, 4));
    }
}