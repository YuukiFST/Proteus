//! HTML→PDF (PRD §9, ledger row 12). The PRD named no library (§13 note: "no
//! library for it, and lopdf can't") — this implementation resolves it as
//! html5ever (via scraper) for spec-compliant lenient parsing + a small layout
//! engine measuring text with ttf_parser against embedded DejaVu fonts, so the
//! output carries REAL embedded fonts and can convert to PDF/A afterwards.
//!
//! Supported subset: h1..h6, p, div, span, strong/b, em/i, ul/ol/li (nested),
//! br, blockquote, table/tr/td (as text flow), a. Script/style are dropped.

use lopdf::{dictionary, Document, FontData, Object, Stream};
use scraper::{ElementRef, Html, Node, Selector};

use crate::error::ProteusError;

const DEJAVU_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const DEJAVU_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
const DEJAVU_OBLIQUE: &[u8] = include_bytes!("../assets/DejaVuSans-Oblique.ttf");
const DEJAVU_BOLD_OBLIQUE: &[u8] = include_bytes!("../assets/DejaVuSans-BoldOblique.ttf");

const PAGE_W: f32 = 595.28; // A4 in points
const PAGE_H: f32 = 841.89;

/// Typography options for the generated document.
#[derive(Debug, Clone)]
pub struct HtmlToPdfOptions {
    pub title: Option<String>,
    /// Base font size in points.
    pub font_size: f32,
    /// Margins on every side, in points.
    pub margin: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
}

impl Default for HtmlToPdfOptions {
    fn default() -> Self {
        HtmlToPdfOptions {
            title: None,
            font_size: 11.0,
            margin: 56.0,
            line_height: 1.4,
        }
    }
}

impl HtmlToPdfOptions {
    fn validate(&self) -> Result<(), ProteusError> {
        if !self.font_size.is_finite() || !(6.0..=96.0).contains(&self.font_size) {
            return Err(ProteusError::InvalidArgument {
                surface: "html_to_pdf",
                reason: format!("font size must be within 6..=96pt, got {}", self.font_size),
            });
        }
        if !self.margin.is_finite() || !(0.0..PAGE_W / 2.0).contains(&self.margin) {
            return Err(ProteusError::InvalidArgument {
                surface: "html_to_pdf",
                reason: format!(
                    "margin must be within 0..{}pt, got {}",
                    PAGE_W / 2.0,
                    self.margin
                ),
            });
        }
        if !self.line_height.is_finite() || !(1.0..=3.0).contains(&self.line_height) {
            return Err(ProteusError::InvalidArgument {
                surface: "html_to_pdf",
                reason: format!("line height must be within 1.0..=3.0, got {}", self.line_height),
            });
        }
        Ok(())
    }
}

/// Convert an HTML document (string) to PDF bytes.
pub fn html_to_pdf(input: &str, options: &HtmlToPdfOptions) -> Result<Vec<u8>, ProteusError> {
    options.validate()?;
    let mut document = parse_document(input, options)?;
    crate::pdf::save_pdf(&mut document)
}

/// Bytes entry point: HTML is decoded as UTF-8 (lossy).
pub fn html_to_pdf_bytes(input: &[u8], options: &HtmlToPdfOptions) -> Result<Vec<u8>, ProteusError> {
    html_to_pdf(&String::from_utf8_lossy(input), options)
}

// ---------------------------------------------------------------------------
// HTML → blocks
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

struct Inline {
    text: String,
    style: Style,
}

struct Paragraph {
    inlines: Vec<Inline>,
    font_size: f32,
    indent: f32,
    spacing_before: f32,
    spacing_after: f32,
}

fn parse_document(html: &str, options: &HtmlToPdfOptions) -> Result<Document, ProteusError> {
    let parsed = Html::parse_document(html);
    let paragraphs = collect_paragraphs(&parsed, options);
    let pages_ops = layout(paragraphs, options);
    build_pdf(pages_ops, options)
}

