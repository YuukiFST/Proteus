//! PDF crop margins (PRD §9) — set a CropBox inset on every page.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf::{crop::CropMargins, crop_margins};
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct CropMarginsView {
    state: ToolState,
    this: Entity<Self>,
    left: Entity<InputState>,
    right: Entity<InputState>,
    top: Entity<InputState>,
    bottom: Entity<InputState>,
}

impl CropMarginsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            left: common::param_field("0", "", window, cx),
            right: common::param_field("0", "", window, cx),
            top: common::param_field("0", "", window, cx),
            bottom: common::param_field("0", "", window, cx),
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
        left: &Entity<InputState>,
        right: &Entity<InputState>,
        top: &Entity<InputState>,
        bottom: &Entity<InputState>,
        cx: &mut App,
    ) {
        let (Some(input), l, r, t, b) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, left),
            common::param_value(cx, right),
            common::param_value(cx, top),
            common::param_value(cx, bottom),
        ) else {
            return;
        };
        let parsed = (|| -> Result<CropMargins, ProteusError> {
            Ok(CropMargins::new(
                common::parse_f32("crop_margins", &l)?,
                common::parse_f32("crop_margins", &r)?,
                common::parse_f32("crop_margins", &t)?,
                common::parse_f32("crop_margins", &b)?,
            ))
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Cropping margins…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let margins = parsed?;
                crop_margins(&input, margins)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("cropped.pdf".into());
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
        let Some(path) = common::pick_output_file(window, "cropped.pdf", Filters::Pdf) else {
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

impl Render for CropMarginsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Crop margins"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Remove an equal or asymmetric margin from every page."),
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
                h_flex()
                    .gap_4()
                    .child(
                        div()
                            .w(px(160.))
                            .child(common::param_render("Left (pt)".into(), &self.left)),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .child(common::param_render("Right (pt)".into(), &self.right)),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .child(common::param_render("Top (pt)".into(), &self.top)),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .child(common::param_render("Bottom (pt)".into(), &self.bottom)),
                    ),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let left = self.left.clone();
                        let right = self.right.clone();
                        let top = self.top.clone();
                        let bottom = self.bottom.clone();
                        move |_, _, cx| {
                            Self::process_action(&this, &left, &right, &top, &bottom, cx)
                        }
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