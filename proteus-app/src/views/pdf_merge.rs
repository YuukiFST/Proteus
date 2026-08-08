//! Merge PDFs (PRD §9) — the only multi-input tool (PRD §3/§8).

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::*;
use proteus_core::pdf::merge_pdfs;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct MergePdfView {
    state: ToolState,
    this: Entity<Self>,
}

impl MergePdfView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            this: cx.entity(),
        }
    }

    fn pick_action(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let paths = common::pick_inputs_pdf(window);
        if paths.is_empty() {
            return; // user cancelled or no selection
        }
        for path in paths {
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
                    });
                    return;
                }
            }
        }
    }

    fn process_action(this: &Entity<Self>, cx: &mut App) {
        let inputs: Vec<Arc<[u8]>> = this
            .read(cx)
            .state
            .files
            .iter()
            .map(|(_, b)| b.clone())
            .collect();
        if inputs.len() < 2 {
            this.update(cx, |s, cx| {
                s.state.set_fail(ProteusError::InvalidArgument {
                    surface: "merge_pdfs",
                    reason: "select at least two PDFs".into(),
                });
                cx.notify();
            });
            return;
        }
        this.update(cx, |s, cx| {
            s.state.set_busy("Merging PDFs…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let refs: Vec<&[u8]> = inputs.iter().map(|b| b.as_ref()).collect();
            let result = merge_pdfs(&refs);
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("merged.pdf".into());
                    s.state.set_ok("Done — press Save to write the merged PDF.");
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
        let output = this.read(cx).state.output_bytes.clone();
        let Some(output) = output else {
            return;
        };
        let Some(path) = common::pick_output_file(window, "merged.pdf", Filters::Pdf) else {
            return;
        };
        match std::fs::write(&path, output.as_ref()) {
            Ok(()) => this
                .update(cx, |s, cx| {
                    s.state
                        .set_ok(format!("Saved to {}", path.display()));
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

impl Render for MergePdfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Merge PDF"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Combine multiple PDFs into one, in selection order (PRD §9)."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("pick")
                            .label("Add PDF files…")
                            .on_click({
                                let this = this.clone();
                                move |_, window, cx| {
                                    Self::pick_action(&this, window, cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("clear")
                            .ghost()
                            .label("Clear")
                            .on_click({
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |s, cx| {
                                        s.state = ToolState::default();
                                        cx.notify();
                                    })
                                }
                            }),
                    ),
            )
            .child(common::input_summary(&self.state))
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        move |_, _, cx| Self::process_action(&this, cx)
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