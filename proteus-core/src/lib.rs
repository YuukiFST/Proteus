//! Proteus core — all business logic for PDF and image operations (PRD §1, §5).
//!
//! Hard constraints:
//! - Zero dependency on `gpui`/`gpui-component` (PRD §6): this crate must build and
//!   test in CI with no display/GPU available.
//! - Zero network calls, ever (PRD §2, §11). No I/O beyond local file bytes.
//! - `cargo test`, `cargo llvm-cov` and `cargo mutants` run against this crate only.
//!
//! Test discipline, tier assignments and per-surface oracles: see `TESTING-LEDGER.md`
//! at the workspace root (PRD §7, prove skill step 7).
#![forbid(unsafe_code)]

pub mod error;
pub mod image_ops;
pub mod pdf;
pub mod pdf_protect;
pub mod pdf_render;

use error::ProteusError;

/// Hard cap per input file, PRD §8: validation happens before any processing.
pub const MAX_INPUT_FILE_BYTES: u64 = 500 * 1024 * 1024;

/// Every entry point that reads user bytes enforces the PRD §8 cap first —
/// the check is size-only, so it cannot be bypassed by cheap headers.
pub fn check_input_size(bytes: &[u8]) -> Result<(), ProteusError> {
    if bytes.len() as u64 > MAX_INPUT_FILE_BYTES {
        return Err(ProteusError::InputTooLarge {
            limit_mb: MAX_INPUT_FILE_BYTES / (1024 * 1024),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_cap_is_exactly_500_mb() {
        // Read through a local: a const-block assert would make mutated constants
        // fail at compile time (uncounted by cargo-mutants) instead of failing the
        // oracle at runtime — the cap must be killable by this test.
        let cap: u64 = MAX_INPUT_FILE_BYTES;
        assert_eq!(cap, 500 * 1024 * 1024);
    }

    #[test]
    fn input_cap_boundary_oracle() {
        // PRD §8: exactly 500 MB passes; anything above is outside the cap.
        let cap: u64 = MAX_INPUT_FILE_BYTES;
        let exactly_500_mb: u64 = 500 * 1024 * 1024;
        assert!(cap >= exactly_500_mb);
        assert!(cap < exactly_500_mb + 1);
    }
}