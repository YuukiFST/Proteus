//! PDF → JPG (PRD §9) — rasterize every page with pdfium at a chosen DPI.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf_render::{is_renderer_available, render_pdf_pages_to_jpegs};
use proteus_core::error::ProteusError;

use super::common::{self, ToolState};

pub struct PdfToJpgView {
    state: ToolState,
    this: Entity<Self>,
    dpi: Entity<InputState>,
}

impl PdfToJpgView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            dpi: common::param_field("150", "150", window, cx),
            this: cx.entity(),
        }
    }

    fn pick_action(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let Some(path) = common::pick_input_pdf(window) else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                this.update(cx, |s, cx| {
                    s.state.add_file(&path, bytes);
                    cx.notify();
                })
            }
            Err(e) => {
                this.update(cx, |s, cx| {
                    s.state
                        .set_fail(format!("could not read {}: {e}", path.display()));
                    cx.notify();
                })
            }
        }
    }

    fn process_action(this: &Entity<Self>, dpi: &Entity<InputState>, cx: &mut App) {
        let (Some(input), dpi_str) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, dpi),
        ) else {
            return;
        };
        let parsed = common::parse_u32("pdf_to_jpg", &dpi_str);
        this.update(cx, |s, cx| {
            s.state.set_busy("Rendering pages…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<Vec<u8>>, ProteusError> {
                let dpi = parsed?;
                if dpi == 0 {
                    return Err(ProteusError::InvalidArgument {
                        surface: "pdf_to_jpg",
                        reason: "dpi must be greater than zero".into(),
                    });
                }
                if !is_renderer_available() {
                    return Err(ProteusError::PdfRenderUnavailable(
                        "pdfium renderer not available (set PROTEUS_PDFIUM_LIB to a pdfium shared library)".into(),
                    ));
                }
                render_pdf_pages_to_jpegs(&input, dpi)
            })();
            this.update(cx, |s, cx| match result {
                Ok(images) => {
                    let outputs: Vec<(String, Arc<[u8]>)> = images
                        .into_iter()
                        .enumerate()
                        .map(|(i, b)| (format!("page-{:03}.jpg", i + 1), Arc::from(b)))
                        .collect();
                    let n = outputs.len();
                    s.state.multi_outputs = outputs
                        .into_iter()
                        .map(|(n, b)| (SharedString::from(n), b))
                        .collect();
                    s.state.output_name = None;
                    s.state.set_ok(format!("Done — {n} JPGs ready. Choose an output folder to write them."));
                    cx.notify();
                }
                Err(e) => {
                    s.state.set_fail(e);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn save_action(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let outputs = this.read(cx).state.multi_outputs.clone();
        if outputs.is_empty() {
            return;
        }
        let Some(folder) = common::pick_output_folder(window) else {
            return;
        };
        let mut saved = 0usize;
        let mut failed: Option<String> = None;
        for (name, bytes) in &outputs {
            match std::fs::write(folder.join(name.as_ref()), bytes.as_ref()) {
                Ok(()) => saved += 1,
                Err(e) => {
                    failed = Some(format!("{name}: {e}"));
                    break;
                }
            }
        }
        this.update(cx, |s, cx| match failed {
            Some(e) => {
                s.state
                    .set_fail(format!("could not write {}: {e}", folder.display()));
                cx.notify();
            }
            None => {
                s.state.set_ok(format!("Saved {saved} file(s) to {}", folder.display()));
                cx.notify();
            }
        })
    }
}

impl Render for PdfToJpgView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("PDF → JPG"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Render every page of a PDF to a separate JPG image."),
            )
            .child(
                Button::new("pick")
                    .label("Open PDF…")
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| Self::pick_action(&this, window, cx)
                    }),
            )
            .child(common::input_summary(&self.state))
            .child(
                div()
                    .w(px(200.))
                    .child(common::param_render("DPI (e.g. 150)".into(), &self.dpi)),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let dpi = self.dpi.clone();
                        move |_, _, cx| Self::process_action(&this, &dpi, cx)
                    }),
            )
            .child(
                Button::new("save")
                    .label("Save all to folder…")
                    .disabled(busy || !has_output)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| Self::save_action(&this, window, cx)
                    }),
            )
            .child(common::status_line(&self.state))
    }
}