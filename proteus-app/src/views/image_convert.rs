//! Image convert (PRD §9) — JPG / PNG / WebP / AVIF.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::*;
use proteus_core::image_ops::{convert_image, ImageFormat};

use super::common::{self, Filters, ToolState};

const FORMATS: [(&str, ImageFormat); 4] = [
    ("JPG", ImageFormat::Jpeg),
    ("PNG", ImageFormat::Png),
    ("WebP", ImageFormat::WebP),
    ("AVIF", ImageFormat::Avif),
];

pub struct ImageConvertView {
    state: ToolState,
    this: Entity<Self>,
    target: ImageFormat,
}

impl ImageConvertView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            target: ImageFormat::Png,
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

    fn process_action(this: &Entity<Self>, target: ImageFormat, cx: &mut App) {
        let Some(input) = this.read(cx).state.files.first().map(|(_, b)| b.clone()) else {
            return;
        };
        this.update(cx, |s, cx| {
            s.state.set_busy("Converting…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = convert_image(&input, target);
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some(format!("converted.{}", target.as_str()).into());
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
        let Some(path) = common::pick_output_file(window, "converted", Filters::Image) else {
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

impl Render for ImageConvertView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();
        let target = self.target;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Convert image"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Convert to JPG, PNG, WebP or AVIF."),
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
                    .gap_2()
                    .child(div().text_xs().text_color(gpui_component::gray(400)).child("Target:"))
                    .children(FORMATS.iter().map(|(label, fmt)| {
                        let this = this.clone();
                        let fmt = *fmt;
                        Button::new(*label)
                            .label(*label)
                            .selected(target == fmt)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |s, cx| {
                                    s.target = fmt;
                                    cx.notify();
                                })
                            })
                    })),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        move |_, _, cx| Self::process_action(&this, target, cx)
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