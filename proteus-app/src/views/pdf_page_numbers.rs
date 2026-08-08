//! PDF page numbers (PRD §9) — footer labels, "N" or "N of M".

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::InputState;
use gpui_component::*;
use proteus_core::pdf::{add_page_numbers, page_numbers::PageNumberOptions};
use proteus_core::error::ProteusError;

use super::common::{self, Filters, ToolState};

pub struct PageNumbersView {
    state: ToolState,
    this: Entity<Self>,
    start_at: Entity<InputState>,
    font_size: Entity<InputState>,
    show_total: bool,
}

impl PageNumbersView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            start_at: common::param_field("first printed number", "1", window, cx),
            font_size: common::param_field("e.g. 12", "12", window, cx),
            show_total: false,
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
        start_at: &Entity<InputState>,
        font_size: &Entity<InputState>,
        show_total: bool,
        cx: &mut App,
    ) {
        let (Some(input), start_str, size_str) = (
            this.read(cx).state.files.first().map(|(_, b)| b.clone()),
            common::param_value(cx, start_at),
            common::param_value(cx, font_size),
        ) else {
            return;
        };
        let parsed = (|| -> Result<PageNumberOptions, ProteusError> {
            let start_at = common::parse_u32("add_page_numbers", &start_str)?;
            let font_size = common::parse_f32("add_page_numbers", &size_str)?;
            if !font_size.is_finite() || font_size <= 0.0 {
                return Err(ProteusError::InvalidArgument {
                    surface: "add_page_numbers",
                    reason: format!("font size must be positive, got {font_size}"),
                });
            }
            Ok(PageNumberOptions {
                start_at,
                show_total,
                font_size,
            })
        })();
        this.update(cx, |s, cx| {
            s.state.set_busy("Adding page numbers…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<u8>, ProteusError> {
                let options = parsed?;
                add_page_numbers(&input, &options)
            })();
            this.update(cx, |s, cx| match result {
                Ok(out) => {
                    s.state.output_bytes = Some(Arc::from(out));
                    s.state.output_name = Some("numbered.pdf".into());
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
        let Some(path) = common::pick_output_file(window, "numbered.pdf", Filters::Pdf) else {
            return;
        };
        match std::fs::write(&path, output.as_ref()) {
            Ok(()) => {
                this.update(cx, |s, cx| {
                    s.state.set_ok(format!("Saved to {}", path.display()));
                    cx.notify();
                })
            }
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

impl Render for PageNumbersView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();
        let show_total = self.show_total;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Add page numbers"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Print centered footer numbers on every page."),
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
                            .w(px(180.))
                            .child(common::param_render("Start at".into(), &self.start_at)),
                    )
                    .child(
                        div()
                            .w(px(180.))
                            .child(common::param_render("Font size (pt)".into(), &self.font_size)),
                    )
                    .child(
                        Button::new("show_total")
                            .label("N of M style")
                            .ghost()
                            .on_click({
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |s, cx| {
                                        s.show_total = !s.show_total;
                                        cx.notify();
                                    })
                                }
                            }),
                    )
                    .child(
                        if show_total {
                            div().text_sm().text_color(gpui_component::green(500)).child("N of M")
                        } else {
                            div().text_sm().text_color(gpui_component::gray(500)).child("plain N")
                        },
                    ),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let start = self.start_at.clone();
                        let font = self.font_size.clone();
                        move |_, _, cx| {
                            Self::process_action(&this, &start, &font, show_total, cx)
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