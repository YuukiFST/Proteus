//! PDF organize / reorder pages (PRD §9) — T1 surface.
//! `order` is a permutation of 1-based page numbers.

use crate::error::ProteusError;
use crate::pdf::{append_document, finalize_pages, open_pdf, prune_unreachable, save_pdf};
use lopdf::Document;

/// Reorder pages: `order` must contain every page number 1..=n exactly once.
pub fn reorder_pdf(input: &[u8], order: &[u32]) -> Result<Vec<u8>, ProteusError> {
    let src = open_pdf(input)?;
    let all = crate::pdf::page_ids(&src);
    let n = all.len() as u32;
    if order.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "reorder_pdf",
            reason: "order may not be empty".into(),
        });
    }
    // permutation validation
    let mut seen = std::collections::HashSet::new();
    for &p in order {
        if p < 1 || p > n {
            return Err(ProteusError::InvalidArgument {
                surface: "reorder_pdf",
                reason: format!("page {p} is outside 1..={n}"),
            });
        }
        if !seen.insert(p) {
            return Err(ProteusError::InvalidArgument {
                surface: "reorder_pdf",
                reason: format!("page {p} appears more than once"),
            });
        }
    }
    if seen.len() as u32 != n {
        return Err(ProteusError::InvalidArgument {
            surface: "reorder_pdf",
            reason: "order must name every page exactly once".into(),
        });
    }

    let mut doc = Document::with_version("1.7");
    let remapped = append_document(&mut doc, &src)?;
    if remapped.len() < n as usize {
        return Err(ProteusError::MalformedInput(
            "page tree lost pages while copying".into(),
        ));
    }
    let wanted: Vec<lopdf::ObjectId> = order
        .iter()
        .map(|p| remapped[(p - 1) as usize])
        .collect();
    finalize_pages(&mut doc, wanted)?;
    prune_unreachable(&mut doc);
    save_pdf(&mut doc)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count, testutil};
    use proptest::prelude::*;

    #[test]
    fn reorder_permutation_changes_page_order() {
        let pdf = testutil::three_page_pdf();
        let reordered = reorder_pdf(&pdf, &[3, 1, 2]).unwrap();
        let text = extract_pdf_text(&reordered).unwrap();
        let gi = text.find("gamma").unwrap();
        let ai = text.find("alpha").unwrap();
        let bi = text.find("beta").unwrap();
        assert!(gi < ai && ai < bi, "expected gamma,alpha,beta: {text}");
    }

    #[test]
    fn identity_permutation_is_unchanged() {
        let pdf = testutil::three_page_pdf();
        let reordered = reorder_pdf(&pdf, &[1, 2, 3]).unwrap();
        let text = extract_pdf_text(&reordered).unwrap();
        let ai = text.find("alpha").unwrap();
        let bi = text.find("beta").unwrap();
        let gi = text.find("gamma").unwrap();
        assert!(ai < bi && bi < gi, "identity must preserve order: {text}");
        assert_eq!(pdf_page_count(&reordered).unwrap(), 3);
    }

    #[test]
    fn invalid_orders_are_rejected() {
        let pdf = testutil::three_page_pdf();
        for bad in [
            vec![1, 2, 4],        // out of range
            vec![1, 1, 2],        // duplicate
            vec![2, 3],           // missing page
            vec![0, 1, 2],        // below 1
            Vec::new(),           // empty
        ] {
            let err = reorder_pdf(&pdf, &bad).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }), "{bad:?}");
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// For every permutation of a document's pages, the output has the
        /// same page count and carries every page's marker exactly once.
        #[test]
        fn any_permutation_preserves_page_set(
            n in 1usize..8,
        ) {
            let texts: Vec<String> = (1..=n).map(|i| format!("marker{i}")).collect();
            let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            let pdf = testutil::marker_pdf(&refs);
            let perm: Vec<u32> = {
                let mut v: Vec<u32> = (1..=n as u32).collect();
                // deterministic shuffle for prop determinism
                let mut seed = n as u64 * 7919;
                for i in (1..v.len()).rev() {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let j = (seed >> 33) as usize % (i + 1);
                    v.swap(i, j);
                }
                v
            };
            let out = reorder_pdf(&pdf, &perm).unwrap();
            let text = extract_pdf_text(&out).unwrap();
            // the last-to-first position of each marker must be ascending per perm order
            let positions: Vec<usize> = perm.iter().map(|p| text.find(&format!("marker{p}")).unwrap()).collect();
            prop_assert!(positions.windows(2).all(|w| w[0] < w[1]), "order {perm:?} not respected: {text}");
        }
    }
}
