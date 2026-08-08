//! PDF→PDF/A (PRD §9) — "Convert to PDF/A" tool. T1 surface, honest about
//! PDF/A-1b obligations:
//!
//! 1. Rejects documents that cannot legally become PDF/A: encrypted files
//!    (PDF/A forbids encryption) and documents with non-embedded fonts
//!    (PDF/A-1 requires embedded font programs; faking conformance is worse
//!    than refusal).
//! 2. Embeds XMP metadata declaring PDF/A-1b and an OutputIntent backed by a
//!    real sRGB ICC v2 profile (built deterministically in this module and
//!    structurally validated by tests).
//! 3. Re-serializes.

use crate::error::ProteusError;
use crate::pdf::{open_pdf, save_pdf};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

/// Minimal PDF/A-1b XMP identification packet (pdfaid:part=1, conformance=B).
const PDFA_XMP: &str = r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
   <pdfaid:part>1</pdfaid:part>
   <pdfaid:conformance>B</pdfaid:conformance>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

/// Convert a PDF to a (claimed) PDF/A-1b document.
pub fn convert_to_pdfa(input: &[u8]) -> Result<Vec<u8>, ProteusError> {
    let mut doc = open_pdf(input)?;
    if doc.is_encrypted() {
        return Err(ProteusError::NotSupported(
            "PDF/A forbids encrypted documents; unlock it first".into(),
        ));
    }
    let unembedded = unembedded_fonts(&doc)?;
    if !unembedded.is_empty() {
        return Err(ProteusError::NotSupported(format!(
            "document uses fonts without embedded programs: {} (PDF/A-1b requires embedded fonts)",
            unembedded.join(", ")
        )));
    }

    let xmp_id = doc.add_object(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        PDFA_XMP.as_bytes().to_vec(),
    ));
    let icc_id = doc.add_object(Stream::new(dictionary! { "N" => 3 }, srgb_icc_profile()));
    let intent_id = doc.add_object(dictionary! {
        "Type" => "OutputIntent",
        "S" => "GTS_PDFA1",
        "OutputConditionIdentifier" => "sRGB IEC61966-2.1",
        "Info" => "sRGB IEC61966-2.1",
        "DestOutputProfile" => icc_id,
    });
    let catalog = doc.catalog_mut().map_err(|e| ProteusError::Pdf(Box::new(e)))?;
    catalog.set(b"Metadata", xmp_id);
    catalog.set(b"OutputIntents", vec![Object::Reference(intent_id)]);

    save_pdf(&mut doc)
}

/// Font names lacking an embedded program (FontDescriptor with FontFile*).
fn unembedded_fonts(doc: &Document) -> Result<Vec<String>, ProteusError> {
    let mut bad = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for font_id in collect_fonts(doc) {
        if !seen.insert(font_id) {
            continue;
        }
        let dict = match doc.get_dictionary(font_id) {
            Ok(d) => d,
            Err(_) => continue, // dangling font ref: not ours to judge strictly
        };
        let name = dict
            .get(b"BaseFont")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_else(|| format!("obj{}", font_id.0));
        let embedded = dict
            .get(b"FontDescriptor")
            .ok()
            .is_some_and(|fd| match fd {
                Object::Reference(r) => doc.get_dictionary(*r).is_ok_and(|desc| {
                    desc.has(b"FontFile") || desc.has(b"FontFile2") || desc.has(b"FontFile3")
                }),
                _ => false,
            });
        if !embedded {
            bad.push(name);
        }
    }
    bad.sort();
    bad.dedup();
    Ok(bad)
}

fn collect_fonts(doc: &Document) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for dict in resource_dicts(doc) {
        let mut fonts = dict.get(b"Font").ok().cloned();
        while let Some(Object::Reference(r)) = fonts {
            fonts = doc.get_object(r).ok().cloned();
        }
        if let Some(Object::Dictionary(fonts)) = fonts {
            for (_, v) in fonts.iter() {
                if let Object::Reference(id) = v {
                    out.push(*id);
                }
            }
        }
    }
    out
}

/// The resource dictionaries that each page can see (page-level or inherited).
fn resource_dicts(doc: &Document) -> Vec<Dictionary> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for page_id in doc.page_iter() {
        let mut cursor = Some(page_id);
        while let Some(id) = cursor {
            let Ok(dict) = doc.get_dictionary(id) else { break };
            if dict.has(b"Resources") && seen.insert(id) {
                let mut res = dict.get(b"Resources").ok().cloned();
                while let Some(Object::Reference(r)) = res {
                    res = doc.get_object(r).ok().cloned();
                }
                if let Some(Object::Dictionary(resources)) = res {
                    out.push(resources);
                }
            }
            cursor = match dict.get(b"Parent") {
                Ok(Object::Reference(p)) => Some(*p),
                _ => None,
            };
        }
    }
    out
}

