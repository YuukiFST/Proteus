//! PDF merge (PRD §9) — T1 surface. Deep-copies every input document into one
//! output and re-roots all pages under a single Pages node.

use crate::error::ProteusError;
use crate::pdf::{append_document, finalize_pages, open_pdf, save_pdf};
use lopdf::Document;

/// Merge any number of PDF documents (bytes) into a single PDF.
pub fn merge_pdfs(inputs: &[&[u8]]) -> Result<Vec<u8>, ProteusError> {
    if inputs.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "merge_pdf",
            reason: "at least one input document is required".into(),
        });
    }
    let mut out = Document::with_version("1.7");
    let mut pages = Vec::new();
    for input in inputs {
        let src = open_pdf(input)?;
        pages.extend(append_document(&mut out, &src)?);
    }
    finalize_pages(&mut out, pages)?;
    save_pdf(&mut out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{
        extract_pdf_text, pdf_page_count, testutil, watermark::add_watermark,
    };

    #[test]
    fn merge_two_docs_sums_page_counts_and_preserves_order() {
        let a = testutil::marker_pdf(&["alpha", "beta"]);
        let b = testutil::marker_pdf(&["gamma"]);
        let merged = merge_pdfs(&[&a, &b]).unwrap();
        assert_eq!(pdf_page_count(&merged).unwrap(), 3);
        let text = extract_pdf_text(&merged).unwrap();
        let ai = text.find("alpha").unwrap();
        let bi = text.find("beta").unwrap();
        let gi = text.find("gamma").unwrap();
        assert!(ai < bi && bi < gi, "page order must be preserved: {text}");
    }

    #[test]
    fn merge_single_input_is_identity_in_page_count() {
        let a = testutil::three_page_pdf();
        let merged = merge_pdfs(&[&a]).unwrap();
        assert_eq!(pdf_page_count(&merged).unwrap(), 3);
        let text = extract_pdf_text(&merged).unwrap();
        assert!(text.contains("alpha") && text.contains("gamma"));
    }

    #[test]
    fn merge_empty_list_is_rejected() {
        let err = merge_pdfs(&[]).unwrap_err();
        assert!(matches!(err, ProteusError::InvalidArgument { .. }));
    }

    #[test]
    fn merge_of_three_docs_consecutively() {
        let a = testutil::one_page_pdf("first");
        let b = testutil::one_page_pdf("second");
        let c = testutil::one_page_pdf("third");
        let two = merge_pdfs(&[&a, &b]).unwrap();
        let three = merge_pdfs(&[&two, &c]).unwrap();
        assert_eq!(pdf_page_count(&three).unwrap(), 3);
        let text = extract_pdf_text(&three).unwrap();
        assert!(text.contains("first") && text.contains("second") && text.contains("third"));
    }

    #[test]
    fn merged_pages_remain_watermarkable() {
        // Merged docs must be structurally sound for downstream ops (a merged
        // page tree that lopdf itself cannot re-read is a broken merge).
        let a = testutil::one_page_pdf("alpha");
        let b = testutil::one_page_pdf("beta");
        let merged = merge_pdfs(&[&a, &b]).unwrap();
        let watermarked = add_watermark(&merged, &Default::default()).unwrap();
        let text = extract_pdf_text(&watermarked).unwrap();
        assert!(text.contains("alpha") && text.contains("beta"));
    }
}