//! PDF structural operations (PRD §5, §9) — `lopdf` based, in-memory API.
//!
//! Module layout mirrors the PRD §9 tool list; every tool is T1 for its business
//! rule and T2 for malformed/adversarial input handling (PRD §7). The adversarial
//! pass over every surface lives in `tests::adversarial` in this file.
//!
//! Shared plumbing here (hardened open, save, deep page copying with id remapping
//! for merge/split/reorder, box math, resource helpers) is the layer every tool
//! sits on, so T2 parsing rules are uniform across surfaces.

pub mod afm;
pub mod compress;
pub mod crop;
pub mod html_to_pdf;
pub mod merge;
pub mod organize;
pub mod page_numbers;
pub mod pdf_to_a;
pub mod rotate;
pub mod split;
pub mod watermark;

pub use compress::compress_pdf;
pub use crop::crop_margins;
pub use merge::merge_pdfs;
pub use organize::reorder_pdf;
pub use page_numbers::add_page_numbers;
pub use rotate::rotate_pdf;
pub use split::split_pdf;
pub use watermark::add_watermark;

use std::collections::{BTreeSet, HashMap};

use crate::error::ProteusError;
use crate::check_input_size;
use lopdf::{dictionary, Dictionary, Document, LoadOptions, Object, ObjectId, Stream};

/// Ceiling for bytes any single stream may decompress to while loading untrusted
/// PDFs (decompression-bomb guard, PRD §7 adversarial inputs).
pub(crate) const MAX_STREAM_DECOMPRESS_BYTES: usize = 256 * 1024 * 1024;

/// Post-load guard: inflate every /FlateDecode stream with a hard cap.
///
/// lopdf's `LoadOptions::max_decompressed_size` bounds only the streams the
/// loader itself decodes (xref streams); page content streams decode lazily,
/// unbounded (lopdf#887 bomb warning). This sweep closes that gap: each flate
/// stream must decode to <= MAX_STREAM_DECOMPRESS_BYTES or the file is
/// rejected as malformed, before any consumer can force a full inflate.
pub(crate) fn reject_bomb_streams(doc: &mut Document) -> Result<(), ProteusError> {
    for obj in doc.objects.values_mut() {
        let Object::Stream(stream) = obj else { continue };
        let has_flate = match stream.dict.get(b"Filter") {
            Ok(Object::Name(n)) => n == b"FlateDecode",
            Ok(Object::Array(a)) => a
                .iter()
                .any(|o| matches!(o, Object::Name(n) if n == b"FlateDecode")),
            _ => false,
        };
        if has_flate {
            stream
                .decompressed_content_with_limit(MAX_STREAM_DECOMPRESS_BYTES)
                .map_err(|e| {
                    ProteusError::MalformedInput(format!(
                        "stream decompresses beyond the {} MiB cap: {e}",
                        MAX_STREAM_DECOMPRESS_BYTES / (1024 * 1024)
                    ))
                })?;
        }
    }
    Ok(())
}

/// Load a PDF with the hardened layer (500 MB cap + decompression-bomb guard).
/// Every PDF tool funnels through here (T2 malformed-input handling).
pub fn open_pdf(bytes: &[u8]) -> Result<Document, ProteusError> {
    check_input_size(bytes)?;
    let options = LoadOptions {
        max_decompressed_size: Some(MAX_STREAM_DECOMPRESS_BYTES),
        ..Default::default()
    };
    let mut doc = Document::load_mem_with_options(bytes, options)
        .map_err(|e| ProteusError::MalformedInput(format!("cannot parse PDF: {e}")))?;
    reject_bomb_streams(&mut doc)?;
    Ok(doc)
}

/// Open a PDF with a password. `NotEncrypted` if the document has no encryption;
/// `WrongPassword` if the password fails; `MalformedInput` for anything unparsable.
pub fn open_pdf_with_password(bytes: &[u8], password: &str) -> Result<Document, ProteusError> {
    if password.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "open_pdf_with_password",
            reason: "password may not be empty".into(),
        });
    }
    check_input_size(bytes)?;
    // lopdf loads encrypted files passwordless without error (the content
    // stays opaque); encryption is only detectable via is_encrypted().
    let passwordless = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_STREAM_DECOMPRESS_BYTES),
            ..Default::default()
        },
    );
    let passwordless = match passwordless {
        Ok(mut doc) => {
            reject_bomb_streams(&mut doc)?;
            doc
        }
        // A file that fails the passwordless load cannot be classified further;
        // the passworded load below maps InvalidPassword -> WrongPassword.
        Err(e) => return Err(ProteusError::MalformedInput(format!("cannot parse PDF: {e}"))),
    };
    if !passwordless.is_encrypted() {
        return Err(ProteusError::NotEncrypted);
    }
    let options = LoadOptions {
        password: Some(password.to_owned()),
        max_decompressed_size: Some(MAX_STREAM_DECOMPRESS_BYTES),
        ..Default::default()
    };
    Document::load_mem_with_options(bytes, options).map_err(|e| match e {
        lopdf::Error::InvalidPassword => ProteusError::WrongPassword,
        e => ProteusError::MalformedInput(format!("cannot parse PDF: {e}")),
    })
}

