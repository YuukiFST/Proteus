//! Domain error types (PRD §5 — `thiserror` for T1/T2 domain errors).
//! `anyhow` is reserved for T0 glue code in proteus-app.

use thiserror::Error;

/// Domain-level errors. Every operation in `proteus-core` fails with a variant here;
/// causes are chained via `#[source]` so lossless details survive.
#[derive(Debug, Error)]
pub enum ProteusError {
    #[error("PDF error: {0}")]
    Pdf(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("image error: {0}")]
    Image(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("input file exceeds the {limit_mb} MB cap (PRD §8)")]
    InputTooLarge { limit_mb: u64 },

    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),

    #[error("unsupported or corrupted input: {0}")]
    MalformedInput(String),

    #[error("invalid argument (surface: {surface}): {reason}")]
    InvalidArgument { surface: &'static str, reason: String },

    #[error("operation cannot be performed on this document: {0}")]
    NotSupported(String),

    #[error("wrong password for this document")]
    WrongPassword,

    #[error("document is not password-protected")]
    NotEncrypted,

    #[error("PDF page rendering is unavailable: {0}")]
    PdfRenderUnavailable(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_too_large_error_displays_cap_and_reason() {
        let err = ProteusError::InputTooLarge { limit_mb: 500 };
        assert_eq!(
            err.to_string(),
            "input file exceeds the 500 MB cap (PRD §8)",
            "the user-facing error must state the violated rule"
        );
    }

    #[test]
    fn invalid_argument_error_names_surface_and_reason() {
        let err = ProteusError::InvalidArgument {
            surface: "rotate_pdf",
            reason: "angle must be a multiple of 90".into(),
        };
        let text = err.to_string();
        assert!(text.contains("rotate_pdf"), "surface must be named: {text}");
        assert!(
            text.contains("multiple of 90"),
            "the reason must reach the user: {text}"
        );
    }
}