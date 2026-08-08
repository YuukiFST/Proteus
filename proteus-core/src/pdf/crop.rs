//! PDF crop margins (PRD §9) — T1 surface.
//!
//! Sets /CropBox relative to the page's current visible box (CropBox or
//! MediaBox, inheritable per ISO 32000-1 §7.7.3.4). Values in points.

use crate::error::ProteusError;
use crate::pdf::{open_pdf, page_visible_box, save_pdf, set_page_box};
use lopdf::{Document, ObjectId};

/// Margin widths for each edge, in points.
#[derive(Debug, Clone, Copy, Default)]
pub struct CropMargins {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl CropMargins {
    pub fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        CropMargins {
            left,
            right,
            top,
            bottom,
        }
    }

    /// Margins must be finite, non-negative, and leave a non-empty box.
    fn validate(&self, w: f32, h: f32) -> Result<(), ProteusError> {
        for (name, m) in [
            ("left", self.left),
            ("right", self.right),
            ("top", self.top),
            ("bottom", self.bottom),
        ] {
            if !m.is_finite() || m < 0.0 {
                return Err(ProteusError::InvalidArgument {
                    surface: "crop_margins",
                    reason: format!("margin {name} must be a finite non-negative number, got {m}"),
                });
            }
        }
        if self.left + self.right >= w {
            return Err(ProteusError::InvalidArgument {
                surface: "crop_margins",
                reason: format!(
                    "left+right margins ({} + {}) must be strictly less than page width {w}",
                    self.left, self.right
                ),
            });
        }
        if self.top + self.bottom >= h {
            return Err(ProteusError::InvalidArgument {
                surface: "crop_margins",
                reason: format!(
                    "top+bottom margins ({} + {}) must be strictly less than page height {h}",
                    self.top, self.bottom
                ),
            });
        }
        Ok(())
    }
}

fn box_of(doc: &Document, page: ObjectId) -> Result<[f32; 4], ProteusError> {
    page_visible_box(doc, page)
}

/// Crop every page to its visible box inset by `margins`.
pub fn crop_margins(input: &[u8], margins: CropMargins) -> Result<Vec<u8>, ProteusError> {
    let mut doc = open_pdf(input)?;
    for page in crate::pdf::page_ids(&doc) {
        let b = box_of(&doc, page)?;
        margins.validate(b[2] - b[0], b[3] - b[1])?;
        let cropped = [
            b[0] + margins.left,
            b[1] + margins.bottom,
            b[2] - margins.right,
            b[3] - margins.top,
        ];
        set_page_box(&mut doc, page, b"CropBox", cropped)?;
    }
    save_pdf(&mut doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, page_box, testutil};

    fn crop_boxes(bytes: &[u8]) -> Vec<[f32; 4]> {
        let doc = open_pdf(bytes).unwrap();
        doc.page_iter()
            .map(|p| page_box(&doc, p, b"CropBox").expect("CropBox set"))
            .collect()
    }

    #[test]
    fn crop_sets_cropbox_to_inset_media_box() {
        let pdf = testutil::one_page_pdf("x"); // fixture MediaBox 612x792
        let out = crop_margins(&pdf, CropMargins::new(10.0, 20.0, 30.0, 40.0)).unwrap();
        assert_eq!(crop_boxes(&out)[0], [10.0, 40.0, 592.0, 762.0]);
    }

    #[test]
    fn sequential_equals_cumulative_crop() {
        let pdf = testutil::one_page_pdf("x");
        let a = CropMargins::new(10.0, 0.0, 0.0, 0.0);
        let step1 = crop_margins(&pdf, a).unwrap();
        let step2 = crop_margins(&step1, a).unwrap();
        let direct = crop_margins(&pdf, CropMargins::new(20.0, 0.0, 0.0, 0.0)).unwrap();
        assert_eq!(crop_boxes(&step2), crop_boxes(&direct));
    }

    #[test]
    fn out_of_bounds_margins_are_rejected() {
        let pdf = testutil::one_page_pdf("x");
        for bad in [
            CropMargins::new(-1.0, 0.0, 0.0, 0.0),
            CropMargins::new(400.0, 400.0, 0.0, 0.0),
            CropMargins::new(0.0, 0.0, 500.0, 500.0),
        ] {
            let err = crop_margins(&pdf, bad).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }));
        }
    }

    #[test]
    fn content_survives_cropping() {
        let pdf = testutil::one_page_pdf("hal me");
        let out = crop_margins(&pdf, CropMargins::new(10.0, 10.0, 10.0, 10.0)).unwrap();
        assert!(extract_pdf_text(&out).unwrap().contains("hal me"));
    }
}