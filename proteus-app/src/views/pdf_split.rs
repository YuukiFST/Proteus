//! Split PDF (PRD §9): extract page ranges into separate documents.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::*;
use gpui_component::input::InputState;
use proteus_core::pdf::{pdf_page_count, split::PageRange, split_pdf};
use proteus_core::error::ProteusError;

use super::common::{self, ToolState};

pub struct SplitPdfView {
    state: ToolState,
    this: Entity<Self>,
    ranges: Entity<InputState>,
}

/// Parse a spec like "1-3,5" or "1 3 5" (spaces, commas and newlines separate
/// ranges; "N" means the single page N). Pure UI glue — validation happens in
/// proteus-core via PageRange::validate.
fn parse_ranges(spec: &str) -> Result<Vec<PageRange>, ProteusError> {
    let mut ranges = Vec::new();
    for token in spec.split([' ', '\t', '\n', ',']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let (a, b) = match token.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (token, token),
        };
        let start: u32 = a.parse().map_err(|_| ProteusError::InvalidArgument {
            surface: "split_pdf",
            reason: format!("'{token}' is not a valid page range"),
        })?;
        let end: u32 = b.parse().map_err(|_| ProteusError::InvalidArgument {
            surface: "split_pdf",
            reason: format!("'{token}' is not a valid page range"),
        })?;
        ranges.push(PageRange::new(start, end));
    }
    if ranges.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "split_pdf",
            reason: "enter at least one page range, e.g. 1-3, 5".into(),
        });
    }
    Ok(ranges)
}

impl SplitPdfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: ToolState::default(),
            ranges: common::param_field("e.g. 1-2, 4", "", window, cx),
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

    fn process_action(this: &Entity<Self>, ranges: &Entity<InputState>, cx: &mut App) {
        let input = this.read(cx).state.files.first().map(|(_, b)| b.clone());
        let Some(input) = input else {
            return;
        };
        let spec = common::param_value(cx, ranges);
        let parsed = parse_ranges(&spec);
        this.update(cx, |s, cx| {
            s.state.set_busy("Splitting PDF…");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = (|| -> Result<Vec<Vec<u8>>, ProteusError> {
                let ranges = parsed?;
                let count = pdf_page_count(&input)?;
                for r in &ranges {
                    r.validate(count)?;
                }
                split_pdf(&input, &ranges)
            })();
            this.update(cx, |s, cx| match result {
                Ok(parts) => {
                    let outputs: Vec<(SharedString, Arc<[u8]>)> = parts
                        .into_iter()
                        .enumerate()
                        .map(|(i, b)| {
                            (format!("part-{:03}.pdf", i + 1).into(), Arc::from(b))
                        })
                        .collect();
                    let n = outputs.len();
                    s.state.multi_outputs = outputs;
                    s.state.output_name = None;
                    s.state.set_ok(format!("Done — {n} parts ready. Choose an output folder to write them."));
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
        let outputs = this.read(cx).state.multi_outputs.clone();
        if outputs.is_empty() {
            return;
        }
        let Some(folder) = common::pick_output_folder(window) else {
            return;
        };
        let mut saved = 0usize;
        let mut failed: Option<String> = None;
        for (name, bytes) in &outputs {
            match std::fs::write(folder.join(name.as_ref()), bytes.as_ref()) {
                Ok(()) => saved += 1,
                Err(e) => {
                    failed = Some(format!("{name}: {e}"));
                    break;
                }
            }
        }
        this.update(cx, |s, cx| match failed {
            Some(e) => {
                s.state.set_fail(format!("could not write {}: {e}", folder.display()));
                cx.notify();
            }
            None => {
                s.state.set_ok(format!("Saved {saved} file(s) to {}", folder.display()));
                cx.notify();
            }
        })
    }
}

impl Render for SplitPdfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let this = self.this.clone();
        let busy = self.state.busy;
        let has_input = self.state.has_input();
        let has_output = self.state.has_output();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(div().text_xl().child("Split PDF"))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui_component::gray(400))
                    .child("Extract page ranges into separate PDF files."),
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
                    .w(px(440.))
                    .child(common::param_render(
                        "Page ranges — e.g. 1-3, 5, 7-9".into(),
                        &self.ranges,
                    )),
            )
            .child(
                Button::new("process")
                    .primary()
                    .label("Process")
                    .disabled(busy || !has_input)
                    .on_click({
                        let this = this.clone();
                        let ranges = self.ranges.clone();
                        move |_, _, cx| Self::process_action(&this, &ranges, cx)
                    }),
            )
            .child(
                Button::new("save")
                    .label("Save all to folder…")
                    .disabled(busy || !has_output)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| Self::save_action(&this, window, cx)
                    }),
            )
            .child(common::status_line(&self.state))
    }
}