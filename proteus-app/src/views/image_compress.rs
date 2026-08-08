//! Image compress (PRD §9) — re-encode with a target quality.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::image_ops::compress_image;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct ImageCompressView {
    state: ToolState,
    this: Entity<Self>,
    quality: Entity<InputState>,
}

impl ImageCompressView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            quality: common::param_field("65", "65", window, cx),
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

    fn process_action(this: &Entity<Self>, quality: &Entity<InputState>, cx: &mut App) {
        let (Some(input), q) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, quality),
        ) else {
            return;
        };
        let parsed = (|| -> Result<Option<u8>, ProteusError> {
            if q.trim().is_empty() {
                return Ok(None);
            }
            let v = common::parse_u32("compress_image", &q)?;
            if !(1..=100).contains(&v) {
                return Err(ProteusError::InvalidArgument {
                    surface: "compress_image",
                    reason: format!("quality must be within 1..=100, got {v}"),
                });
            }
            Ok(Some(v as u8))
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Compressing…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let quality = parsed?;
                compress_image(&input, quality)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("compressed-image".into());
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
        let Some(path) = common::pick_output_file(window, "compressed-image", Filters::Image) else {
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

impl Render for ImageCompressView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Compress image"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Re-encode an image with a target quality (JPG/WebP/AVIF)."),
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
                    .w(px(200.))
                    .child(common::param_render("Quality (1..100)".into(), &self.quality)),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let quality = self.quality.clone();
                        move |_, _, cx| Self::process_action(&this, &quality, cx)
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