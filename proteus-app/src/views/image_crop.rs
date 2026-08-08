//! Image crop (PRD §9) — normalized x/y/width/height crop, no interactive canvas.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::image_ops::crop_image;
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct ImageCropView {
    state: ToolState,
    this: Entity<Self>,
    x: Entity<InputState>,
    y: Entity<InputState>,
    width: Entity<InputState>,
    height: Entity<InputState>,
}

impl ImageCropView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            x: common::param_field("px from top-left", "0", window, cx),
            y: common::param_field("px from top-left", "0", window, cx),
            width: common::param_field("px", "", window, cx),
            height: common::param_field("px", "", window, cx),
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
        x: &Entity<InputState>,
        y: &Entity<InputState>,
        width: &Entity<InputState>,
        height: &Entity<InputState>,
        cx: &mut App,
    ) {
        let Some(input) = this.read(cx).state.files.first().map(|(_, b)| b.clone()) else {
            return;
        };
        let (x_s, y_s, w_s, h_s) = (
            common::param_value(cx, x),
            common::param_value(cx, y),
            common::param_value(cx, width),
            common::param_value(cx, height),
        );
        let parsed = (|| -> Result<(u32, u32, u32, u32), ProteusError> {
            let x = common::parse_u32("crop_image", &x_s)?;
            let y = common::parse_u32("crop_image", &y_s)?;
            let width = common::parse_u32("crop_image", &w_s)?;
            let height = common::parse_u32("crop_image", &h_s)?;
            if width == 0 || height == 0 {
                return Err(ProteusError::InvalidArgument {
                    surface: "crop_image",
                    reason: "crop width and height must be nonzero".into(),
                });
            }
            Ok((x, y, width, height))
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Cropping…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let (x, y, width, height) = parsed?;
                crop_image(&input, x, y, width, height)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("cropped".into());
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
        let Some(path) = common::pick_output_file(window, "cropped", Filters::Image) else {
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

impl Render for ImageCropView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Crop image"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Crop a rectangle from the top-left origin (pixel coordinates)."),
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
                            .w(px(150.))
                            .child(common::param_render("X".into(), &self.x)),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .child(common::param_render("Y".into(), &self.y)),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .child(common::param_render("Width".into(), &self.width)),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .child(common::param_render("Height".into(), &self.height)),
                    ),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let x = self.x.clone();
                        let y = self.y.clone();
                        let width = self.width.clone();
                        let height = self.height.clone();
                        move |_, _, cx| {
                            Self::process_action(&this, &x, &y, &width, &height, cx)
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