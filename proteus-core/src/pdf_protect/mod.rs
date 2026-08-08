//! PDF protect / unlock (PRD §9; T2 surface — password ops).
//!
//! Two engines behind one contract:
//! - **InMemoryProtectEngine** (default): lopdf's own AES-256 (PDF 2.0 R6)
//!   encryption, fully in-memory (PRD §8). The PRD §5 text ("lopdf can only
//!   decrypt, it cannot create protection") predates lopdf ≥0.44's encryption
//!   support; this removes the pdfk subprocess from the default path entirely.
//! - **PdfkCliEngine**: the PRD-chosen external CLI (`pdfk`), kept as a
//!   pluggable engine, exercised by fake-CLI protocol tests + a real-binary
//!   integration test that self-skips when the binary is absent.
//!
//! Mutation gate (PRD §7): validation + engine dispatch logic live here so
//! cargo-mutants has reachable, killable decisions; the AES state itself is
//! downstream of lopdf.

mod pdfk;

use std::collections::BTreeMap;
use std::sync::Arc;

use rand::RngExt as _;
use crate::error::ProteusError;
use crate::pdf::{open_pdf, open_pdf_with_password, save_pdf};
use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{EncryptionState, EncryptionVersion};

/// The behavior both engines implement.
pub trait ProtectEngine: Send + Sync {
    fn protect(&self, input: &[u8], user_password: &str, owner_password: &str)
        -> Result<Vec<u8>, ProteusError>;
    fn unlock(&self, input: &[u8], password: &str) -> Result<Vec<u8>, ProteusError>;
}

/// Password input rules, applied by every engine before any work happens.
fn validate_passwords(user: &str, owner: &str) -> Result<(), ProteusError> {
    if user.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "protect_pdf",
            reason: "user password may not be empty".into(),
        });
    }
    if owner.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "protect_pdf",
            reason: "owner password may not be empty".into(),
        });
    }
    if user.len() > 127 || owner.len() > 127 {
        return Err(ProteusError::InvalidArgument {
            surface: "protect_pdf",
            reason: "PDF 2.0 passwords are limited to 127 bytes".into(),
        });
    }
    if user.contains('\u{0}') || owner.contains('\u{0}') {
        return Err(ProteusError::InvalidArgument {
            surface: "protect_pdf",
            reason: "passwords may not contain NUL bytes".into(),
        });
    }
    Ok(())
}

/// In-memory AES-256 (R6) engine via lopdf's own crypto.
pub struct InMemoryProtectEngine;

impl ProtectEngine for InMemoryProtectEngine {
    fn protect(
        &self,
        input: &[u8],
        user_password: &str,
        owner_password: &str,
    ) -> Result<Vec<u8>, ProteusError> {
        let mut doc = open_pdf(input)?;
        let mut file_key = [0u8; 32];
        rand::rng().fill(&mut file_key);
        let mut crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>> = BTreeMap::new();
        crypt_filters.insert(b"StdCF".to_vec(), Arc::new(Aes256CryptFilter));
        let version = EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters,
            file_encryption_key: &file_key,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password,
            user_password,
            permissions: lopdf::Permissions::all(),
        };
        let state = EncryptionState::try_from(version)
            .map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        doc.encrypt(&state).map_err(|e| ProteusError::Pdf(Box::new(e)))?;
        save_pdf(&mut doc)
    }

    fn unlock(&self, input: &[u8], password: &str) -> Result<Vec<u8>, ProteusError> {
        let mut doc = open_pdf_with_password(input, password)?;
        // erase the encryption and reserialize with plaintext streams
        doc.trailer.remove(b"Encrypt");
        doc.encryption_state = None;
        // lopdf remembers crypt filters on decrypted objects? They were already
        // decrypted in memory; saving without the Encrypt dict yields a clean file.
        save_pdf(&mut doc)
    }
}

/// Runtime selection: prefers the in-memory engine; pdfk is opt-in via env.
fn default_engine() -> Box<dyn ProtectEngine> {
    match pdfk::PdfkCliProtect::locate() {
        Some(cli) => Box::new(cli),
        None => Box::new(InMemoryProtectEngine),
    }
}

/// Protect a PDF with a user password (+ optional owner password).
pub fn protect_pdf(
    input: &[u8],
    user_password: &str,
    owner_password: Option<&str>,
) -> Result<Vec<u8>, ProteusError> {
    let owner = owner_password.unwrap_or(user_password);
    validate_passwords(user_password, owner)?;
    default_engine().protect(input, user_password, owner)
}

/// Remove encryption from a PDF using its password.
pub fn unlock_pdf(input: &[u8], password: &str) -> Result<Vec<u8>, ProteusError> {
    if password.is_empty() {
        return Err(ProteusError::InvalidArgument {
            surface: "unlock_pdf",
            reason: "password may not be empty".into(),
        });
    }
    default_engine().unlock(input, password)
}

