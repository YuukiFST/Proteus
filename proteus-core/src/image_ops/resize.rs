//! Image resize (PRD §9) — T1. Wrapper over `image`'s thinning/documented
//! resizers; validates target geometry before decoding.

use crate::error::ProteusError;
use crate::image_ops::{decode_image, encode_like};

/// Resize keeping aspect ratio; `max_dim` bounds the longer side (points/pixels).
pub fn resize_image_max_dim(input: &[u8], max_dim: u32) -> Result<Vec<u8>, ProteusError> {
    if max_dim == 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "resize_image",
            reason: "max dimension must be nonzero".into(),
        });
    }
    let img = decode_image(input)?;
    let (w, h) = (img.width(), img.height());
    if w <= max_dim && h <= max_dim {
        return Ok(input.to_vec());
    }
    let scale = (max_dim as f32 / w.max(h) as f32).min(1.0);
    let (nw, nh) = ((w as f32 * scale).round().max(1.0) as u32, (h as f32 * scale).round().max(1.0) as u32);
    let resized = img.resize(nw, nh, image::imageops::FilterType::Lanczos3);
    encode_like(input, &resized, 92)
}

/// Unified PRD §9 surface: `keep_ratio` fits within W×H preserving aspect,
/// otherwise stretches to exactly W×H.
pub fn resize_image(input: &[u8], width: u32, height: u32, keep_ratio: bool) -> Result<Vec<u8>, ProteusError> {
    if keep_ratio {
        resize_image_max_dim_in_box(input, width, height)
    } else {
        resize_image_exact(input, width, height)
    }
}

/// Fit within a box, preserving aspect ratio (both dims ≤ target).
fn resize_image_max_dim_in_box(input: &[u8], max_w: u32, max_h: u32) -> Result<Vec<u8>, ProteusError> {
    if max_w == 0 || max_h == 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "resize_image",
            reason: "box dimensions must be nonzero".into(),
        });
    }
    let img = decode_image(input)?;
    let (w, h) = (img.width(), img.height());
    let scale = (max_w as f32 / w as f32).min(max_h as f32 / h as f32).min(1.0);
    let (nw, nh) = (
        (w as f32 * scale).round().max(1.0) as u32,
        (h as f32 * scale).round().max(1.0) as u32,
    );
    if scale >= 1.0 {
        return Ok(input.to_vec());
    }
    let resized = img.resize(nw, nh, image::imageops::FilterType::Lanczos3);
    encode_like(input, &resized, 92)
}

/// Resize to an exact box (exact w×h stretch).
pub fn resize_image_exact(input: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ProteusError> {
    if width == 0 || height == 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "resize_image",
            reason: "width and height must be nonzero".into(),
        });
    }
    if width > 20_000 || height > 20_000 {
        return Err(ProteusError::InvalidArgument {
            surface: "resize_image",
            reason: "resize target too large".into(),
        });
    }
    let img = decode_image(input)?;
    let resized = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
    encode_like(input, &resized, 92)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_ops::testutil;

    #[test]
    fn downscaling_preserves_aspect_ratio_within_the_bounds() {
        let png = testutil::solid_png(800, 400, [5, 5, 5]);
        let out = resize_image_max_dim(&png, 100).unwrap();
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
    }

    #[test]
    fn smaller_than_limit_images_pass_through() {
        let png = testutil::solid_png(40, 30, [5, 5, 5]);
        let out = resize_image_max_dim(&png, 100).unwrap();
        assert_eq!(out, png);
    }

    #[test]
    fn exact_resize_sets_dimensions() {
        let png = testutil::solid_png(10, 20, [7, 7, 7]);
        let out = resize_image_exact(&png, 8, 8).unwrap();
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (8, 8));
    }

    #[test]
    fn zero_dimensions_rejected() {
        let png = testutil::solid_png(4, 4, [0, 0, 0]);
        assert!(matches!(
            resize_image_max_dim(&png, 0).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
        assert!(matches!(
            resize_image_exact(&png, 0, 8).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            resize_image_exact(b"garbage", 4, 4).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }
}