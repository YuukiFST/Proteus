//! PDF Organize/Reorder pages (PRD §9) — reorder via a page list.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf::{pdf_page_count, reorder_pdf};
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct OrganizePdfView {
    state: ToolState,
    this: Entity<Self>,
    order: Entity<InputState>,
}

impl OrganizePdfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            order: common::param_field(
                "e.g. 3,1,2 (or 4 2 1 3)",
                "",
                window,
                cx,
            ),
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

    fn process_action(this: &Entity<Self>, order: &Entity<InputState>, cx: &mut App) {
        let (Some(input), order_str) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, order),
        ) else {
            return;
        };
        let parsed = (|| -> Result<Vec<u32>, ProteusError> {
            let tokens: Vec<&str> = order_str
                .split([' ', '\t', '\n', ','])
                .filter(|t| !t.trim().is_empty())
                .collect();
            if tokens.is_empty() {
                return Err(ProteusError::InvalidArgument {
                    surface: "reorder_pdf",
                    reason: "enter a page order, e.g. 3,1,2".into(),
                });
            }
            tokens
                .iter()
                .map(|t| {
                    t.trim().parse::<u32>().map_err(|_| {
                        ProteusError::InvalidArgument {
                            surface: "reorder_pdf",
                            reason: format!("'{t}' is not a page number"),
                        }
                    })
                })
                .collect()
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Reordering…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let order = parsed?;
                let count = pdf_page_count(&input)?;
                for page in &order {
                    if page < &1 || page > &count {
                        return Err(ProteusError::InvalidArgument {
                            surface: "reorder_pdf",
                            reason: format!(
                                "page {page} is out of range (document has {count} pages)"
                            ),
                        });
                    }
                }
                reorder_pdf(&input, &order)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("reordered.pdf".into());
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
        let Some(path) = common::pick_output_file(window, "reordered.pdf", Filters::Pdf) else {
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

impl Render for OrganizePdfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Organize PDF"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Reorder pages by listing the desired 1-based order."),
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
                    .w(px(440.))
                    .child(common::param_render("New page order".into(), &self.order)),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let order = self.order.clone();
                        move |_, _, cx| Self::process_action(&this, &order, cx)
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