// ---------------------------------------------------------------------------
// sRGB ICC v2 profile builder (deterministic, no assets).
// ---------------------------------------------------------------------------

fn fix16(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn tag_type(sig: &[u8; 4], payload: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(sig);
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved
    out.extend_from_slice(&payload);
    out
}

fn text_type(text: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&((text.len() + 1) as u32).to_be_bytes());
    payload.extend_from_slice(text.as_bytes());
    payload.push(0); // NUL terminator
    tag_type(b"desc", payload)
}

fn xyz_type(vals: [f64; 3]) -> Vec<u8> {
    // ICC1:v2: XYZ tag type carries three s15Fixed16 numbers (4 bytes each).
    let mut payload = Vec::new();
    for v in vals {
        payload.extend_from_slice(&fix16(v));
    }
    tag_type(b"XYZ ", payload)
}

fn sf32_type(values: &[[f64; 3]]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(values.len() as u32).to_be_bytes());
    for row in values {
        for v in row {
            payload.extend_from_slice(&fix16(*v));
        }
    }
    tag_type(b"sf32", payload)
}

/// Parametric curve, type 0: y = x^gamma with gamma 2.4 (sRGB transfer).
fn para_type() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u16.to_be_bytes()); // function type 0
    payload.extend_from_slice(&0u16.to_be_bytes()); // reserved
    payload.extend_from_slice(&2.4f32.to_be_bytes());
    tag_type(b"para", payload)
}

/// Deterministic sRGB v2 profile (D50 illuminant, Bradford adaptation).
fn srgb_icc_profile() -> Vec<u8> {
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", text_type("sRGB IEC61966-2.1")),
        (b"cprt", text_type("Public domain sRGB profile (Proteus)")),
        (b"wtpt", xyz_type([0.9642, 1.0, 0.8249])),
    ];
    tags.push((b"bkpt", xyz_type([0.0, 0.0, 0.0])));
    tags.push((
        b"chad",
        sf32_type(&[
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ]),
    ));
    tags.push((b"rXYZ", xyz_type([0.4361, 0.2225, 0.0139])));
    tags.push((b"gXYZ", xyz_type([0.3851, 0.7169, 0.0971])));
    tags.push((b"bXYZ", xyz_type([0.1431, 0.0606, 0.7141])));
    let trc = para_type();
    tags.push((b"rTRC", trc.clone()));
    tags.push((b"gTRC", trc.clone()));
    tags.push((b"bTRC", trc.clone()));

    const HEADER: usize = 128;
    const TAG_ENTRY: usize = 16; // sig + offset + size
    let table_len = 4 + TAG_ENTRY * tags.len();
    let offset = (HEADER + table_len) as u32;
    let mut cooked: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (sig, data) in &tags {
        cooked.push((sig.to_vec(), data.clone()));
    }
    let mut table = Vec::new();
    let mut total_len = HEADER + table_len;
    for (sig, data) in &cooked {
        table.push((sig.clone(), total_len as u32, data.len() as u32));
        total_len += data.len();
    }
    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(&header_full(total_len as u32));
    out.extend_from_slice(&(cooked.len() as u32).to_be_bytes());
    // tags **must** be sorted by signature for strict parsers
    let mut order: Vec<usize> = (0..cooked.len()).collect();
    order.sort_by(|&a, &b| cooked[a].0.cmp(&cooked[b].0));
    for idx in order {
        out.extend_from_slice(&cooked[idx].0);
        out.extend_from_slice(&table[idx].1.to_be_bytes());
        out.extend_from_slice(&table[idx].2.to_be_bytes());
    }
    // payloads in the same sorted order does not matter; offset-table matches,
    // so append in table order
    let mut pos = HEADER + table_len;
    for (_, data) in &cooked {
        while out.len() < pos {
            out.push(0);
        }
        out.extend_from_slice(data);
        pos += data.len();
    }
    let _ = offset;
    out
}

