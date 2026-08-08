//! Image convert (PRD §9) — T1. Format conversion between JPEG/PNG/WebP/AVIF.

use crate::error::ProteusError;
use crate::image_ops::{decode_image, encode};

/// Canonical target formats (matches PRD §9's list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Avif,
}

impl From<ImageFormat> for image::ImageFormat {
    fn from(f: ImageFormat) -> Self {
        match f {
            ImageFormat::Jpeg => image::ImageFormat::Jpeg,
            ImageFormat::Png => image::ImageFormat::Png,
            ImageFormat::WebP => image::ImageFormat::WebP,
            ImageFormat::Avif => image::ImageFormat::Avif,
        }
    }
}

impl ImageFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Png => "png",
            ImageFormat::WebP => "webp",
            ImageFormat::Avif => "avif",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ProteusError> {
        match s.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Ok(ImageFormat::Jpeg),
            "png" => Ok(ImageFormat::Png),
            "webp" => Ok(ImageFormat::WebP),
            "avif" => Ok(ImageFormat::Avif),
            other => Err(ProteusError::InvalidArgument {
                surface: "convert_image",
                reason: format!("unsupported target format '{other}'"),
            }),
        }
    }
}

/// Convert an image between formats.
pub fn convert_image(input: &[u8], target: ImageFormat) -> Result<Vec<u8>, ProteusError> {
    let img = decode_image(input)?;
    encode(&img, target, 85)
}

#[cfg(test)]
mod tests {
    
    use super::*;
    use crate::image_ops::testutil;
    use image::ImageFormat as ImgFmt;

    fn png_of(w: u32, h: u32, colour: [u8; 3]) -> Vec<u8> {
        testutil::solid_png(w, h, colour)
    }

    #[test]
    fn png_to_jpeg_converts_and_decodes() {
        let out = convert_image(&png_of(8, 8, [255, 0, 0]), ImageFormat::Jpeg).unwrap();
        assert_eq!(image::guess_format(&out).unwrap(), ImgFmt::Jpeg);
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (8, 8));
    }

    #[test]
    fn jpeg_to_png_preserves_dimensions_and_alpha_source() {
        let jpeg: Vec<u8> = {
            let mut c = std::io::Cursor::new(Vec::new());
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut c, 80)
                .encode_image(&image::RgbImage::from_pixel(16, 16, image::Rgb([50, 100, 150])))
                .unwrap();
            c.into_inner()
        };
        let out = convert_image(&jpeg, ImageFormat::Png).unwrap();
        assert_eq!(image::guess_format(&out).unwrap(), ImgFmt::Png);
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width(), img.height()), (16, 16));
    }

    #[test]
    fn convert_to_webp_and_avif() {
        let png = png_of(6, 6, [9, 9, 9]);
        for target in [ImageFormat::WebP, ImageFormat::Avif] {
            let out = convert_image(&png, target).unwrap();
            let expected = if target == ImageFormat::WebP { ImgFmt::WebP } else { ImgFmt::Avif };
            assert_eq!(image::guess_format(&out).unwrap(), expected);
        }
    }

    #[test]
    fn format_parsing_is_case_insensitive_and_rejects_unknown() {
        assert_eq!(ImageFormat::parse("JPEG").unwrap(), ImageFormat::Jpeg);
        assert_eq!(ImageFormat::parse("jpg").unwrap(), ImageFormat::Jpeg);
        assert_eq!(ImageFormat::parse("PNG").unwrap(), ImageFormat::Png);
        assert!(ImageFormat::parse("gif").is_err());
        assert!(ImageFormat::parse("").is_err());
    }

    #[test]
    fn garbage_input_is_malformed() {
        assert!(matches!(
            convert_image(b"nope", ImageFormat::Png).unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }

}