/// Serialize a document back to bytes, fully in-memory (PRD §8).
pub fn save_pdf(doc: &mut Document) -> Result<Vec<u8>, ProteusError> {
    let mut out = std::io::Cursor::new(Vec::new());
    doc.save_to(&mut out)
        .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
    Ok(out.into_inner())
}

/// Flat page-object ids in tree order.
pub(crate) fn page_ids(doc: &Document) -> Vec<ObjectId> {
    doc.page_iter().collect()
}

/// 1-based page count of a PDF document.
pub fn pdf_page_count(bytes: &[u8]) -> Result<u32, ProteusError> {
    let doc = open_pdf(bytes)?;
    Ok(page_ids(&doc).len() as u32)
}

/// Text layer of the whole document (oracle support and face validation).
pub fn extract_pdf_text(bytes: &[u8]) -> Result<String, ProteusError> {
    let doc = open_pdf(bytes)?;
    let pages: Vec<u32> = (1..=page_ids(&doc).len() as u32).collect();
    doc.extract_text(&pages)
        .map_err(|e| ProteusError::Pdf(Box::new(e)))
}

/// A page's /key box, following the parent chain (MediaBox etc. are inheritable
/// per ISO 32000-1 §7.7.3.4 — fixtures put them on the Pages node).
pub(crate) fn page_box(doc: &Document, page: ObjectId, key: &[u8]) -> Result<[f32; 4], ProteusError> {
    let mut cursor = Some(page);
    while let Some(id) = cursor {
        let dict = doc.get_dictionary(id).map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        if let Ok(arr) = dict.get(key) {
            let arr: &Object = arr;
            if let Object::Array(items) = arr {
                return parse_box_array(items);
            }
        }
        cursor = match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    Err(ProteusError::NotSupported(format!(
        "page {} has no /{} anywhere in its ancestry",
        page.0,
        String::from_utf8_lossy(key)
    )))
}

fn parse_box_array(arr: &[Object]) -> Result<[f32; 4], ProteusError> {
    let mut out = [0.0f32; 4];
    if arr.len() != 4 {
        return Err(ProteusError::MalformedInput(format!(
            "box array has {} entries, expected 4",
            arr.len()
        )));
    }
    for (i, v) in arr.iter().enumerate().take(4) {
        out[i] = match v {
            Object::Integer(n) => *n as f32,
            Object::Real(x) => *x,
            other => {
                return Err(ProteusError::MalformedInput(format!(
                    "box entry {i} is not a number: {other:?}"
                )))
            }
        };
    }
    Ok(out)
}

/// Visible area of a page: CropBox when set, else MediaBox.
pub(crate) fn page_visible_box(doc: &Document, page: ObjectId) -> Result<[f32; 4], ProteusError> {
    match page_box(doc, page, b"CropBox") {
        Ok(b) => Ok(b),
        Err(ProteusError::NotSupported(_)) => page_box(doc, page, b"MediaBox"),
        Err(e) => Err(e),
    }
}

pub(crate) fn set_page_box(
    doc: &mut Document,
    page: ObjectId,
    key: &[u8],
    value: [f32; 4],
) -> Result<(), ProteusError> {
    let dict = doc
        .get_dictionary_mut(page)
        .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
    dict.set(
        key,
        Object::Array(value.iter().map(|v| Object::Real(*v)).collect()),
    );
    Ok(())
}

/// Deep-copy the catalog-reachable closure of `src` into `dst` with fresh ids,
/// rewriting every cross-reference. Returns the ids of the copied pages in tree
/// order. Foundation of merge / split / organize.
pub(crate) fn append_document(
    dst: &mut Document,
    src: &Document,
) -> Result<Vec<ObjectId>, ProteusError> {
    let mut src = src.clone();
    let reachable = src.traverse_objects(|_| {});
    let remap: HashMap<ObjectId, ObjectId> = reachable
        .iter()
        .map(|id| (*id, dst.new_object_id()))
        .collect();
    for id in &reachable {
        let obj = src
            .get_object(*id)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?
            .clone();
        dst.set_object(remap[id], rewrite_refs(obj, &remap));
    }
    let mut pages = Vec::new();
    for id in src.page_iter() {
        pages.push(*remap.get(&id).ok_or_else(|| {
            ProteusError::MalformedInput(format!("page object {id:?} not in catalog closure"))
        })?);
    }
    Ok(pages)
}

fn rewrite_refs(obj: Object, remap: &HashMap<ObjectId, ObjectId>) -> Object {
    match obj {
        Object::Reference(id) => Object::Reference(*remap.get(&id).unwrap_or(&id)),
        Object::Array(values) => Object::Array(
            values.into_iter().map(|o| rewrite_refs(o, remap)).collect(),
        ),
        Object::Dictionary(dict) => Object::Dictionary(rewrite_dict(dict, remap)),
        Object::Stream(stream) => Object::Stream(
            Stream::new(rewrite_dict(stream.dict, remap), stream.content.to_vec()),
        ),
        other => other,
    }
}

fn rewrite_dict(dict: Dictionary, remap: &HashMap<ObjectId, ObjectId>) -> Dictionary {
    dict.into_iter()
        .map(|(k, o)| (k, rewrite_refs(o, remap)))
        .collect()
}

/// Re-root copied pages under a single flat Pages node with a fresh catalog.
pub(crate) fn finalize_pages(
    dst: &mut Document,
    pages: Vec<ObjectId>,
) -> Result<(), ProteusError> {
    if pages.is_empty() {
        return Err(ProteusError::NotSupported(
            "result would contain zero pages".into(),
        ));
    }
    let mut unique = BTreeSet::new();
    let pages: Vec<ObjectId> = pages.into_iter().filter(|p| unique.insert(*p)).collect();
    let pages_id = dst.new_object_id();
    let kids: Vec<Object> = pages.iter().map(|p| Object::Reference(*p)).collect();
    for p in &pages {
        let dict = dst
            .get_dictionary_mut(*p)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        dict.set(b"Parent", pages_id);
    }
    dst.set_object(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => kids.clone(),
            "Count" => kids.len() as i64,
        },
    );
    let catalog_id = dst.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    dst.trailer.set(b"Root", catalog_id);
    Ok(())
}