fn collect_paragraphs(html: &Html, options: &HtmlToPdfOptions) -> Vec<Paragraph> {
    let selector = Selector::parse(
        "h1,h2,h3,h4,h5,h6,p,li,blockquote,div,td",
    )
    .expect("static selector");
    let mut out = Vec::new();
    for element in html.select(&selector) {
        let name = element.value().name();
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let size = match name {
                    "h1" => 24.0,
                    "h2" => 20.0,
                    "h3" => 16.0,
                    "h4" => 14.0,
                    _ => 12.0,
                };
                out.push(Paragraph {
                    inlines: vec![Inline {
                        text: element_text(&element),
                        style: Style::Bold,
                    }],
                    font_size: size,
                    indent: 0.0,
                    spacing_before: size * 0.9,
                    spacing_after: size * 0.5,
                });
            }
            "li" => {
                let (prefix, indent) = list_context(&element);
                let mut para = text_paragraph(&element, prefix, indent, options.font_size);
                if element.ancestors().any(|n| {
                    n.value().as_element()
                        .map(|e| e.name() == "ol" || e.name() == "ul")
                        .unwrap_or(false)
                }) {
                    // nested items keep their own spacing small
                    para.spacing_before = 2.0;
                    para.spacing_after = 2.0;
                }
                out.push(para);
            }
            _ => out.push(text_paragraph(&element, String::new(), 0.0, options.font_size)),
        }
    }
    out
}

fn text_paragraph(el: &ElementRef, prefix: String, indent: f32, font_size: f32) -> Paragraph {
    let mut inlines = collect_inline(el, Style::Regular);
    if !prefix.is_empty() {
        inlines.insert(0, Inline { text: prefix, style: Style::Regular });
    }
    Paragraph {
        inlines,
        font_size,
        indent,
        spacing_before: 4.0,
        spacing_after: 6.0,
    }
}

/// Inline content in document order, honoring strong/em/br and dropping
/// script/style.
fn collect_inline(el: &ElementRef, base: Style) -> Vec<Inline> {
    let mut out = Vec::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => push_inline(&mut out, t.text.to_string(), base),
            Node::Element(_) => {
                let Some(child_el) = ElementRef::wrap(child) else { continue };
                let name = child_el.value().name();
                match name {
                    "script" | "style" => {}
                    "br" => push_inline(&mut out, " ".to_string(), base),
                    "strong" | "b" => out.extend(collect_inline(&child_el, bold_style(base))),
                    "em" | "i" => out.extend(collect_inline(&child_el, italic_style(base))),
                    _ => out.extend(collect_inline(&child_el, base)),
                }
            }
            _ => {}
        }
    }
    out
}

fn push_inline(out: &mut Vec<Inline>, mut text: String, style: Style) {
    // Collapse whitespace runs (HTML rendering semantics), trim edges.
    let mut collapsed = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
            }
            prev_space = true;
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }
    text = collapsed.trim().to_string();
    if !text.is_empty() {
        if let Some(last) = out.last_mut() {
            if last.style == style {
                last.text.push(' ');
                last.text.push_str(&text);
                return;
            }
        }
        out.push(Inline { text, style });
    }
}

fn bold_style(s: Style) -> Style {
    match s {
        Style::Italic | Style::BoldItalic => Style::BoldItalic,
        _ => Style::Bold,
    }
}

fn italic_style(s: Style) -> Style {
    match s {
        Style::Bold | Style::BoldItalic => Style::BoldItalic,
        _ => Style::Italic,
    }
}

/// All descendant text of an element (script/style filtered), collapsed.
fn element_text(el: &ElementRef) -> String {
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(&t.text),
            Node::Element(_) => {
                if let Some(c) = ElementRef::wrap(child) {
                    let name = c.value().name();
                    if name != "script" && name != "style" {
                        out.push_str(&element_text(&c));
                    }
                }
            }
            _ => {}
        }
    }
    collapse(out)
}

