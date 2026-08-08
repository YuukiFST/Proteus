//! Image resize (PRD §9) — target W×H, optionally keeping aspect ratio.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::image_ops::resize_image;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct ImageResizeView {
    state: ToolState,
    this: Entity<Self>,
    width: Entity<InputState>,
    height: Entity<InputState>,
    keep_ratio: bool,
}

impl ImageResizeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            width: common::param_field("e.g. 1280", "", window, cx),
            height: common::param_field("e.g. 720", "", window, cx),
            keep_ratio: true,
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

    fn process_action(
        this: &Entity<Self>,
        width: &Entity<InputState>,
        height: &Entity<InputState>,
        keep_ratio: bool,
        cx: &mut App,
    ) {
        let (Some(input), w, h) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, width),
            common::param_value(cx, height),
        ) else {
            return;
        };
        let parsed = (|| -> Result<(u32, u32), ProteusError> {
            let width = common::parse_u32("resize_image", &w)?;
            let height = common::parse_u32("resize_image", &h)?;
            if width == 0 || height == 0 {
                return Err(ProteusError::InvalidArgument {
                    surface: "resize_image",
                    reason: "width and height must be nonzero".into(),
                });
            }
            Ok((width, height))
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Resizing…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let (width, height) = parsed?;
                resize_image(&input, width, height, keep_ratio)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("resized".into());
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
        let Some(path) = common::pick_output_file(window, "resized", Filters::Image) else {
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

impl Render for ImageResizeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();
        let keep_ratio = self.keep_ratio;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Resize image"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Scale to a target width/height; optionally preserve aspect ratio."),
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
                h_flex()
                    .gap_4()
                    .child(
                        div()
                            .w(px(180.))
                            .child(common::param_render("Width (px)".into(), &self.width)),
                    )
                    .child(
                        div()
                            .w(px(180.))
                            .child(common::param_render("Height (px)".into(), &self.height)),
                    )
                    .child(
                        Button::new("keep_ratio")
                            .selected(keep_ratio)
                            .label(if keep_ratio { "Keep aspect ratio" } else { "Exact size" })
                            .on_click({
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |s, cx| {
                                        s.keep_ratio = !s.keep_ratio;
                                        cx.notify();
                                    })
                                }
                            }),
                    ),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let width = self.width.clone();
                        let height = self.height.clone();
                        move |_, _, cx| Self::process_action(&this, &width, &height, keep_ratio, cx)
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