/// Drop objects that are no longer reachable from the trailer.
pub(crate) fn prune_unreachable(dst: &mut Document) {
    let mut probe = dst.clone();
    let reachable: BTreeSet<ObjectId> = probe.traverse_objects(|_| {}).into_iter().collect();
    dst.objects.retain(|id, _| reachable.contains(id));
}

/// Ensure `resources.<kind>` contains `entry_tag -> value` on a page.
///
/// Inheritable resources live on ancestor Pages nodes (ISO 32000-1 §7.7.3.4);
/// creating a page-level Resources dict would shadow them, breaking the page's
/// existing text/images. So if the page has no Resources, we build the page's
/// dict as a first-wins copy of its ancestors' merged resource dictionaries.
pub(crate) fn set_resource_entry(
    doc: &mut Document,
    page: ObjectId,
    kind: &[u8],
    entry_tag: &[u8],
    value: Object,
) -> Result<(), ProteusError> {
    // 1. If the page has no Resources of its own, materialize the inherited one.
    let has_resources = doc
        .get_dictionary(page)
        .map_err(|e| ProteusError::Pdf(Box::new(e)))?
        .has(b"Resources");
    if !has_resources {
        let merged = inherited_resources(doc, page)?;
        let id = doc.add_object(Object::Dictionary(merged));
        let page_dict = doc
            .get_dictionary_mut(page)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        page_dict.set(b"Resources", Object::Reference(id));
    }

    // 2. Resolve the resources dict object id (following reference chains).
    //    Inline dictionaries are materialized into an indirect object so the
    //    mutation lands in a shared, mutable place (ISO 32000-1 §7.8.3).
    let resources_id = {
        let page_dict = doc
            .get_dictionary(page)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        let mut current = page_dict.get(b"Resources").cloned().unwrap_or(Object::Null);
        let mut resolved = None;
        while let Object::Reference(id) = current {
            resolved = Some(id);
            current = doc
                .get_object(id)
                .map_err(|e| ProteusError::Pdf(Box::new(e)))?
                .clone();
        }
        match (resolved, current) {
            (Some(id), _) => id,
            (None, Object::Dictionary(inline)) => {
                let id = doc.add_object(Object::Dictionary(inline));
                let page_dict = doc
                    .get_dictionary_mut(page)
                    .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
                page_dict.set(b"Resources", Object::Reference(id));
                id
            }
            _ => {
                return Err(ProteusError::MalformedInput(
                    "page Resources is not a dictionary".into(),
                ))
            }
        }
    };

    // 3. Resolve (or create) the kind sub-dictionary.
    let kind_id = {
        let resources = doc
            .get_dictionary(resources_id)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        let mut current = resources.get(kind).cloned().unwrap_or(Object::Null);
        let mut kind_ref = None;
        while let Object::Reference(id) = current {
            kind_ref = Some(id);
            current = doc
                .get_object(id)
                .map_err(|e| ProteusError::Pdf(Box::new(e)))?
                .clone();
        }
        match (kind_ref, current) {
            (Some(id), _) => id,
            (None, Object::Dictionary(inline)) => {
                let id = doc.add_object(Object::Dictionary(inline));
                let resources = doc
                    .get_dictionary_mut(resources_id)
                    .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
                resources.set(kind, Object::Reference(id));
                id
            }
            (None, _) => {
                let id = doc.add_object(Dictionary::new());
                let resources = doc
                    .get_dictionary_mut(resources_id)
                    .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
                resources.set(kind, Object::Reference(id));
                id
            }
        }
    };

    // 4. Insert into the owned kind dict.
    let kind_dict = doc
        .get_dictionary_mut(kind_id)
        .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
    kind_dict.set(entry_tag, value);
    Ok(())
}