/// Produce the InMemory engine explicitly placed in the default-locked path.
pub fn in_memory_engine() -> Box<dyn ProtectEngine> {
    Box::new(InMemoryProtectEngine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{extract_pdf_text, pdf_page_count, testutil};
    use proptest::prelude::*;

    #[test]
    fn passwords_fail_individually_on_length_and_nuls() {
        // Each bad-password class must be rejected on its own side too: the
        // `||`-style gates must not require BOTH passwords to be bad.
        let pdf = testutil::one_page_pdf("mark");
        let long: String = "x".repeat(128);
        let long_ref: &str = &long;
        // 127 bytes is the legal maximum (PDF 2.0): accepted end to end, so a
        // `>=` regression on the 127 limit is caught on the boundary itself.
        let boundary: String = "y".repeat(127);
        let ok = protect_pdf(&pdf, &boundary, Some("owner")).unwrap();
        // The 127-byte password must survive the crypto pipeline end to end.
        let doc = crate::pdf::open_pdf_with_password(&ok, &boundary).unwrap();
        assert_eq!(crate::pdf::page_ids(&doc).len(), 1);
        // Same boundary on the OWNER side: 127-byte owner is legal too.
        let ok = protect_pdf(&pdf, "user", Some(&boundary)).unwrap();
        let doc = crate::pdf::open_pdf_with_password(&ok, "user").unwrap();
        assert_eq!(crate::pdf::page_ids(&doc).len(), 1);
        // owner defaults to the user password when None is passed, so a
        // bad OWNER only becomes observable when it is passed explicitly.
        for (user, owner, user_side_bad) in [
            (long_ref, "owner", true),
            ("user", long_ref, false),
            ("bad\0bad", "owner", true),
            ("user", "bad\0owner", false),
        ] {
            if user_side_bad {
                let err = protect_pdf(&pdf, user, None).unwrap_err();
                assert!(matches!(err, ProteusError::InvalidArgument { .. }), "{err:?}");
            }
            let err = protect_pdf(&pdf, user, Some(owner)).unwrap_err();
            assert!(matches!(err, ProteusError::InvalidArgument { .. }), "{err:?}");
        }
    }

fn lock_and_unlock_roundtrip(engine: &dyn ProtectEngine, password: &str) {
        let pdf = testutil::marker_pdf(&["alpha", "beta"]);
        let locked = engine.protect(&pdf, password, password).unwrap();
        // must be encrypted: opening without a password yields an encrypted
        // document (lopdf does not validate ciphertext at load time)
        let doc = crate::pdf::open_pdf(&locked).expect("encrypted file parses");
        assert!(doc.is_encrypted(), "locked file must be marked encrypted");
        // loads and decrypts with the password
        let doc = crate::pdf::open_pdf_with_password(&locked, password).unwrap();
        assert_eq!(crate::pdf::page_ids(&doc).len(), 2);
        // wrong password is rejected
        let err = crate::pdf::open_pdf_with_password(&locked, "wrong-pw").unwrap_err();
        assert!(matches!(err, ProteusError::WrongPassword));
        let unlocked = engine.unlock(&locked, password).unwrap();
        // Opens without any password now, content preserved.
        let reopened = crate::pdf::open_pdf(&unlocked).unwrap();
        assert!(!reopened.is_encrypted(), "unlocked file must not be encrypted");
        assert_eq!(crate::pdf::page_ids(&reopened).len(), 2);
        assert_eq!(pdf_page_count(&unlocked).unwrap(), 2);
        let text = extract_pdf_text(&unlocked).unwrap();
        assert!(text.contains("alpha") && text.contains("beta"));
    }

    #[test]
    fn in_memory_engine_protect_unlock_roundtrip() {
        lock_and_unlock_roundtrip(&InMemoryProtectEngine, "hunter2")
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]
        /// Round-trip for arbitrary (printable) passwords.
        #[test]
        fn roundtrip_arbitrary_password(pw in "[a-zA-Z0-9@!#$%^&*()_+-=~]{1,32}") {
            lock_and_unlock_roundtrip(&InMemoryProtectEngine, &pw);
        }
    }

    #[test]
    fn protect_rejects_empty_passwords() {
        let pdf = testutil::one_page_pdf("x");
        assert!(matches!(
            protect_pdf(&pdf, "", None).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
        assert!(matches!(
            protect_pdf(&pdf, "ok", Some("")).unwrap_err(),
            ProteusError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn protect_rejects_overlong_passwords() {
        let pdf = testutil::one_page_pdf("x");
        let long = "x".repeat(128);
        let err = protect_pdf(&pdf, &long, None).unwrap_err();
        assert!(matches!(err, ProteusError::InvalidArgument { .. }));
    }

    #[test]
    fn unlock_of_plain_document_is_not_encrypted_error() {
        let pdf = testutil::one_page_pdf("x");
        let err = unlock_pdf(&pdf, "pw").unwrap_err();
        assert!(matches!(err, ProteusError::NotEncrypted));
    }

    #[test]
    fn unlock_with_wrong_password_is_wrong_password() {
        let pdf = testutil::one_page_pdf("x");
        let locked = protect_pdf(&pdf, "right", None).unwrap();
        let err = unlock_pdf(&locked, "wrong").unwrap_err();
        assert!(matches!(err, ProteusError::WrongPassword), "{err:?}");
    }

    #[test]
    fn protecting_an_encrypted_pdf_is_rejected() {
        let pdf = testutil::one_page_pdf("x");
        let once = protect_pdf(&pdf, "a", None).unwrap();
        let err = protect_pdf(&once, "b", None).unwrap_err();
        // already locked → open fails as malformed (cannot be parsed w/o pw)
        assert!(err.to_string().contains("PDF"), "{err:?}");
    }

    #[test]
    fn active_engine_roundtrip_via_env_selection() {
        // `default_engine()` prefers the pdfk CLI when PROTEUS_PDFK_BINARY is
        // set; with nothing set it degrades to the in-memory engine.
        let engine = default_engine();
        lock_and_unlock_roundtrip(&*engine, "envtest");
    }

    #[test]
    fn pdfk_engine_via_fake_cli_is_hermetic() {
        let fake = pdfk::tests::fake_pdfk_script();
        let cli = pdfk::PdfkCliProtect { binary: fake };
        let pdf = testutil::one_page_pdf("mark");
        let locked = cli.protect(&pdf, "right", "right").unwrap();
        assert_eq!(locked, pdf, "byte passthrough");
        let unlocked = cli.unlock(&locked, "right").unwrap();
        assert_eq!(unlocked, pdf, "round-trip");
    }
}