fn collapse(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// Bullet "• " or "N. " and indentation by list nesting depth (18pt per level).
fn list_context(el: &ElementRef) -> (String, f32) {
    let depth = el
        .ancestors()
        .filter(|n| {
            n.value().as_element()
                .map(|e| e.name() == "ul" || e.name() == "ol")
                .unwrap_or(false)
        })
        .count();
    let parent_is_ol = el
        .parent()
        .and_then(ElementRef::wrap)
        .map(|e| e.value().name() == "ol")
        .unwrap_or(false);
    let indent = depth.saturating_sub(1) as f32 * 18.0;
    if parent_is_ol {
        let number = el
            .parent()
            .and_then(ElementRef::wrap)
            .map(|parent| {
                parent
                    .children()
                    .filter_map(ElementRef::wrap)
                    .take_while(|sib| sib.id() != el.id())
                    .filter(|sib| sib.value().name() == "li")
                    .count()
            })
            .unwrap_or(0)
            + 1;
        (format!("{number}. "), indent)
    } else {
        ("•  ".to_string(), indent)
    }
}

// ---------------------------------------------------------------------------
// blocks → layout → page ops
// ---------------------------------------------------------------------------

fn faces() -> [ttf_parser::Face<'static>; 4] {
    [
        ttf_parser::Face::parse(DEJAVU_REGULAR, 0).expect("asset font"),
        ttf_parser::Face::parse(DEJAVU_BOLD, 0).expect("asset font"),
        ttf_parser::Face::parse(DEJAVU_OBLIQUE, 0).expect("asset font"),
        ttf_parser::Face::parse(DEJAVU_BOLD_OBLIQUE, 0).expect("asset font"),
    ]
}

fn style_index(s: Style) -> usize {
    match s {
        Style::Regular => 0,
        Style::Bold => 1,
        Style::Italic => 2,
        Style::BoldItalic => 3,
    }
}

fn char_width(f: &ttf_parser::Face, c: char) -> f32 {
    let upem = f.units_per_em().max(1) as f32;
    let units = f
        .glyph_index(c)
        .and_then(|g| f.glyph_hor_advance(g))
        .unwrap_or(0) as f32;
    units / upem
}

fn word_width(faces: &[ttf_parser::Face<'static>; 4], style: Style, word: &str, size: f32) -> f32 {
    let f = &faces[style_index(style)];
    word.chars().map(|c| char_width(f, c)).sum::<f32>() * size
}

fn layout(paragraphs: Vec<Paragraph>, options: &HtmlToPdfOptions) -> Vec<Vec<u8>> {
    let fm = faces();
    let line_h = options.font_size * options.line_height;
    let space_w = options.font_size * 0.30;

    let mut pages: Vec<Vec<u8>> = Vec::new();
    let mut page_ops: Vec<u8> = Vec::new();
    let mut baseline = PAGE_H - options.margin;

    for para in &paragraphs {
        let mut words: Vec<(String, Style)> = Vec::new();
        for inline in &para.inlines {
            for word in inline.text.split(' ') {
                if !word.trim().is_empty() {
                    words.push((word.to_string(), inline.style));
                }
            }
        }
        if words.is_empty() {
            continue;
        }
        baseline -= para.spacing_before;
        let mut x = options.margin + para.indent;
        for (word, style) in &words {
            let w = word_width(&fm, *style, word, para.font_size);
            if x + w > PAGE_W - options.margin && x > options.margin + para.indent {
                baseline -= line_h;
                if baseline - line_h < options.margin {
                    pages.push(std::mem::take(&mut page_ops));
                    baseline = PAGE_H - options.margin;
                }
                x = options.margin + para.indent;
            }
            if baseline - line_h < options.margin {
                pages.push(std::mem::take(&mut page_ops));
                baseline = PAGE_H - options.margin;
            }
            let tag = match style_index(*style) {
                0 => "F1",
                1 => "F2",
                2 => "F3",
                _ => "F4",
            };
            let bytes = crate::pdf::pdf_text_bytes(word);
            page_ops.extend_from_slice(b"BT\n");
            page_ops.extend_from_slice(format!("/{tag} {} Tf\n", para.font_size).as_bytes());
            page_ops.extend_from_slice(format!("1 0 0 1 {} {} Tm\n", x, baseline).as_bytes());
            page_ops.extend_from_slice(b"(");
            page_ops.extend_from_slice(&crate::pdf::escape_pdf_string(&bytes));
            page_ops.extend_from_slice(b") Tj\nET\n");
            x += w + space_w;
        }
        baseline -= para.spacing_after;
    }
    // Always emit at least one page, even for empty input.
    pages.push(page_ops);
    pages
}

fn build_pdf(pages_ops: Vec<Vec<u8>>, options: &HtmlToPdfOptions) -> Result<Document, ProteusError> {
    let mut doc = Document::with_version("1.5");
    let fonts = [
        doc.add_font(FontData::new(DEJAVU_REGULAR, "DejaVuSans".to_string())).map_err(pdf_err)?,
        doc.add_font(FontData::new(DEJAVU_BOLD, "DejaVuSans-Bold".to_string())).map_err(pdf_err)?,
        doc.add_font(FontData::new(DEJAVU_OBLIQUE, "DejaVuSans-Oblique".to_string())).map_err(pdf_err)?,
        doc.add_font(FontData::new(DEJAVU_BOLD_OBLIQUE, "DejaVuSans-BoldOblique".to_string())).map_err(pdf_err)?,
    ];
    if let Some(title) = options.title.as_deref().filter(|t| !t.is_empty()) {
        let info = doc.add_object(dictionary! {
            "Title" => Object::string_literal(crate::pdf::pdf_text_bytes(title)),
            "Creator" => Object::string_literal(b"Proteus"),
        });
        doc.trailer.set(b"Info", info);
    }
    let mut page_objs = Vec::new();
    for ops in pages_ops {
        let content_id = doc.add_object(Stream::new(dictionary! {}, ops));
        let resources = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => fonts[0],
                "F2" => fonts[1],
                "F3" => fonts[2],
                "F4" => fonts[3],
            },
        });
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Contents" => content_id,
            "Resources" => resources,
            "MediaBox" => vec![Object::Real(0.0), Object::Real(0.0), Object::Real(PAGE_W), Object::Real(PAGE_H)],
        });
        page_objs.push(Object::Reference(page));
    }
    let pages_id = doc.new_object_id();
    doc.set_object(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => page_objs.clone(),
            "Count" => page_objs.len() as i64,
        },
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set(b"Root", catalog_id);
    Ok(doc)
}