/// Merge the resource dictionaries from `page` and its ancestors (closest
/// wins per key, per ISO 32000-1 §7.7.3.4 inheritance).
fn inherited_resources(doc: &Document, page: ObjectId) -> Result<Dictionary, ProteusError> {
    let mut merged = Dictionary::new();
    let mut cursor = Some(page);
    while let Some(id) = cursor {
        let dict = doc.get_dictionary(id).map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        if let Ok(resources) = dict.get(b"Resources") {
            let mut current = resources.clone();
            while let Object::Reference(ref_id) = current {
                current = doc
                    .get_object(ref_id)
                    .map_err(|e| ProteusError::Pdf(Box::new(e)))?
                    .clone();
            }
            if let Object::Dictionary(d) = current {
                for (k, v) in d.into_iter() {
                    // First source walking up wins.
                    if !merged.has(&k) {
                        merged.set(k, v);
                    }
                }
            } else {
                return Err(ProteusError::MalformedInput(format!(
                    "resources of object {} is not a dictionary",
                    id.0
                )));
            }
        }
        cursor = match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    Ok(merged)
}

/// Add a base-14 (non-embedded) font dictionary to the document.
/// Used for watermark and page-number text (viewers substitute the base-14 face).
pub(crate) fn add_base14_font(doc: &mut Document, base_font: &str) -> ObjectId {
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => base_font,
        "Encoding" => "WinAnsiEncoding",
    })
}

/// Content-stream ops for one text line at (x, y) in the given font tag/size.
/// Returns raw ops WITHOUT BT/ET wrapper for embedding into streams.
pub(crate) fn text_line_ops(font_tag: &[u8], size: f32, x: f32, y: f32, text: &[u8]) -> Vec<u8> {
    let tag = String::from_utf8_lossy(font_tag);
    let mut out = Vec::new();
    out.extend_from_slice(b"BT\n");
    out.extend_from_slice(format!("/{tag} {size} Tf\n").as_bytes());
    out.extend_from_slice(format!("1 0 0 1 {x} {y} Tm\n").as_bytes());
    out.extend_from_slice(b"(");
    out.extend_from_slice(&escape_pdf_string(text));
    out.extend_from_slice(b") Tj\nET\n");
    out
}

/// Escape raw bytes for a PDF literal string literal.
pub(crate) fn escape_pdf_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 8);
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            0..=31 | 126..=255 if b != b'\n' && b != b'\r' => {
                out.push(b'\\');
                out.extend_from_slice(format!("{:03o}", b).as_bytes());
            }
            _ => out.push(b),
        }
    }
    out
}

/// Map Unicode chars to WinAnsi-ish bytes for literal strings; non-Latin-1
/// becomes '?' (v1: watermark/page-number text is Latin-1 in practice).
pub(crate) fn pdf_text_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c as u32 {
            0..=0x7E => c as u8,
            0x20AC => 0x80,
            0x201A => 0x82,
            0x0192 => 0x83,
            0x201E => 0x84,
            0x2026 => 0x85,
            0x2020 => 0x86,
            0x2021 => 0x87,
            0x02C6 => 0x88,
            0x2030 => 0x89,
            0x0160 => 0x8A,
            0x2039 => 0x8B,
            0x0152 => 0x8C,
            0x017D => 0x8E,
            0x2018 => 0x91,
            0x2019 => 0x92,
            0x201C => 0x93,
            0x201D => 0x94,
            0x2022 => 0x95,
            0x2013 => 0x96,
            0x2014 => 0x97,
            0x02DC => 0x98,
            0x2122 => 0x99,
            0x0161 => 0x9A,
            0x203A => 0x9B,
            0x0153 => 0x9C,
            0x017E => 0x9E,
            0x0178 => 0x9F,
            0xA0..=0xFF => c as u8,
            _ => b'?',
        })
        .collect()
}

/// Append raw ops to a page's content (preserves existing content).
pub(crate) fn append_content(
    doc: &mut Document,
    page: ObjectId,
    ops: Vec<u8>,
) -> Result<(), ProteusError> {
    doc.add_page_contents(page, ops.to_vec())
        .map_err(|e| ProteusError::Pdf(Box::new(e)))
}

/// Concatenated decoded content bytes of a page (content-preservation oracles).
#[cfg(test)]
pub(crate) fn page_content_of(doc: &Document, page: ObjectId) -> Vec<u8> {
    doc.get_page_content(page)
}