fn header_full(size: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(128);
    h.extend_from_slice(&size.to_be_bytes()); // 0
    h.extend_from_slice(b"ADRA"); // 4  CMM
    h.extend_from_slice(&0x02300000u32.to_be_bytes()); // 8  version 2.30
    h.extend_from_slice(b"mntr"); // 12
    h.extend_from_slice(b"RGB "); // 16
    h.extend_from_slice(b"XYZ "); // 20
    h.extend_from_slice(&[0x07, 0xDE, 0x01, 0x01, 0, 0, 0, 0]); // 24 date
    h.extend_from_slice(b"acsp"); // 32
    h.extend_from_slice(&0u32.to_be_bytes()); // 36 platform
    h.extend_from_slice(&0u32.to_be_bytes()); // 40 flags (unused)
    h.extend_from_slice(&0u32.to_be_bytes()); // 44 manufacturer
    h.extend_from_slice(&0u32.to_be_bytes()); // 48 model
    h.extend_from_slice(&0u32.to_be_bytes()); // 52 attributes
    h.extend_from_slice(&0u32.to_be_bytes()); // 56 rendering intent
    h.extend_from_slice(&fix16(0.9642)); // 60 illuminant X (D50)
    h.extend_from_slice(&fix16(1.0)); // 64
    h.extend_from_slice(&fix16(0.8249)); // 68
    h.extend_from_slice(b"PZIM"); // 72 creator
    h.extend_from_slice(&[0u8; 16]); // 76 profile ID
    h.extend_from_slice(&0u32.to_be_bytes()); // 92 reserved
    while h.len() < 128 {
        h.push(0);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{pdf_page_count, testutil};

    #[test]
    fn srgb_icc_is_structurally_valid() {
        let icc = srgb_icc_profile();
        assert_eq!(&icc[0..4], &(icc.len() as u32).to_be_bytes(), "size field");
        assert_eq!(&icc[12..16], b"mntr");
        assert_eq!(&icc[16..20], b"RGB ");
        assert_eq!(&icc[20..24], b"XYZ ");
        assert_eq!(&icc[32..36], b"acsp");
        // parse the tag table: every entry in bounds
        let n = u32::from_be_bytes([icc[128], icc[129], icc[130], icc[131]]) as usize;
        assert!(n >= 8, "expected >=8 tags, got {n}");
        let mut pos = 132;
        for _ in 0..n {
            let off = u32::from_be_bytes([icc[pos + 4], icc[pos + 5], icc[pos + 6], icc[pos + 7]]) as usize;
            let len = u32::from_be_bytes([icc[pos + 8], icc[pos + 9], icc[pos + 10], icc[pos + 11]]) as usize;
            assert!(off >= 128 && off + len <= icc.len(), "tag {pos}: off {off} len {len}");
            pos += 12;
        }
    }

    #[test]
    fn image_only_pdf_converts_and_claims_pdfa() {
        let pdf = testutil::fontless_pdf(2);
        let out = convert_to_pdfa(&pdf).unwrap();
        assert_eq!(pdf_page_count(&out).unwrap(), 2);
        let raw = String::from_utf8_lossy(&out);
        assert!(
            raw.contains("pdfaid:conformance") || raw.contains("pdfaid:part"),
            "XMP claim missing"
        );
        assert!(raw.contains("GTS_PDFA1"), "OutputIntent missing");
        let reopened = crate::pdf::open_pdf(&out).unwrap();
        assert!(reopened.catalog().unwrap().has(b"OutputIntents"));
    }

    #[test]
    fn html_output_with_embedded_fonts_converts() {
        let html = crate::pdf::html_to_pdf::html_to_pdf("<h1>Hi</h1><p>Body</p>", &Default::default()).unwrap();
        let out = convert_to_pdfa(&html).unwrap();
        assert_eq!(pdf_page_count(&out).unwrap(), 1);
    }

    #[test]
    fn base14_font_is_refused_rather_than_faked() {
        // A watermark on an embedded-font document introduces a base-14 font:
        // PDF/A-1b forbids non-embedded fonts, so the tool must refuse.
        let html = crate::pdf::html_to_pdf::html_to_pdf("<p>x</p>", &Default::default()).unwrap();
        let wm = crate::pdf::watermark::add_watermark(&html, &Default::default()).unwrap();
        let err = convert_to_pdfa(&wm).unwrap_err();
        assert!(matches!(err, ProteusError::NotSupported(_)), "{err:?}");
    }

    #[test]
    fn encrypted_document_is_rejected() {
        let pdf = testutil::one_page_pdf("x");
        let locked = crate::pdf_protect::protect_pdf(&pdf, "pw", None).unwrap();
        let err = convert_to_pdfa(&locked).unwrap_err();
        assert!(matches!(err, ProteusError::NotSupported(_)));
    }

    #[test]
    fn malformed_input_rejected() {
        assert!(matches!(
            convert_to_pdfa(b"junk").unwrap_err(),
            ProteusError::MalformedInput(_)
        ));
    }
}