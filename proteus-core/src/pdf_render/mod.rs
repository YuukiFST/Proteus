//! PDF page rendering (PRD §9, ledger row 6) — PDF→JPG via pdfium-render.
//!
//! Pdfium is loaded dynamically at runtime; availability is probed through
//! `PROTEUS_PDFIUM_LIB` env or the platform's default search. When the library
//! is absent every operation fails with `PdfRenderUnavailable` — never with a
//! panic — which the adversarial sweep treats as a clean domain error.

use crate::error::ProteusError;
use crate::check_input_size;

use pdfium_render::prelude::*;

/// Bind to PDFium, honouring `PROTEUS_PDFIUM_LIB`.
pub fn bind_pdfium() -> Result<Pdfium, ProteusError> {
    let lib = match std::env::var_os("PROTEUS_PDFIUM_LIB") {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => Pdfium::pdfium_platform_library_name_at_path("."),
    };
    let bindings = Pdfium::bind_to_library(&lib).map_err(|e| {
        ProteusError::PdfRenderUnavailable(format!(
            "cannot load Pdfium from {}: {e}; set PROTEUS_PDFIUM_LIB",
            lib.display()
        ))
    })?;
    Ok(Pdfium::new(bindings))
}

/// Render every page of the PDF to a JPEG at the given DPI.
pub fn render_pdf_pages_to_jpegs(input: &[u8], dpi: u32) -> Result<Vec<Vec<u8>>, ProteusError> {
    check_input_size(input)?;
    if dpi == 0 {
        return Err(ProteusError::InvalidArgument {
            surface: "render_pdf_pages_to_jpegs",
            reason: "DPI must be nonzero".into(),
        });
    }
    let pdfium = bind_pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_vec(input.to_vec(), None)
        .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
    let pages = doc.pages();
    let count = pages.len();
    if count == 0 {
        return Err(ProteusError::MalformedInput("no pages to render".into()));
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let page = pages.get(i).map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        let width = (page.width().value * dpi as f32 / 72.0).round() as i32;
        let height = (page.height().value * dpi as f32 / 72.0).round() as i32;
        let bitmap = page
            .render(width.max(1), height.max(1), None)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        let rgba = bitmap.as_rgba_bytes();
        let w = bitmap.width() as u32;
        let h = bitmap.height() as u32;
        let image = image::RgbaImage::from_raw(w, h, rgba)
            .ok_or_else(|| ProteusError::MalformedInput("pdfium bitmap decode failed".into()))?;
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            
            .encode(image.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .map_err(|e| ProteusError::Image(Box::new(e)))?;
        out.push(jpeg);
    }
    Ok(out)
}

/// Probe-only: Ok if a renderer can be bound.
pub fn is_renderer_available() -> bool {
    bind_pdfium().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::testutil;

    #[test]
    fn unavailable_pdfium_errors_cleanly() {
        // With the env var pointing at a bogus path the error is a domain
        // error, never a panic — and that path tests on every machine.
        std::env::set_var("PROTEUS_PDFIUM_LIB", "/nonexistent/libpdfium.so");
        let result = render_pdf_pages_to_jpegs(&testutil::one_page_pdf("x"), 150);
        std::env::remove_var("PROTEUS_PDFIUM_LIB");
        assert!(
            matches!(result, Err(ProteusError::PdfRenderUnavailable(_))),
            "{result:?}"
        );
    }

    #[test]
    fn zero_dpi_rejected() {
        let err = render_pdf_pages_to_jpegs(&testutil::one_page_pdf("x"), 0).unwrap_err();
        assert!(matches!(err, ProteusError::InvalidArgument { .. }));
    }

    #[test]
    #[ignore = "needs a real PDFium library on the machine (PROTEUS_PDFIUM_LIB)"]
    fn real_pdfium_renders_non_blank_page() {
        let pdf = testutil::blank_pdf(1);
        // Make the page visibly non-blank via a huge black rectangle? The
        // fixture carries text; non-blank assertion is enough.
        let jpegs = render_pdf_pages_to_jpegs(&pdf, 150).unwrap();
        assert_eq!(jpegs.len(), 1);
        let decoded = image::load_from_memory(&jpegs[0]).unwrap().to_rgb8();
        let non_white = decoded
            .pixels()
            .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
            .count();
        assert!(non_white > 0, "rendered page must not be blank");
    }

    #[test]
    #[ignore = "needs a real PDFium binary on the machine"]
    fn real_pdfium_page_count_matches() {
        let pdf = testutil::marker_pdf(&["a", "b", "c"]);
        let jpegs = render_pdf_pages_to_jpegs(&pdf, 72).unwrap();
        assert_eq!(jpegs.len(), 3);
    }
}