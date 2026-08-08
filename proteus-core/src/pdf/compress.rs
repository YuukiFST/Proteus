//! PDF compress (PRD §9) — T1 surface.
//!
//! Lossless: re-serializes the document with stream compression and drops
//! dead objects; if the pipeline can't produce something smaller, the
//! original bytes are returned unchanged (the size oracle stays honest).

use crate::error::ProteusError;
use crate::pdf::{open_pdf, prune_unreachable, save_pdf};

pub fn compress_pdf(input: &[u8]) -> Result<Vec<u8>, ProteusError> {
    let mut doc = open_pdf(input)?;
    doc.compress();
    prune_unreachable(&mut doc);
    let compressed = save_pdf(&mut doc)?;
    // Never emit output larger than the input: best effort, size-ordered.
    if compressed.len() < input.len() {
        Ok(compressed)
    } else {
        Ok(input.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count, testutil};
    use lopdf::{dictionary, Document, Object, Stream};

    /// Build a genuinely wasteful document: many duplicated, uncompressed
    /// streams from hand-written raw PDF bytes.
    fn bulky_pdf() -> Vec<u8> {
        // ten pages, each with a big repeated content stream
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        // ten pages each with a big, compressible, uncompressed content stream
        for _ in 0..10 {
            let content: Vec<u8> = vec![b'x'; 40_000];
            let content_id = doc.add_object(Stream::new(dictionary! {}, content));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            });
            kids.push(Object::Reference(page_id));
        }
        doc.set_object(
            pages_id,
            dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => 10_i64,
            },
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set(b"Root", root);
        crate::pdf::save_pdf(&mut doc).unwrap()
    }

    #[test]
    fn compress_output_is_valid_and_smaller_or_equal() {
        let input = bulky_pdf();
        let out = compress_pdf(&input).unwrap();
        assert!(
            out.len() <= input.len(),
            "compressed {} vs input {}",
            out.len(),
            input.len()
        );
        // Must still be a fully readable, same-shape document.
        assert_eq!(pdf_page_count(&out).unwrap(), 10);
    }

    #[test]
    fn compress_small_document_never_grows() {
        // A tiny fixture that is already optimal: output must be ≤ input.
        let input = testutil::one_page_pdf("tiny");
        let out = compress_pdf(&input).unwrap();
        assert!(
            out.len() <= input.len(),
            "must not grow: {} > {}",
            out.len(),
            input.len()
        );
        let text = extract_pdf_text(&out).unwrap();
        assert!(text.contains("tiny"), "content lost: {text}");
    }

    #[test]
    fn compress_preserves_text_and_structure() {
        let input = testutil::three_page_pdf();
        let out = compress_pdf(&input).unwrap();
        assert_eq!(pdf_page_count(&out).unwrap(), 3);
        let text = extract_pdf_text(&out).unwrap();
        for marker in ["alpha", "beta", "gamma"] {
            assert!(text.contains(marker), "{marker} lost: {text}");
        }
    }

    #[test]
    fn compress_rejects_malformed_input() {
        assert!(matches!(
            compress_pdf(b"junk").unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }
}