/// Extract the text layer of a single page by its 1-based number.
pub fn extract_page_text(bytes: &[u8], page_no: u32) -> Result<String, ProteusError> {
    let doc = open_pdf(bytes)?;
    doc.extract_text(&[page_no])
        .map_err(|e| ProteusError::Pdf(Box::new(e)))
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// Fixture: n pages with the given text markers (each page carries one).
    pub fn marker_pdf(texts: &[&str]) -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let mut kids = Vec::new();
        for text in texts {
            let mut ops = Vec::new();
            ops.extend_from_slice(b"BT /F1 12 Tf 72 720 Td (");
            ops.extend_from_slice(&escape_pdf_string(&pdf_text_bytes(text)));
            ops.extend_from_slice(b") Tj ET");
            let content_id = doc.add_object(Stream::new(dictionary! {}, ops));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "MediaBox" => vec![Object::Real(0.0), Object::Real(0.0), Object::Real(612.0), Object::Real(792.0)],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                "Contents" => content_id,
            });
            kids.push(Object::Reference(page_id));
        }
        let pages_id = doc.new_object_id();
        doc.set_object(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => texts.len() as i64 },
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set(b"Root", catalog_id);
        save_pdf(&mut doc).expect("fixture must serialize")
    }

    pub fn one_page_pdf(text: &str) -> Vec<u8> {
        marker_pdf(&[text])
    }

    pub fn three_page_pdf() -> Vec<u8> {
        marker_pdf(&["alpha", "beta", "gamma"])
    }

    /// A PDF with no text content across `pages` pages.
    pub fn blank_pdf(pages: u32) -> Vec<u8> {
        let texts: Vec<&str> = (0..pages).map(|_| "").collect();
        marker_pdf(&texts)
    }

    /// Pages with no Font resources at all (for PDF/A paths that must not
    /// trip the embedded-font gate).
    pub fn fontless_pdf(pages: u32) -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let mut kids = Vec::new();
        for _ in 0..pages {
            let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "MediaBox" => vec![Object::Real(0.0), Object::Real(0.0), Object::Real(612.0), Object::Real(792.0)],
                "Contents" => content_id,
            });
            kids.push(Object::Reference(page_id));
        }
        let pages_id = doc.new_object_id();
        doc.set_object(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => pages as i64 },
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set(b"Root", catalog_id);
        save_pdf(&mut doc).expect("fixture must serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_INPUT_FILE_BYTES;
    use crate::pdf::{
        compress::compress_pdf, crop::crop_margins, merge::merge_pdfs,
        organize::reorder_pdf, page_numbers::add_page_numbers, pdf_to_a::convert_to_pdfa,
        rotate::rotate_pdf, split::{split_pdf, PageRange}, watermark::add_watermark,
    };
    use proptest::prelude::*;

    fn garbage_inputs() -> Vec<Vec<u8>> {
        vec![
            Vec::new(),
            b"%PDF-1.4".to_vec(),
            b"not a pdf at all".to_vec(),
            b"\x00\x01\x02\x03\x04".to_vec(),
            b"%PDF-1.7\n1 0 obj\n<< /Length 999999999 >>\nstream\n".to_vec(),
            b"%PDF-1.4\n%%EOF\n".to_vec(),
            {
                let mut t = testutil::three_page_pdf();
                t.truncate(t.len() / 2);
                t
            },
        ]
    }

    // ---- T2 malformed-input handling -----------------------------------------

    #[test]
    fn open_pdf_maps_garbage_to_malformed_input() {
        for input in garbage_inputs() {
            let err = open_pdf(&input);
            assert!(
                matches!(err, Err(ProteusError::MalformedInput(_))),
                "garbage must be MalformedInput, got {err:?}"
            );
        }
    }

    #[test]
    fn open_pdf_accepts_valid_document() {
        let bytes = testutil::three_page_pdf();
        let doc = open_pdf(&bytes).expect("valid pdf opens");
        assert_eq!(page_ids(&doc).len(), 3);
    }

    #[test]
    fn every_surface_rejects_garbage_cleanly() {
        // No panics, no hangs, every op fails with a domain error.
        type SweepOp = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, ProteusError>>;
        let ops: Vec<SweepOp> = vec![
            Box::new(|b| merge_pdfs(&[b])),
            Box::new(|b| split_pdf(b, &[PageRange::new(1, 1)]).map(|pages| pages.into_iter().flatten().collect())),
            Box::new(|b| reorder_pdf(b, &[1])),
            Box::new(|b| rotate_pdf(b, 90)),
            Box::new(compress_pdf),
            Box::new(|b| crop_margins(b, Default::default())),
            Box::new(|b| add_watermark(b, &Default::default())),
            Box::new(|b| add_page_numbers(b, &Default::default())),
            Box::new(convert_to_pdfa),
            Box::new(|b| crate::pdf_protect::protect_pdf(b, "pw", None)),
            Box::new(|b| crate::pdf_protect::unlock_pdf(b, "pw")),
            Box::new(|b| crate::image_ops::compress_image(b, None)),
            Box::new(|b| crate::image_ops::resize_image(b, 10, 10, true)),
            Box::new(|b| crate::image_ops::crop_image(b, 0, 0, 1, 1)),
            Box::new(|b| crate::image_ops::convert_image(b, crate::image_ops::ImageFormat::Jpeg)),
            Box::new(|b| crate::image_ops::rotate_image(b, 90.0)),
        ];
        for input in garbage_inputs() {
            for op in &ops {
                let res = op(&input);
                match res {
                    Err(ProteusError::MalformedInput(_))
                    | Err(ProteusError::InvalidArgument { .. })
                    | Err(ProteusError::NotSupported(_))
                    | Err(ProteusError::NotEncrypted)
                    | Err(ProteusError::WrongPassword) => {}
                    other => panic!(
                        "op must reject garbage {:?} with a domain error, got {:?}",
                        input, other
                    ),
                }
            }
        }
    }

    #[test]
    fn cap_is_enforced_before_any_parsing() {
        let big: Vec<u8> = vec![0u8; MAX_INPUT_FILE_BYTES as usize + 1];
        let err = open_pdf(&big).unwrap_err();
        assert!(
            matches!(err, ProteusError::InputTooLarge { limit_mb: 500 }),
            "cap must be reported, got {err:?}"
        );
    }

    // ---- shared pipe oracles -------------------------------------------------

    #[test]
    fn page_count_oracle_counts_leaf_pages() {
        assert_eq!(pdf_page_count(&testutil::marker_pdf(&["a", "b"])).unwrap(), 2);
        assert_eq!(pdf_page_count(&testutil::one_page_pdf("x")).unwrap(), 1);
        assert!(matches!(
            pdf_page_count(b"garbage"),
            Err(ProteusError::MalformedInput(_))
        ));
    }

    #[test]
    fn extract_text_round_trips_markers() {
        let bytes = testutil::three_page_pdf();
        let text = extract_pdf_text(&bytes).unwrap();
        assert!(text.contains("alpha"), "text layer: {text}");
        assert!(text.contains("beta"), "text layer: {text}");
        assert!(text.contains("gamma"), "text layer: {text}");
    }

    proptest! {
        /// split→merge round-trip preserves page count and text order (T1/T2 property).
        #[test]
        fn split_merge_roundtrip_preserves_count_and_text(
            page_texts in prop::collection::vec(prop::collection::vec("[a-z]{3,8}", 1..4), 1..3),
        ) {
            let parts: Vec<Vec<u8>> = page_texts.iter()
                .map(|t| testutil::marker_pdf(&t.iter().map(|w| w.as_str()).collect::<Vec<_>>()))
                .collect();
            let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
            let merged = merge_pdfs(&refs).expect("merge must succeed");
            let expected: u32 = parts.iter().map(|p| pdf_page_count(p).unwrap()).sum();
            let actual = pdf_page_count(&merged).unwrap();
            prop_assert_eq!(actual, expected, "merged page count must be the sum");
            let text = extract_pdf_text(&merged).unwrap();
            for part in page_texts.iter() {
                for marker in part {
                    prop_assert!(text.contains(marker.as_str()), "marker {marker} lost: {text}");
                }
            }
        }
    }

    // ---- T2 oracles: mutate-input guards and inheritance walkers (PRD §7) ----

    /// Deflate (zlib) of the payload — PDF /FlateDecode streams.
    fn zlib_stream(payload: &[u8]) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(payload).unwrap();
        enc.finish().unwrap()
    }

    /// one_page_pdf with its content stream replaced by a FlateDecode stream
    /// that decompresses to `size` zero bytes.
    fn pdf_with_stream_of_size(size: usize) -> Vec<u8> {
        let mut doc = Document::load_mem(&testutil::one_page_pdf("mark")).unwrap();
        let page = crate::pdf::page_ids(&doc)[0];
        let content_id = match doc.get_dictionary(page).unwrap().get(b"Contents").unwrap() {
            Object::Reference(id) => *id,
            other => panic!("fixture must have indirect contents, got {other:?}"),
        };
        let stream = Stream::new(
            dictionary! { "Filter" => "FlateDecode" },
            zlib_stream(&vec![0u8; size]),
        );
        doc.objects.insert(content_id, Object::Stream(stream));
        save_pdf(&mut doc).unwrap()
    }

    #[test]
    fn decompression_bomb_over_cap_is_rejected() {
        // 300 MiB >> MAX_STREAM_DECOMPRESS_BYTES: refuse to inflate, don't OOM.
        let bytes = pdf_with_stream_of_size(300 * 1024 * 1024);
        let err = open_pdf(&bytes).unwrap_err();
        assert!(matches!(err, ProteusError::MalformedInput(_)), "{err:?}");
    }

    #[test]
    fn bomb_guard_covers_array_filters() {
        // /Filter as an ARRAY ([FlateDecode]) must be swept too — lopdf may
        // prefix other filters (e.g. /Crypt) before the flate layer.
        let mut doc = Document::load_mem(&testutil::one_page_pdf("mark")).unwrap();
        let page = crate::pdf::page_ids(&doc)[0];
        let content_id = match doc.get_dictionary(page).unwrap().get(b"Contents").unwrap() {
            Object::Reference(id) => *id,
            other => panic!("fixture must have indirect contents, got {other:?}"),
        };
        let stream = Stream::new(
            dictionary! { "Filter" => vec![Object::Name("FlateDecode".into())] },
            zlib_stream(&vec![0u8; 300 * 1024 * 1024]),
        );
        doc.objects.insert(content_id, Object::Stream(stream));
        let bytes = save_pdf(&mut doc).unwrap();
        let err = open_pdf(&bytes).unwrap_err();
        assert!(matches!(err, ProteusError::MalformedInput(_)), "{err:?}");
    }

    #[test]
    fn large_bounded_stream_is_accepted() {
        // 2 MiB: below the bomb cap; big-but-normalized streams must load.
        let bytes = pdf_with_stream_of_size(2 * 1024 * 1024);
        let doc = open_pdf(&bytes).expect("2 MiB stream is below the bomb cap");
        assert_eq!(crate::pdf::page_ids(&doc).len(), 1);
    }

    #[test]
    fn pdf_text_bytes_maps_whole_winansi_charset() {
        let pairs: &[(char, u8)] = &[
            ('\u{20AC}', 0x80), ('\u{201A}', 0x82), ('\u{0192}', 0x83),
            ('\u{201E}', 0x84), ('\u{2026}', 0x85), ('\u{2020}', 0x86),
            ('\u{2021}', 0x87), ('\u{02C6}', 0x88), ('\u{2030}', 0x89),
            ('\u{0160}', 0x8A), ('\u{0161}', 0x9A), ('\u{2039}', 0x8B), ('\u{0152}', 0x8C),
            ('\u{017D}', 0x8E), ('\u{2018}', 0x91), ('\u{2019}', 0x92),
            ('\u{201C}', 0x93), ('\u{201D}', 0x94), ('\u{2022}', 0x95),
            ('\u{2013}', 0x96), ('\u{2014}', 0x97), ('\u{02DC}', 0x98),
            ('\u{2122}', 0x99), ('\u{203A}', 0x9B), ('\u{0153}', 0x9C),
            ('\u{017E}', 0x9E), ('\u{0178}', 0x9F),
            ('\u{00A9}', 0xA9), ('\u{00E9}', 0xE9), ('\u{00FF}', 0xFF),
        ];
        for &(c, byte) in pairs {
            assert_eq!(
                pdf_text_bytes(&c.to_string()),
                &[byte],
                "WinAnsi char U+{:04X}",
                c as u32
            );
        }
        assert_eq!(pdf_text_bytes("中"), b"?", "non-Latin-1 must map to '?'");
    }

    #[test]
    fn escape_pdf_string_escapes_specials_and_keeps_newlines() {
        let escaped = escape_pdf_string(b"a(b)c\\d\ne\n\r\tf");
        let s = String::from_utf8_lossy(&escaped).into_owned();
        assert!(s.contains(r"\("), "open paren must be escaped");
        assert!(s.contains(r"\)"), "close paren must be escaped");
        assert!(s.contains(r"\\"), "backslash must be escaped");
        assert!(s.contains('\n'), "LF stays literal in PDF strings");
        assert!(s.contains('\r'), "CR stays literal in PDF strings");
        assert!(s.contains(r"\011"), "tab must become \\011");
        assert!(!s.contains('\t'), "raw tab must not appear");
    }

    /// Chain: page (no Resources/MediaBox) -> p1 (Font F1) -> p2 (Font F1 with
    /// a different font object) -> root Pages. Closest ancestor must win.
    fn inheritance_chain() -> (Document, ObjectId, ObjectId, ObjectId) {
        let mut doc = Document::with_version("1.4");
        let f1 = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let f2 = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" });
        let res1 = doc.add_object(Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(f1) },
        }));
        let res2 = doc.add_object(Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(f2) },
        }));
        let page_id = doc.new_object_id();
        let p1 = doc.new_object_id();
        let p2 = doc.new_object_id();
        let root = doc.new_object_id();
        doc.set_object(page_id, dictionary! { "Type" => "Page", "Parent" => Object::Reference(p1) });
        doc.set_object(p1, dictionary! { "Type" => "Pages", "Count" => 1,
            "Parent" => Object::Reference(p2),
            "Resources" => Object::Reference(res1),
            "Kids" => vec![Object::Reference(page_id)] });
        doc.set_object(p2, dictionary! { "Type" => "Pages", "Count" => 1,
            "Parent" => Object::Reference(root),
            "Resources" => Object::Reference(res2),
            "Kids" => vec![Object::Reference(p1)] });
        doc.set_object(root, dictionary! { "Type" => "Pages", "Count" => 1,
            "Kids" => vec![Object::Reference(p2)] });
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set(b"Root", catalog);
        (doc, page_id, f1, f2)
    }

    fn root_pages_id(doc: &Document, page: ObjectId) -> ObjectId {
        let mut id = page;
        loop {
            match doc.get_dictionary(id).unwrap().get(b"Parent") {
                Ok(Object::Reference(p)) => id = *p,
                _ => return id,
            }
        }
    }

    #[test]
    fn page_box_resolves_inherited_mediabox() {
        let (mut doc, page, _, _) = inheritance_chain();
        let root = root_pages_id(&doc, page);
        doc.get_dictionary_mut(root).unwrap().set(
            b"MediaBox",
            vec![Object::Real(0.0), Object::Real(0.0), Object::Real(612.0), Object::Real(792.0)],
        );
        assert_eq!(page_box(&doc, page, b"MediaBox").unwrap(), [0.0, 0.0, 612.0, 792.0]);
        let err = page_box(&doc, page, b"CropBox").unwrap_err();
        assert!(matches!(err, ProteusError::NotSupported(_)), "{err:?}");
    }

    #[test]
    fn page_box_rejects_short_box_array() {
        let (mut doc, page, _, _) = inheritance_chain();
        let root = root_pages_id(&doc, page);
        doc.get_dictionary_mut(root).unwrap().set(
            b"MediaBox",
            vec![Object::Real(0.0), Object::Real(0.0), Object::Real(612.0)],
        );
        let err = page_box(&doc, page, b"MediaBox").unwrap_err();
        assert!(matches!(err, ProteusError::MalformedInput(_)), "{err:?}");
    }

    #[test]
    fn prune_unreachable_drops_orphans() {
        let mut doc = Document::load_mem(&testutil::one_page_pdf("mark")).unwrap();
        let orphan = doc.add_object(dictionary! { "Type" => "Orphan" });
        assert!(doc.objects.contains_key(&orphan));
        prune_unreachable(&mut doc);
        assert!(
            !doc.objects.contains_key(&orphan),
            "objects unreachable from the trailer must be pruned"
        );
        assert_eq!(crate::pdf::page_ids(&doc).len(), 1, "reachable structure stays");
    }

    #[test]
    fn set_resource_entry_keeps_existing_page_resources() {
        let mut doc = Document::load_mem(&testutil::one_page_pdf("mark")).unwrap();
        let page = crate::pdf::page_ids(&doc)[0];
        // The fixture's Resources is an inline dict (not yet materialized).
        assert!(matches!(
            doc.get_dictionary(page).unwrap().get(b"Resources").unwrap(),
            Object::Dictionary(_)
        ));
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Times-Roman" });
        set_resource_entry(&mut doc, page, b"Font", b"G1", Object::Reference(font)).unwrap();
        // The inline dict is materialized into an indirect object, and both the
        // existing F1 and the new G1 survive — nothing was shadowed or lost.
        let resources_id = match doc.get_dictionary(page).unwrap().get(b"Resources").unwrap() {
            Object::Reference(id) => *id,
            other => panic!("page Resources must be materialized as indirect, got {other:?}"),
        };
        let resources = doc.get_dictionary(resources_id).unwrap();
        let fonts = match resources.get(b"Font").unwrap() {
            Object::Reference(id) => doc.get_dictionary(*id).unwrap(),
            Object::Dictionary(d) => d,
            other => panic!("Font resources must be a dict, got {other:?}"),
        };
        assert!(fonts.get(b"F1").is_ok(), "existing fonts preserved");
        assert!(fonts.get(b"G1").is_ok(), "G1 entry must be added");
    }

    #[test]
    fn set_resource_entry_merges_closest_ancestor_resources() {
        // Closest ancestor wins per key (page-side F1 beats grandparent F1),
        // the new entry is added, and the page gets its own Resources dict.
        let (mut doc, page, f1, f2) = inheritance_chain();
        let g1 = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Symbol" });
        set_resource_entry(&mut doc, page, b"Font", b"G1", Object::Reference(g1)).unwrap();
        let resources_id = match doc.get_dictionary(page).unwrap().get(b"Resources").unwrap() {
            Object::Reference(id) => *id,
            other => panic!("page Resources must be an indirect dict, got {other:?}"),
        };
        let resources = doc.get_dictionary(resources_id).unwrap();
        let fonts = match resources.get(b"Font").unwrap() {
            Object::Reference(id) => doc.get_dictionary(*id).unwrap(),
            Object::Dictionary(d) => d,
            other => panic!("Font resources must be a dict, got {other:?}"),
        };
        match fonts.get(b"F1") {
            Ok(Object::Reference(id)) if *id == f1 => {}
            other => panic!("closest F1 must be the parent's font, got {other:?} (f2={f2:?} leaked?)"),
        }
        assert!(fonts.get(b"G1").is_ok(), "new font must be added");
    }
}
