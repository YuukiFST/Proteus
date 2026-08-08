//! PDF watermark (PRD §9) — T1 surface. Text overlay placed with authentic
//! Helvetica metrics (AFM), positioned in the page's visible box.

use crate::error::ProteusError;
use crate::pdf::afm::text_width_pt;
use crate::pdf::{
    add_base14_font, append_content, open_pdf, page_visible_box, save_pdf, set_resource_entry,
    text_line_ops,
};
use lopdf::{dictionary, Document, Object, ObjectId};

/// Anchor placement for the watermark within each page's visible box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkPosition {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct WatermarkOptions {
    pub text: String,
    pub font_size: f32,
    /// 0.0 (invisible) ..= 1.0 (opaque).
    pub opacity: f32,
    pub position: WatermarkPosition,
    /// Multiple of 90 (0 default). Rotation uses a text-matrix composition.
    pub rotation_degrees: i16,
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        WatermarkOptions {
            text: "CONFIDENTIAL".to_string(),
            font_size: 48.0,
            opacity: 0.25,
            position: WatermarkPosition::Center,
            rotation_degrees: 0,
        }
    }
}

impl WatermarkOptions {
    fn validate(&self) -> Result<(), ProteusError> {
        if self.text.trim().is_empty() {
            return Err(ProteusError::InvalidArgument {
                surface: "add_watermark",
                reason: "watermark text may not be empty".into(),
            });
        }
        if !self.font_size.is_finite() || self.font_size <= 0.0 {
            return Err(ProteusError::InvalidArgument {
                surface: "add_watermark",
                reason: format!("font size must be positive, got {}", self.font_size),
            });
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(ProteusError::InvalidArgument {
                surface: "add_watermark",
                reason: format!("opacity must be within 0..=1, got {}", self.opacity),
            });
        }
        if self.rotation_degrees % 90 != 0 {
            return Err(ProteusError::InvalidArgument {
                surface: "add_watermark",
                reason: format!("rotation {}° is not a multiple of 90", self.rotation_degrees),
            });
        }
        Ok(())
    }
}

/// Overlay the watermark text on every page of the document.
pub fn add_watermark(input: &[u8], options: &WatermarkOptions) -> Result<Vec<u8>, ProteusError> {
    options.validate()?;
    let mut doc = open_pdf(input)?;
    let font_id = add_base14_font(&mut doc, "Helvetica");
    let gs_id = add_ext_gstate(&mut doc, options.opacity)?;
    let bytes = crate::pdf::pdf_text_bytes(&options.text);
    let size = options.font_size;

    for page in crate::pdf::page_ids(&doc) {
        let b = page_visible_box(&doc, page)?;
        let (x, y) = anchor_point(options.position, b, &bytes, size);
        let mut ops = Vec::new();
        ops.extend_from_slice(b"q\n/GS1 gs\n0.5 0.5 0.5 rg\n");
        if options.rotation_degrees.rem_euclid(360) == 0 {
            ops.extend_from_slice(&text_line_ops(b"WFont", size, x, y, &bytes));
        } else {
            let rad = (options.rotation_degrees as f32).to_radians();
            let cos = rad.cos();
            let sin = rad.sin();
            let neg_sin = -sin;
            ops.extend_from_slice(b"BT\n");
            ops.extend_from_slice(format!("/WFont {size} Tf\n").as_bytes());
            ops.extend_from_slice(format!("{cos} {sin} {neg_sin} {cos} {x} {y} Tm\n(").as_bytes());
            ops.extend_from_slice(&crate::pdf::escape_pdf_string(&bytes));
            ops.extend_from_slice(b") Tj\nET\n");
        }
        ops.extend_from_slice(b"Q\n");
        set_resource_entry(&mut doc, page, b"Font", b"WFont", Object::Reference(font_id))?;
        set_resource_entry(&mut doc, page, b"ExtGState", b"GS1", Object::Reference(gs_id))?;
        append_content(&mut doc, page, ops)?;
    }
    save_pdf(&mut doc)
}

fn add_ext_gstate(doc: &mut Document, opacity: f32) -> Result<ObjectId, ProteusError> {
    Ok(doc.add_object(dictionary! {
        "Type" => "ExtGState",
        "ca" => opacity,
        "CA" => opacity,
    }))
}

