//! Shared glue for tool views (T0 surface, PRD §7).
//!
//! Everything here is UI/glue only: rfd native dialogs (PRD §8 primary input
//! path), in-memory byte transport between view and `proteus-core`, and the
//! shared status widget. Zero business logic — every transformation lives in
//! `proteus-core`.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::*;
use gpui_component::input::{Input, InputState};
use proteus_core::error::ProteusError;

/// I/O state every tool view shares, PRD §8:
/// inputs are read fully into memory, output is produced in memory, and saved
/// to a user-chosen path. `files` holds every loaded input (Merge is the only
/// multi-input tool, but the model tolerates several).
#[derive(Clone)]
pub struct ToolState {
    pub files: Vec<(SharedString, Arc<[u8]>)>,
    pub output_name: Option<SharedString>,
    pub output_bytes: Option<Arc<[u8]>>,
    pub multi_outputs: Vec<(SharedString, Arc<[u8]>)>,
    pub status: SharedString,
    pub busy: bool,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            output_name: None,
            output_bytes: None,
            multi_outputs: Vec::new(),
            status: "No input yet — pick a file with the button above.".into(),
            busy: false,
        }
    }
}

impl ToolState {
    pub fn has_input(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn has_output(&self) -> bool {
        self.output_bytes.is_some() || !self.multi_outputs.is_empty()
    }

    pub fn add_file(&mut self, path: &std::path::Path, bytes: Vec<u8>) {
        let name: SharedString = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        self.files.push((name, Arc::from(bytes)));
        self.reset_output();
        self.status = format!("Loaded {} file(s). Set parameters, then Process.", self.files.len()).into();
    }

    pub fn reset_output(&mut self) {
        self.output_bytes = None;
        self.output_name = None;
        self.multi_outputs.clear();
    }

    pub fn set_busy(&mut self, message: impl Into<SharedString>) {
        self.busy = true;
        self.status = message.into();
    }

    pub fn set_ok(&mut self, message: impl Into<SharedString>) {
        self.busy = false;
        self.status = message.into();
    }

    pub fn set_fail<E: std::fmt::Display>(&mut self, err: E) {
        self.busy = false;
        self.reset_output();
        self.status = format!("Error: {err}").into();
    }

    pub fn error(&self) -> bool {
        self.status.starts_with("Error:")
    }
}

/// Parse a `u32` parameter input, mapping bad input to the core's
/// `InvalidArgument` shape so views surface errors uniformly.
pub fn parse_u32(param: &'static str, value: &str) -> Result<u32, ProteusError> {
    value.trim().parse::<u32>().map_err(|_| ProteusError::InvalidArgument {
        surface: param,
        reason: format!("'{value}' is not a whole number"),
    })
}

pub fn parse_f32(param: &'static str, value: &str) -> Result<f32, ProteusError> {
    value
        .trim()
        .parse::<f32>()
        .map_err(|_| ProteusError::InvalidArgument {
            surface: param,
            reason: format!("'{value}' is not a number"),
        })
}

pub fn parse_i32(param: &'static str, value: &str) -> Result<i32, ProteusError> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|_| ProteusError::InvalidArgument {
            surface: param,
            reason: format!("'{value}' is not a whole number"),
        })
}

// ---------------------------------------------------------------------------
// Native dialogs (PRD §8). rfd is the only dialog provider; the gpui window is
// passed as parent so the native dialog is owned by the app window.
// ---------------------------------------------------------------------------

fn with_pdf_filter(fd: rfd::FileDialog) -> rfd::FileDialog {
    fd.add_filter("PDF", &["pdf"])
}

fn with_image_filter(fd: rfd::FileDialog) -> rfd::FileDialog {
    fd.add_filter("Image", &["png", "jpg", "jpeg", "webp", "avif"])
}

pub fn pick_input_pdf(window: &mut Window) -> Option<PathBuf> {
    with_pdf_filter(rfd::FileDialog::new())
        .set_parent(window)
        .set_title("Choose a PDF")
        .pick_file()
}

pub fn pick_inputs_pdf(window: &mut Window) -> Vec<PathBuf> {
    with_pdf_filter(rfd::FileDialog::new())
        .set_parent(window)
        .set_title("Choose PDFs to merge")
        .pick_files()
        .unwrap_or_default()
}

pub fn pick_input_image(window: &mut Window) -> Option<PathBuf> {
    with_image_filter(rfd::FileDialog::new())
        .set_parent(window)
        .set_title("Choose an image")
        .pick_file()
}

pub fn pick_input_any(window: &mut Window) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_parent(window)
        .set_title("Choose a file")
        .pick_file()
}

/// Native "Save As" — the user chooses the destination explicitly (PRD §8).
pub fn pick_output_file(window: &mut Window, name: &str, filters: Filters) -> Option<PathBuf> {
    let mut fd = rfd::FileDialog::new().set_parent(window).set_file_name(name);
    fd = match filters {
        Filters::Pdf => with_pdf_filter(fd),
        Filters::Image => with_image_filter(fd),
    };
    fd.set_title("Save output as").save_file()
}

/// Multi-file outputs (Split PDF, PDF->JPG) save into a chosen folder.
pub fn pick_output_folder(window: &mut Window) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_parent(window)
        .set_title("Choose an output folder")
        .pick_folder()
}

#[derive(Clone, Copy)]
pub enum Filters {
    Pdf,
    Image,
}

// ---------------------------------------------------------------------------
// Parameter input helpers.
// ---------------------------------------------------------------------------

/// A text field for one parameter, pre-filled with `default`.
pub fn param_field(
    placeholder: &str,
    default: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let default = default.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder.to_string());
        if !default.is_empty() {
            state.set_value(default.clone(), window, cx);
        }
        state
    })
}

pub fn param_value(cx: &App, field: &Entity<InputState>) -> String {
    field.read(cx).value().to_string()
}

pub fn param_render(label: SharedString, input: &Entity<InputState>) -> impl IntoElement {
    v_flex()
        .gap(px(4.))
        .child(div().child(label.clone()).text_xs().text_color(gpui_component::gray(400)))
        .child(Input::new(input))
}

/// Row of small text describing the currently loaded inputs.
pub fn input_summary(state: &ToolState) -> impl IntoElement {
    let (color, text) = if let Some((name, bytes)) = state.files.first() {
        let others = state.files.len() - 1;
        let suffix = if others > 0 {
            format!(" (+{others} more)")
        } else {
            String::new()
        };
        (
            gpui_component::green(500),
            format!(
                "Input: {name}{suffix} ({:.1} MB)",
                bytes.len() as f64 / 1048576.0
            ),
        )
    } else {
        (gpui_component::gray(500), "No input yet".to_string())
    };
    h_flex()
        .text_sm()
        .text_color(color)
        .child(text)
        .child(
            if let Some(name) = &state.output_name {
                div().child(format!("  →  Output: {name}"))
            } else if !state.multi_outputs.is_empty() {
                div().child(format!("  →  {} outputs", state.multi_outputs.len()))
            } else {
                div()
            },
        )
}

pub fn status_line(state: &ToolState) -> impl IntoElement {
    let color = if state.busy {
        gpui_component::blue(500)
    } else if state.error() {
        gpui_component::red(500)
    } else if state.has_output() {
        gpui_component::green(500)
    } else {
        gpui_component::gray(500)
    };
    h_flex()
        .gap_2()
        .child(div().size_2().rounded_full().bg(color))
        .child(div().text_sm().text_color(color).child(state.status.clone()))
}