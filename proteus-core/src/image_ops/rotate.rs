//! Image rotate (PRD §9) — T1. 90° steps are lossless; arbitrary angles
//! composite onto a canvas with a background colour.

use image::{DynamicImage, Rgba, RgbaImage};

use crate::error::ProteusError;
use crate::image_ops::{decode_image, encode_like};

/// Rotate by a multiple of 90°.
pub fn rotate_image_90(input: &[u8], quarter_turns: i32) -> Result<Vec<u8>, ProteusError> {
    let img = decode_image(input)?;
    let rotated = match quarter_turns.rem_euclid(4) {
        0 => img.clone(),
        1 | -3 => img.rotate90(),
        2 | -2 => img.rotate180(),
        _ => img.rotate270(),
    };
    encode_like(input, &rotated, 92)
}

/// Rotate by an arbitrary angle in degrees (counter-clockwise); multiples of
/// 90° take the lossless native path.
pub fn rotate_image(input: &[u8], degrees: f32) -> Result<Vec<u8>, ProteusError> {
    if !degrees.is_finite() || !(-360.0..=360.0).contains(&degrees) {
        return Err(ProteusError::InvalidArgument {
            surface: "rotate_image",
            reason: format!("angle must be within -360..=360 degrees, got {degrees}"),
        });
    }
    let d = ((degrees % 360.0) + 360.0) % 360.0;
    let img = decode_image(input)?;
    let rotated = if (d - 90.0).abs() < 1e-6 {
        img.rotate90()
    } else if (d - 180.0).abs() < 1e-6 {
        img.rotate180()
    } else if (d - 270.0).abs() < 1e-6 {
        img.rotate270()
    } else if (d - 0.0).abs() < 1e-6 {
        img
    } else {
        rotate_by_angle(&img, d)
    };
    encode_like(input, &rotated, 92)
}

/// Generic-angle rotation: inverse-map with bilinear sampling onto a
/// white-canvased bounding box (image 0.25 has no arbitrary-angle op).
fn rotate_by_angle(img: &DynamicImage, degrees: f32) -> DynamicImage {
    let src = img.to_rgba8();
    let (w, h) = (src.width() as f32, src.height() as f32);
    let rad = degrees.to_radians();
    let (cos, sin) = (rad.cos(), rad.sin());
    let nw = (w * cos.abs() + h * sin.abs()).ceil().max(1.0) as u32;
    let nh = (w * sin.abs() + h * cos.abs()).ceil().max(1.0) as u32;
    let (cx, cy) = (nw as f32 / 2.0, nh as f32 / 2.0);
    let (sxc, syc) = (w / 2.0, h / 2.0);
    let mut out = RgbaImage::new(nw, nh);
    for y in 0..nh {
        for x in 0..nw {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let sx = dx * cos + dy * sin + sxc;
            let sy = -dx * sin + dy * cos + syc;
            if sx >= 0.0 && sy >= 0.0 && sx <= w - 1.0 && sy <= h - 1.0 {
                let x0 = sx.floor().min(w - 1.0) as u32;
                let y0 = sy.floor().min(h - 1.0) as u32;
                let fx = sx - x0 as f32;
                let fy = sy - y0 as f32;
                let x1 = (x0 + 1).min(w as u32 - 1);
                let y1 = (y0 + 1).min(h as u32 - 1);
                let p00 = src.get_pixel(x0, y0).0;
                let p10 = src.get_pixel(x1, y0).0;
                let p01 = src.get_pixel(x0, y1).0;
                let p11 = src.get_pixel(x1, y1).0;
                let mut px = [255u8; 4];
                for i in 0..4 {
                    let v = p00[i] as f32 * (1.0 - fx) * (1.0 - fy)
                        + p10[i] as f32 * fx * (1.0 - fy)
                        + p01[i] as f32 * (1.0 - fx) * fy
                        + p11[i] as f32 * fx * fy;
                    px[i] = v.round() as u8;
                }
                out.put_pixel(x, y, Rgba(px));
            }
        }
    }
    DynamicImage::ImageRgba8(out)
}

/// Rotate by an arbitrary angle in degrees (legacy name kept for callers).
pub fn rotate_image_angle(input: &[u8], degrees: f32) -> Result<Vec<u8>, ProteusError> {
    rotate_image(input, degrees)
}

/// Exposed for parity: normalize any angle to the representative quarter turn.
pub fn canonical_turns(quarter_turns: i32) -> u32 {
    quarter_turns.rem_euclid(4) as u32
}

#[cfg(test)]
mod tests {
    
    use super::*;
    use crate::image_ops::testutil;

    #[test]
    fn ninety_degree_rotation_swaps_dimensions() {
        let png = testutil::solid_png(30, 40, [8, 8, 8]);
        let out = rotate_image_90(&png, 1).unwrap();
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (40, 30));
    }

    #[test]
    fn four_turns_is_identity() {
        let png = testutil::solid_png(12, 9, [8, 8, 8]);
        let out = rotate_image_90(&png, 4).unwrap();
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (12, 9));
    }

    #[test]
    fn asymmetric_marker_moves_with_rotation() {
        // white 2x3 png, black only at (0,0) → rotated 90° ccw, black moves to
        // (0, height-1) (top-left corner of rotated = old bottom-left).
        let mut img = image::RgbImage::new(2, 3);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([250u8, 250, 250]);
        }
        img.put_pixel(0, 2, image::Rgb([0, 0, 0]));
        let png_bytes = crate::image_ops::encode(
            &image::DynamicImage::ImageRgb8(img),
            crate::image_ops::TargetFormat::Png,
            90,
        )
        .unwrap();
        let out = rotate_image_90(&png_bytes, 1).unwrap();
        let rotated = decode_image(&out).unwrap().to_rgb8();
        assert_eq!((rotated.width(), rotated.height()), (3, 2));
        assert!(rotated.get_pixel(0, 0).0 == [0, 0, 0], "marker must move to top-left");
    }

    #[test]
    fn arbitrary_angle_pads_and_keeps_content() {
        let png = testutil::solid_png(20, 20, [9, 9, 9]);
        let out = rotate_image_angle(&png, -45.0).unwrap();
        let img = decode_image(&out).unwrap();
        assert!(img.width() >= 20 && img.height() >= 20, "{}x{}", img.width(), img.height());
    }

    #[test]
    fn invalid_angles_rejected() {
        let png = testutil::solid_png(4, 4, [0, 0, 0]);
        assert!(matches!(
            rotate_image_angle(&png, f32::NAN).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
        assert!(matches!(
            rotate_image_angle(&png, 720.0).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            rotate_image_90(b"garbage", 1).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
        assert!(matches!(
            rotate_image_angle(b"garbage", 30.0).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }
}