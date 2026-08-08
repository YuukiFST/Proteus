//! PDF Unlock (PRD §9, T2 surface) — remove encryption with the correct password.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf_protect::unlock_pdf;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct UnlockPdfView {
    state: ToolState,
    this: Entity<Self>,
    password: Entity<InputState>,
}

impl UnlockPdfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            password: cx.new(|cx| {
                InputState::new(window, cx).placeholder("document password").masked(true)
            }),
            this: cx.entity(),
        }
    }
}

impl UnlockPdfView {
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

    fn process_action(this: &Entity<Self>, password: &Entity<InputState>, cx: &mut App) {
        let (Some(input), pw) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, password),
        ) else {
            return;
        };
        this.update(cx, |s, cx| {
            s.state.set_busy("Unlocking PDF…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                if pw.is_empty() {
                    return Err(ProteusError::InvalidArgument {
                        surface: "unlock_pdf",
                        reason: "a password is required".into(),
                    });
                }
                unlock_pdf(&input, &pw)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("unlocked.pdf".into());
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
        let Some(bytes) = this.read(cx).state.output_bytes.clone() else {
            return;
        };
        let Some(path) = common::pick_output_file(window, "unlocked.pdf", Filters::Pdf) else {
            return;
        };
        match std::fs::write(&path, bytes.as_ref()) {
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

impl Render for UnlockPdfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Unlock PDF"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Remove password protection using the document password."),
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
                    .w(px(360.))
                    .child(common::param_render("Document password".into(), &self.password)),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let password = self.password.clone();
                        move |_, _, cx| Self::process_action(&this, &password, cx)
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