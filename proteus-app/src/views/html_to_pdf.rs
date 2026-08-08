//! HTML→PDF (PRD §9) — render an HTML file to PDF bytes.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf::html_to_pdf::{html_to_pdf_bytes, HtmlToPdfOptions};
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct HtmlToPdfView {
    state: ToolState,
    this: Entity<Self>,
    font_size: Entity<InputState>,
    margin: Entity<InputState>,
    line_height: Entity<InputState>,
}

impl HtmlToPdfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let defaults = HtmlToPdfOptions::default();
        Self {
            state: ToolState::default(),
            font_size: common::param_field(
                "11",
                defaults.font_size.to_string(),
                window,
                cx,
            ),
            margin: common::param_field("56", defaults.margin.to_string(), window, cx),
            line_height: common::param_field(
                "1.4",
                defaults.line_height.to_string(),
                window,
                cx,
            ),
            this: cx.entity(),
        }
    }

    fn pick_action(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let Some(path) = common::pick_input_any(window) else {
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

    fn process_action(
        this: &Entity<Self>,
        font_size: &Entity<InputState>,
        margin: &Entity<InputState>,
        line_height: &Entity<InputState>,
        cx: &mut App,
    ) {
        let (Some(input), fs, m, lh) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, font_size),
            common::param_value(cx, margin),
            common::param_value(cx, line_height),
        ) else {
            return;
        };
        let parsed = (|| -> Result<HtmlToPdfOptions, ProteusError> {
            let font_size = common::parse_f32("html_to_pdf", &fs)?;
            let margin = common::parse_f32("html_to_pdf", &m)?;
            let line_height = common::parse_f32("html_to_pdf", &lh)?;
            Ok(HtmlToPdfOptions {
                title: None,
                font_size,
                margin,
                line_height,
            })
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Converting HTML to PDF…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let options = parsed?;
                html_to_pdf_bytes(&input, &options)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("document.pdf".into());
                    s.state.set_ok("Done — press Save to write the result.");
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
        let Some(output) = this.read(cx).state.output_bytes.clone() else {
            return;
        };
        let Some(path) = common::pick_output_file(window, "document.pdf", Filters::Pdf) else {
            return;
        };
        match std::fs::write(&path, output.as_ref()) {
            Ok(()) => this
                .update(cx, |s, cx| {
                    s.state.set_ok(format!("Saved to {}", path.display()));
                    cx.notify();
                }),
            Err(e) => {
                this.update(cx, |s, cx| {
                    s.state
                        .set_fail(format!("could not write {}: {e}", path.display()));
                    cx.notify();
                })
            }
        }
    }
}

impl Render for HtmlToPdfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("HTML → PDF"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Convert an HTML file to a PDF with embedded fonts, fully offline."),
            )
            .child(
                Button::new("pick")
                    .label("Open HTML file…")
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| Self::pick_action(&this, window, cx)
                    }),
            )
            .child(common::input_summary(&self.state))
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        div()
                            .w(px(170.))
                            .child(common::param_render("Font size (pt)".into(), &self.font_size)),
                    )
                    .child(
                        div()
                            .w(px(170.))
                            .child(common::param_render("Margin (pt)".into(), &self.margin)),
                    )
                    .child(
                        div()
                            .w(px(180.))
                            .child(common::param_render("Line height".into(), &self.line_height)),
                    ),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let fs = self.font_size.clone();
                        let m = self.margin.clone();
                        let lh = self.line_height.clone();
                        move |_, _, cx| Self::process_action(&this, &fs, &m, &lh, cx)
                    }),
            )
            .child(
                Button::new("save")
                    .label("Save output…")
                    .disabled(busy || !has_output)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| Self::save_action(&this, window, cx)
                    }),
            )
            .child(common::status_line(&self.state))
    }
}