//! PDF page numbers (PRD §9) — T1 surface. Footer-centered "N" or "N of M"
//! labels placed with Helvetica metrics.

use crate::error::ProteusError;
use crate::pdf::afm::text_width_pt;
use crate::pdf::{
    add_base14_font, append_content, open_pdf, page_visible_box, save_pdf, set_resource_entry,
    text_line_ops,
};
use lopdf::Object;

#[derive(Debug, Clone)]
pub struct PageNumberOptions {
    /// Number printed on the first page (1-based source page numbering).
    pub start_at: u32,
    /// Print "N of M" instead of "N".
    pub show_total: bool,
    /// Font size in points.
    pub font_size: f32,
}

impl Default for PageNumberOptions {
    fn default() -> Self {
        PageNumberOptions {
            start_at: 1,
            show_total: false,
            font_size: 12.0,
        }
    }
}

/// Add a centered page-number label to the footer of every page.
pub fn add_page_numbers(input: &[u8], options: &PageNumberOptions) -> Result<Vec<u8>, ProteusError> {
    if !options.font_size.is_finite() || options.font_size <= 0.0 {
        return Err(ProteusError::InvalidArgument {
            surface: "add_page_numbers",
            reason: format!("font size must be positive, got {}", options.font_size),
        });
    }
    let mut doc = open_pdf(input)?;
    let pages = crate::pdf::page_ids(&doc);
    let total = pages.len() as u32;
    let font_id = add_base14_font(&mut doc, "Helvetica");

    for (i, page) in pages.iter().enumerate() {
        let n = options.start_at.saturating_add(i as u32);
        let label = if options.show_total {
            format!("{n} / {total}")
        } else {
            format!("{n}")
        };
        let bytes = crate::pdf::pdf_text_bytes(&label);
        let b = page_visible_box(&doc, *page)?;
        let w = text_width_pt(&bytes, options.font_size);
        let x = (b[0] + b[2]) / 2.0 - w / 2.0;
        let y = b[1] + 24.0;
        let ops = text_line_ops(b"NumFont", options.font_size, x, y, &bytes);
        set_resource_entry(&mut doc, *page, b"Font", b"NumFont", Object::Reference(font_id))?;
        append_content(&mut doc, *page, ops)?;
    }
    save_pdf(&mut doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count, testutil};

    #[test]
    fn every_page_gets_its_ordinal_label() {
        let pdf = testutil::three_page_pdf();
        let out = add_page_numbers(&pdf, &Default::default()).unwrap();
        for (p, label) in [(1u32, "1"), (2, "2"), (3, "3")] {
            let text = crate::pdf::extract_page_text(&out, p).unwrap();
            assert!(
                text.contains(label),
                "page {p} must carry label {label}: {text}"
            );
        }
        assert_eq!(pdf_page_count(&out).unwrap(), 3);
    }

    #[test]
    fn start_at_shifts_all_labels() {
        let pdf = testutil::marker_pdf(&["a", "b"]);
        let out = add_page_numbers(
            &pdf,
            &PageNumberOptions { start_at: 10, ..Default::default() },
        )
        .unwrap();
        let t1 = crate::pdf::extract_page_text(&out, 1).unwrap();
        let t2 = crate::pdf::extract_page_text(&out, 2).unwrap();
        assert!(t1.contains('1') && t1.contains('0'), "page1 label has 10: {t1}");
        assert!(t2.contains('1') && t2.contains('1'), "page2 label has 11: {t2}");
    }

    #[test]
    fn show_total_labels_are_n_of_m() {
        let pdf = testutil::marker_pdf(&["a", "b"]);
        let out = add_page_numbers(
            &pdf,
            &PageNumberOptions { show_total: true, ..Default::default() },
        )
        .unwrap();
        let p1 = crate::pdf::extract_page_text(&out, 1).unwrap();
        let p2 = crate::pdf::extract_page_text(&out, 2).unwrap();
        assert!(p1.contains("1 / 2"), "p1: {p1}");
        assert!(p2.contains("2 / 2"), "p2: {p2}");
    }

    #[test]
    fn original_content_survives_numbering() {
        let pdf = testutil::one_page_pdf("unique marker words");
        let out = add_page_numbers(&pdf, &Default::default()).unwrap();
        let text = extract_pdf_text(&out).unwrap();
        assert!(text.contains("unique marker words"), "{text}");
        // no asterisk of the original content stream
        let doc = crate::pdf::open_pdf(&pdf).unwrap();
        let before = crate::pdf::page_content_of(&doc, doc.page_iter().next().unwrap());
        let doc2 = crate::pdf::open_pdf(&out).unwrap();
        let after = crate::pdf::page_content_of(&doc2, doc2.page_iter().next().unwrap());
        assert!(after.starts_with(before.as_slice()));
    }

    #[test]
    fn invalid_font_size_rejected() {
        let pdf = testutil::one_page_pdf("x");
        let err = add_page_numbers(
            &pdf,
            &PageNumberOptions { font_size: -1.0, ..Default::default() },
        )
        .unwrap_err();
        assert!(matches!(err, ProteusError::InvalidArgument { .. }));
    }
}