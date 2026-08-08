//! PDF split (PRD §9) — T1 surface: parameter-based page-range extraction.

use std::collections::HashMap;

use crate::error::ProteusError;
use crate::pdf::{append_document, finalize_pages, open_pdf, page_ids, prune_unreachable, save_pdf};
use lopdf::{Document, ObjectId};

/// An inclusive 1-based page range `start..=end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    pub start: u32,
    pub end: u32,
}

impl PageRange {
    pub fn new(start: u32, end: u32) -> Self {
        PageRange { start, end }
    }

    /// Inclusive 1-based validation against a document's page count.
    pub fn validate(&self, page_count: u32) -> Result<(), ProteusError> {
        if self.start < 1 {
            return Err(ProteusError::InvalidArgument {
                surface: "split_pdf",
                reason: format!("range start {} is below page 1", self.start),
            });
        }
        if self.end < self.start {
            return Err(ProteusError::InvalidArgument {
                surface: "split_pdf",
                reason: format!("range end {} is below start {}", self.end, self.start),
            });
        }
        if self.end > page_count {
            return Err(ProteusError::InvalidArgument {
                surface: "split_pdf",
                reason: format!(
                    "range end {} is beyond the document's {} pages",
                    self.end, page_count
                ),
            });
        }
        Ok(())
    }
}

/// Split a PDF into output documents, one per range, preserving in-document
/// page order. Ranges may overlap; each output starts from the source pages.
pub fn split_pdf(input: &[u8], ranges: &[PageRange]) -> Result<Vec<Vec<u8>>, ProteusError> {
    if ranges.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "split_pdf",
            reason: "at least one page range is required".into(),
        });
    }
    let src = open_pdf(input)?;
    let all = page_ids(&src);
    let page_count = all.len() as u32;
    for range in ranges {
        range.validate(page_count)?;
    }

    let mut outputs = Vec::with_capacity(ranges.len());
    for range in ranges {
        let mut doc = Document::with_version("1.7");
        // Copy the whole catalog closure (pages + shared objects).
        let remapped: Vec<ObjectId> = append_document(&mut doc, &src)?;
        if remapped.len() < page_count as usize {
            return Err(ProteusError::MalformedInput(
                "page closure lost pages while copying".into(),
            ));
        }
        // remapped[i] corresponds to all[i].
        let map: HashMap<lopdf::ObjectId, lopdf::ObjectId> = all
            .iter()
            .zip(remapped)
            .map(|(a, b)| (*a, b))
            .collect();
        let wanted: Vec<lopdf::ObjectId> = (range.start..=range.end)
            .map(|i| map[&all[(i - 1) as usize]])
            .collect();
        finalize_pages(&mut doc, wanted)?;
        prune_unreachable(&mut doc);
        outputs.push(save_pdf(&mut doc)?);
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, merge_pdfs, pdf_page_count, testutil};
    use proptest::prelude::*;

    #[test]
    fn split_every_page_isolates_markers() {
        let pdf = testutil::three_page_pdf();
        let parts =
            split_pdf(&pdf, &[PageRange::new(1, 1), PageRange::new(2, 2), PageRange::new(3, 3)])
                .unwrap();
        assert_eq!(parts.len(), 3);
        for (part, marker) in parts.iter().zip(["alpha", "beta", "gamma"]) {
            let text = extract_pdf_text(part).unwrap();
            assert!(text.contains(marker), "page must carry {marker}: {text}");
            assert_eq!(pdf_page_count(part).unwrap(), 1);
        }
    }

    #[test]
    fn split_range_extracts_subsequence_only() {
        let pdf = testutil::marker_pdf(&["alpha", "beta", "gamma", "delta"]);
        let parts = split_pdf(&pdf, &[PageRange::new(2, 3)]).unwrap();
        assert_eq!(parts.len(), 1);
        let text = extract_pdf_text(&parts[0]).unwrap();
        assert!(text.contains("beta") && text.contains("gamma"));
        assert!(!text.contains("alpha"), "leak: {text}");
        assert!(!text.contains("delta"), "leak: {text}");
    }

    #[test]
    fn split_range_validates_bounds() {
        let pdf = testutil::three_page_pdf();
        for bad in [PageRange::new(0, 1), PageRange::new(2, 1), PageRange::new(1, 4)] {
            let err = split_pdf(&pdf, &[bad]).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }), "{bad:?}");
        }
        assert!(matches!(
            split_pdf(&pdf, &[]).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// Round-trip: split everything and merge it back reproduces the doc.
        #[test]
        fn split_all_pages_roundtrip_to_original(
            page_count in 1usize..7,
        ) {
            let texts: Vec<String> = (1..=page_count).map(|i| format!("page{i}")).collect();
            let refs_str: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            let pdf = testutil::marker_pdf(&refs_str);
            let ranges: Vec<PageRange> =
                (1..=page_count as u32).map(|i| PageRange::new(i, i)).collect();
            let parts = split_pdf(&pdf, &ranges).unwrap();
            prop_assert_eq!(parts.len(), page_count);
            let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
            let merged = merge_pdfs(&refs).unwrap();
            let count = pdf_page_count(&merged).unwrap();
            prop_assert_eq!(count as usize, page_count);
            // Text markers survive the round trip.
            let text = extract_pdf_text(&merged).unwrap();
            for i in 0..page_count {
                prop_assert!(text.contains(&format!("page{}", i + 1)), "marker lost: {text}");
            }
        }
    }
}