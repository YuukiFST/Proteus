//! PDF rotate (PRD §9) — T1 surface. Sets page /Rotate (ISO 32000-1 §7.9.1)
//! so viewers rotate; content bytes are untouched.

use crate::error::ProteusError;
use crate::pdf::{open_pdf, save_pdf};
use lopdf::Object;

/// Rotate every page by `degrees` (mutually any multiple of 90; negative allowed,
/// normalized into [0, 360)).
pub fn rotate_pdf(input: &[u8], degrees: i32) -> Result<Vec<u8>, ProteusError> {
    if degrees % 90 != 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "rotate_pdf",
            reason: format!("rotation {degrees}° is not a multiple of 90"),
        });
    }
    let mut doc = open_pdf(input)?;
    let delta = (degrees.rem_euclid(360)) as i64;
    for page_id in crate::pdf::page_ids(&doc) {
        let dict = doc
            .get_dictionary_mut(page_id)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        let current = match dict.get(b"Rotate") {
            Ok(Object::Integer(n)) => *n,
            _ => 0,
        };
        dict.set(b"Rotate", (current + delta).rem_euclid(360));
    }
    save_pdf(&mut doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count, testutil};
    use lopdf::Object;

    fn read_rotate(part: &[u8]) -> Vec<i64> {
        let doc = crate::pdf::open_pdf(part).unwrap();
        doc.page_iter()
            .map(|id| {
                let dict = doc.get_dictionary(id).unwrap();
                match dict.get(b"Rotate") {
                    Ok(Object::Integer(n)) => *n,
                    _ => 0,
                }
            })
            .collect()
    }

    #[test]
    fn rotate_90_four_times_is_identity() {
        let pdf = testutil::one_page_pdf("halcyon");
        let r1 = rotate_pdf(&pdf, 90).unwrap();
        let r2 = rotate_pdf(&r1, 90).unwrap();
        let r3 = rotate_pdf(&r2, 90).unwrap();
        let r4 = rotate_pdf(&r3, 90).unwrap();
        assert_eq!(read_rotate(&r4), vec![0], "4×90° must be identity");
        assert_eq!(pdf_page_count(&r4).unwrap(), 1);
        // Content survives the rotation circuit.
        let text = extract_pdf_text(&r4).unwrap();
        assert!(text.contains("halcyon"), "text lost: {text}");
    }

    #[test]
    fn rotate_270_equals_minus_90() {
        let pdf = testutil::one_page_pdf("x");
        let a = rotate_pdf(&pdf, 270).unwrap();
        let b = rotate_pdf(&pdf, -90).unwrap();
        assert_eq!(read_rotate(&a), read_rotate(&b));
    }

    #[test]
    fn non_multiple_of_90_is_rejected() {
        let pdf = testutil::one_page_pdf("x");
        for bad in [45, 91, -45, 100] {
            let err = rotate_pdf(&pdf, bad).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }), "{bad}");
        }
    }

    #[test]
    fn rotate_applies_to_every_page() {
        let pdf = testutil::three_page_pdf();
        let out = rotate_pdf(&pdf, 180).unwrap();
        assert_eq!(read_rotate(&out), vec![180, 180, 180]);
    }
}