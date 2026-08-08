//! Image rotate (PRD §9) — any angle, multiples of 90 are lossless.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::image_ops::rotate_image;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct ImageRotateView {
    state: ToolState,
    this: Entity<Self>,
    degrees: Entity<InputState>,
}

impl ImageRotateView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            degrees: common::param_field("e.g. 90, -45", "90", window, cx),
            this: cx.entity(),
        }
    }

    fn pick_action(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let Some(path) = common::pick_input_image(window) else {
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

    fn process_action(this: &Entity<Self>, degrees: &Entity<InputState>, cx: &mut App) {
        let (Some(input), deg) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, degrees),
        ) else {
            return;
        };
        let parsed = common::parse_f32("rotate_image", &deg);
        this.update(cx, |s, cx| {
            s.state.set_busy("Rotating…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let degrees = parsed?;
                rotate_image(&input, degrees)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("rotated".into());
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
        let Some(path) = common::pick_output_file(window, "rotated", Filters::Image) else {
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

impl Render for ImageRotateView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Rotate image"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Rotate counter-clockwise by any angle; multiples of 90° are lossless."),
            )
            .child(
                Button::new("pick")
                    .label("Open image…")
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| Self::pick_action(&this, window, cx)
                    }),
            )
            .child(common::input_summary(&self.state))
            .child(
                div()
                    .w(px(220.))
                    .child(common::param_render("Degrees".into(), &self.degrees)),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let degrees = self.degrees.clone();
                        move |_, _, cx| Self::process_action(&this, &degrees, cx)
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