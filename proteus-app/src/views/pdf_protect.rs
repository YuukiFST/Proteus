//! PDF Protect (PRD §9, T2 surface) — add a user/owner password (AES-256 R6).

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf_protect::protect_pdf;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct ProtectPdfView {
    state: ToolState,
    this: Entity<Self>,
    user_password: Entity<InputState>,
    owner_password: Entity<InputState>,
}

impl ProtectPdfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (up, op) = {
            let user = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("required")
                    .masked(true)
            });
            let owner = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("optional (defaults to user password)")
                    .masked(true)
            });
            (user, owner)
        };
        Self {
            state: ToolState::default(),
            user_password: up,
            owner_password: op,
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

    fn process_action(
        this: &Entity<Self>,
        user: &Entity<InputState>,
        owner: &Entity<InputState>,
        cx: &mut App,
    ) {
        let (Some(input), user_pw, owner_pw) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, user),
            common::param_value(cx, owner),
        ) else {
            return;
        };
        this.update(cx, |s, cx| {
            s.state.set_busy("Protecting PDF…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                if user_pw.is_empty() {
                    return Err(ProteusError::InvalidArgument {
                        surface: "protect_pdf",
                        reason: "a password is required to protect the PDF".into(),
                    });
                }
                let owner = if owner_pw.is_empty() {
                    None
                } else {
                    Some(owner_pw.as_str())
                };
                protect_pdf(&input, &user_pw, owner)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("protected.pdf".into());
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
        let Some(path) = common::pick_output_file(window, "protected.pdf", Filters::Pdf) else {
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

impl Render for ProtectPdfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Protect PDF"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Encrypt with a password (AES-256, PDF 2.0/R6). Owner password is optional."),
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
                v_flex()
                    .gap_3()
                    .child(div().w(px(360.)).child(common::param_render(
                        "User password".into(),
                        &self.user_password,
                    )))
                    .child(div().w(px(360.)).child(common::param_render(
                        "Owner password".into(),
                        &self.owner_password,
                    ))),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let user = self.user_password.clone();
                        let owner = self.owner_password.clone();
                        move |_, _, cx| Self::process_action(&this, &user, &owner, cx)
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