//! PDF watermark (PRD §9) — text overlay with position/rotation/opacity.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf::{add_watermark, watermark::{WatermarkOptions, WatermarkPosition}};
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct PdfWatermarkView {
    state: ToolState,
    this: Entity<Self>,
    text: Entity<InputState>,
    font_size: Entity<InputState>,
    opacity: Entity<InputState>,
    rotation: Entity<InputState>,
    position: WatermarkPosition,
}

const POSITIONS: [(&str, WatermarkPosition); 5] = [
    ("Center", WatermarkPosition::Center),
    ("Top left", WatermarkPosition::TopLeft),
    ("Top right", WatermarkPosition::TopRight),
    ("Bottom left", WatermarkPosition::BottomLeft),
    ("Bottom right", WatermarkPosition::BottomRight),
];

impl PdfWatermarkView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            text: common::param_field("CONFIDENTIAL", "CONFIDENTIAL", window, cx),
            font_size: common::param_field("48", "48", window, cx),
            opacity: common::param_field("0.25", "0.25", window, cx),
            rotation: common::param_field("0", "0", window, cx),
            position: WatermarkPosition::Center,
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
        text: &Entity<InputState>,
        font_size: &Entity<InputState>,
        opacity: &Entity<InputState>,
        rotation: &Entity<InputState>,
        position: WatermarkPosition,
        cx: &mut App,
    ) {
        let (Some(input), t, fs, op, rot) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, text),
            common::param_value(cx, font_size),
            common::param_value(cx, opacity),
            common::param_value(cx, rotation),
        ) else {
            return;
        };
        let parsed = (|| -> Result<WatermarkOptions, ProteusError> {
            let font_size = common::parse_f32("add_watermark", &fs)?;
            let opacity = common::parse_f32("add_watermark", &op)?;
            let rotation = common::parse_i32("add_watermark", &rot)?;
            if t.trim().is_empty() {
                return Err(ProteusError::InvalidArgument {
                    surface: "add_watermark",
                    reason: "watermark text may not be empty".into(),
                });
            }
            if !font_size.is_finite() || font_size <= 0.0 {
                return Err(ProteusError::InvalidArgument {
                    surface: "add_watermark",
                    reason: format!("font size must be positive, got {font_size}"),
                });
            }
            if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                return Err(ProteusError::InvalidArgument {
                    surface: "add_watermark",
                    reason: format!("opacity must be within 0..=1, got {opacity}"),
                });
            }
            Ok(WatermarkOptions {
                text: t.trim().to_string(),
                font_size,
                opacity,
                position,
                rotation_degrees: rotation as i16,
            })
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Adding watermark…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let options = parsed?;
                add_watermark(&input, &options)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("watermarked.pdf".into());
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
        let Some(path) = common::pick_output_file(window, "watermarked.pdf", Filters::Pdf) else {
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

fn position_rank(pos: WatermarkPosition) -> usize {
    match pos {
        WatermarkPosition::TopLeft => 0,
        WatermarkPosition::TopRight => 1,
        WatermarkPosition::Center => 2,
        WatermarkPosition::BottomLeft => 3,
        WatermarkPosition::BottomRight => 4,
    }
}

impl Render for PdfWatermarkView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();
        let position = self.position;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("PDF watermark"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Stamp semi-transparent text across every page."),
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
                    .child(
                        div()
                            .w(px(360.))
                            .child(common::param_render("Watermark text".into(), &self.text)),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(140.))
                                    .child(common::param_render("Font size (pt)".into(), &self.font_size)),
                            )
                            .child(
                                div()
                                    .w(px(140.))
                                    .child(common::param_render("Opacity 0..1".into(), &self.opacity)),
                            )
                            .child(
                                div()
                                    .w(px(180.))
                                    .child(common::param_render(
                                        "Rotation (mult. of 90)".into(),
                                        &self.rotation,
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_xs().text_color(gpui_component::gray(400)).child("Position:"))
                            .children(POSITIONS.iter().map(|(label, pos)| {
                                let this = this.clone();
                                let pos = *pos;
                                Button::new(("wm_pos", position_rank(pos)))
                                    .label(*label)
                                    .ghost()
                                    .selected(position == pos)
                                    .on_click(move |_, _, cx| {
                                        this.update(cx, |s, cx| {
                                            s.position = pos;
                                            cx.notify();
                                        })
                                    })
                            })),
                    ),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let text = self.text.clone();
                        let font_size = self.font_size.clone();
                        let opacity = self.opacity.clone();
                        let rotation = self.rotation.clone();
                        move |_, _, cx| {
                            Self::process_action(
                                &this, &text, &font_size, &opacity, &rotation, position, cx,
                            )
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