fn pdf_err(e: lopdf::Error) -> ProteusError {
    ProteusError::Pdf(Box::new(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count};

    #[test]
    fn empty_html_yields_a_valid_single_page() {
        let out = html_to_pdf("", &Default::default()).unwrap();
        assert_eq!(pdf_page_count(&out).unwrap(), 1);
    }

    #[test]
    fn heading_and_paragraph_text_are_extractable() {
        let html = "<h1>Big Title</h1><p>Some body text here.</p>";
        let out = html_to_pdf(html, &Default::default()).unwrap();
        let text = extract_pdf_text(&out).unwrap().replace('\n', " ");
        assert!(text.contains("Big Title"), "{}", text);
        assert!(text.contains("Some body text here."), "{text}");
    }

    #[test]
    fn long_text_flows_to_multiple_pages() {
        let body: String = "lorem ipsum dolor sit amet ".repeat(1200);
        let out = html_to_pdf(&format!("<p>{body}</p>"), &Default::default()).unwrap();
        let n = pdf_page_count(&out).unwrap();
        assert!(n > 1, "expected pagination, got {n} page(s)");
    }

    #[test]
    fn list_items_render_all_items() {
        let html = "<ul><li>first item</li><li>second item</li></ul>";
        let out = html_to_pdf(html, &Default::default()).unwrap();
        let text = extract_pdf_text(&out).unwrap().replace('\n', " ");
        assert!(text.contains("first item"), "{text}");
        assert!(text.contains("second item"), "{text}");
    }

    #[test]
    fn malformed_html_is_lenient_and_never_panics() {
        for bad in [
            "<h1>no close",
            "<p><b>bold</p>",
            "<<<<",
            "</html>",
            "]]>",
            "&#",
            "<script>alert(1)</script>",
        ] {
            let out = html_to_pdf(bad, &Default::default()).unwrap();
            assert!(pdf_page_count(&out).unwrap() >= 1);
        }
    }

    #[test]
    fn script_content_is_not_rendered() {
        let html = "<p>visible<script>hack(1)</script></p>";
        let out = html_to_pdf(html, &Default::default()).unwrap();
        let text = extract_pdf_text(&out).unwrap();
        assert!(text.contains("visible"), "{text}");
        assert!(!text.contains("hack"), "script leaked: {text}");
    }

    #[test]
    fn inline_styles_survive() {
        let html = "<p>Before <strong>BOLD</strong> after <em>italic</em> end.</p>";
        let out = html_to_pdf(html, &Default::default()).unwrap();
        let text = extract_pdf_text(&out).unwrap();
        assert!(text.contains("BOLD") && text.contains("italic"), "{text}");
    }

    #[test]
    fn nested_lists_keep_hierarchy_text() {
        let html = "<ul><li>top<li>nested<ul><li>deep one</li></ul></li></ul>";
        let out = html_to_pdf(html, &Default::default()).unwrap();
        let text = extract_pdf_text(&out).unwrap().replace('\n', " ");
        assert!(text.contains("top") && text.contains("deep one"), "{text}");
    }

    #[test]
    fn invalid_options_rejected() {
        for opts in [
            HtmlToPdfOptions { font_size: 2.0, ..Default::default() },
            HtmlToPdfOptions { margin: 999.0, ..Default::default() },
            HtmlToPdfOptions { line_height: 5.0, ..Default::default() },
        ] {
            let err = html_to_pdf("<p>x</p>", &opts).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }));
        }
    }

    #[test]
    fn title_metadata_is_written() {
        let out = html_to_pdf(
            "<p>x</p>",
            &HtmlToPdfOptions { title: Some("Doc Title".into()), ..Default::default() },
        )
        .unwrap();
        let raw = String::from_utf8_lossy(&out);
        assert!(raw.contains("Doc Title"), "title must appear in Info");
    }
}