fn anchor_point(
    position: WatermarkPosition,
    b: [f32; 4],
    bytes: &[u8],
    size: f32,
) -> (f32, f32) {
    let w = text_width_pt(bytes, size);
    let cx = (b[0] + b[2]) / 2.0;
    let cy = (b[1] + b[3]) / 2.0;
    match position {
        WatermarkPosition::Center => (cx - w / 2.0, cy - size / 2.0),
        WatermarkPosition::TopLeft => (b[0] + 24.0, b[3] - 24.0 - size),
        WatermarkPosition::TopRight => (b[2] - 24.0 - w, b[3] - 24.0 - size),
        WatermarkPosition::BottomLeft => (b[0] + 24.0, b[1] + 24.0),
        WatermarkPosition::BottomRight => (b[2] - 24.0 - w, b[1] + 24.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count, testutil};

    #[test]
    fn watermark_text_lands_in_text_layer() {
        let pdf = testutil::one_page_pdf("content here");
        let opts = WatermarkOptions {
            text: "SECRET MARK".into(),
            opacity: 0.5,
            font_size: 30.0,
            ..Default::default()
        };
        let out = add_watermark(&pdf, &opts).unwrap();
        let text = extract_pdf_text(&out).unwrap();
        assert!(text.contains("SECRET MARK"), "watermark missing: {text}");
        assert!(text.contains("content here"), "original content lost: {text}");
        assert_eq!(pdf_page_count(&out).unwrap(), 1);
    }

    #[test]
    fn watermark_preserves_original_content_stream() {
        let pdf = testutil::one_page_pdf("plain");
        let doc = open_pdf(&pdf).unwrap();
        let page = doc.page_iter().next().unwrap();
        let before = crate::pdf::page_content_of(&doc, page);
        let out = add_watermark(&pdf, &Default::default()).unwrap();
        let doc2 = open_pdf(&out).unwrap();
        let page2 = doc2.page_iter().next().unwrap();
        let after = crate::pdf::page_content_of(&doc2, page2);
        assert!(
            after.starts_with(before.as_slice()),
            "original stream must survive verbatim"
        );
    }

    #[test]
    fn every_page_gets_the_watermark() {
        let pdf = testutil::three_page_pdf();
        let opts = WatermarkOptions {
            text: "X".into(),
            ..Default::default()
        };
        let out = add_watermark(&pdf, &opts).unwrap();
        for p in 1..=3 {
            let text = crate::pdf::extract_page_text(&out, p).unwrap();
            assert!(text.contains('X'), "watermark missing on page {p}");
        }
    }

    #[test]
    fn invalid_options_are_rejected_before_work() {
        let pdf = testutil::one_page_pdf("x");
        let cases = [
            WatermarkOptions { text: "".into(), ..Default::default() },
            WatermarkOptions { font_size: 0.0, ..Default::default() },
            WatermarkOptions { opacity: 1.5, ..Default::default() },
            WatermarkOptions { rotation_degrees: 45, ..Default::default() },
        ];
        for c in cases {
            let err = add_watermark(&pdf, &c).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }));
        }
    }

    #[test]
    fn centered_watermark_uses_afm_metrics() {
        let pdf = testutil::one_page_pdf("x");
        let out = add_watermark(
            &pdf,
            &WatermarkOptions {
                text: "ABA".into(),
                font_size: 20.0,
                ..Default::default()
            },
        )
        .unwrap();
        let doc = open_pdf(&out).unwrap();
        let content = crate::pdf::page_content_of(&doc, doc.page_iter().next().unwrap());
        let text = String::from_utf8_lossy(&content);
        // Grab the Tm x-coordinate from the injected block (Tf and Tm are on
        // separate lines in text_line_ops).
        let tm_line = text.lines().find(|l| l.contains(" Tm")).unwrap_or("");
        let x: f32 = tm_line.split_whitespace().nth(4).unwrap_or("0").parse().unwrap_or(0.0);
        let w = text_width_pt(b"ABA", 20.0);
        let expected = (612.0 - w) / 2.0; // fixture MediaBox width 612
        assert!((x - expected).abs() < 0.6, "x={x}, expected {expected}");
    }
}