//! pdfk external-CLI engine (PRD §5) behind the `ProtectEngine` contract.
//!
//! Offline single binary; invoked with argv (no shell), passwords never on the
//! command line in an unquoted form, temp files cleaned up with RAII.
//! Detection order: `PROTEUS_PDFK_BINARY` env var, then `pdfk` on PATH.

use std::path::PathBuf;
use std::process::Command;

use crate::error::ProteusError;
use crate::pdf_protect::ProtectEngine;

/// Wrapper around the pdfk CLI.
pub struct PdfkCliProtect {
    pub binary: PathBuf,
}

impl PdfkCliProtect {
    /// Locate a usable pdfk binary, or None.
    pub fn locate() -> Option<Self> {
        if let Some(path) = std::env::var_os("PROTEUS_PDFK_BINARY") {
            if !path.is_empty() {
                return Some(PdfkCliProtect { binary: PathBuf::from(path) });
            }
        }
        // PATH search without running anything.
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("pdfk");
            if candidate.is_file() {
                return Some(PdfkCliProtect { binary: candidate });
            }
        }
        None
    }
}

struct TempFile {
    path: PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn unique_temp_path(suffix: &str) -> Result<TempFile, ProteusError> {
    let name = format!(
        "proteus-pdfk-{}-{}-{suffix}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    );
    let mut path = std::env::temp_dir();
    path.push(name);
    Ok(TempFile { path })
}

/// Run pdfk; exits non-zero map to a domain error (the `check` gate below
/// distinguishes wrong passwords from hard failures).
fn run_pdfk(binary: &PathBuf, args: &[&str]) -> Result<(), ProteusError> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(ProteusError::Io)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = if stderr.trim().is_empty() {
        "pdfk exited with an error".to_string()
    } else {
        stderr.trim().to_string()
    };
    Err(ProteusError::Pdf(Box::new(std::io::Error::other(format!("pdfk failed: {detail}")))))
}

impl ProtectEngine for PdfkCliProtect {
    fn protect(
        &self,
        input: &[u8],
        user_password: &str,
        owner_password: &str,
    ) -> Result<Vec<u8>, ProteusError> {
        if self.binary.as_os_str().is_empty() {
            return Err(ProteusError::InvalidArgument {
                surface: "pdfk_protect",
                reason: "pdfk binary path is empty".into(),
            });
        }
        let in_file = unique_temp_path("in.pdf")?;
        let out_file = unique_temp_path("out.pdf")?;
        std::fs::write(&in_file.path, input).map_err(ProteusError::Io)?;

        let mut args = vec!["lock", in_file.path.to_str().unwrap()];
        if owner_password == user_password {
            args.extend(["--password", user_password]);
        } else {
            args.extend(["--user-password", user_password, "--owner-password", owner_password]);
        }
        args.extend(["--output", out_file.path.to_str().unwrap()]);
        run_pdfk(&self.binary, &args)?;

        let out = std::fs::read(&out_file.path).map_err(ProteusError::Io)?;
        Ok(out)
    }

    fn unlock(&self, input: &[u8], password: &str) -> Result<Vec<u8>, ProteusError> {
        let in_file = unique_temp_path("in.pdf")?;
        let out_file = unique_temp_path("out.pdf")?;
        std::fs::write(&in_file.path, input).map_err(ProteusError::Io)?;
        // password gate first via `check` (exit 0 = correct) — so wrong
        // passwords map to WrongPassword, not a generic failure.
        if run_pdfk(&self.binary, &["check", in_file.path.to_str().unwrap(), "--password", password]).is_err() {
            // Distinguish "not encrypted" from "wrong password": try open.
            if crate::pdf::open_pdf(input).is_ok() {
                return Err(ProteusError::NotEncrypted);
            }
            return Err(ProteusError::WrongPassword);
        }
        let args = ["unlock", in_file.path.to_str().unwrap(), "--password", password, "--output", out_file.path.to_str().unwrap()];
        run_pdfk(&self.binary, &args)?;
        let out = std::fs::read(&out_file.path).map_err(ProteusError::Io)?;
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    /// A hermetic stand-in for the pdfk CLI: the protocol the wrapper speaks.
    /// `lock`/`unlock` copy in→out; `check` succeeds only for the password
    /// "right". Written as a real script so the wrapper's subprocess path is
    /// exercised end to end.
    pub fn fake_pdfk_script() -> PathBuf {
        // Unique directory per call: tests run in parallel and a shared script
        // path would get rewritten while another test executes it (ETXTBSY).
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "proteus-fake-pdfk-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pdfk");
        let script = r#"#!/bin/sh
cmd="$1"; shift
in=""; out=""; pw=""
while [ $# -gt 0 ]; do
  case "$1" in
    --password) pw="$2"; shift 2 ;;
    --output) out="$2"; shift 2 ;;
    --user-password|--owner-password)
      # Contract pin: when user==owner the wrapper must send a single
      # --password flag, never the pair form (a regression here means two
      # passwords were smuggled onto the command line — reject loudly).
      exit 3 ;;
    *) in="$1"; shift ;;
  esac
done
if [ "$cmd" = "check" ]; then
  [ "$pw" = "right" ] || exit 1
  exit 0
fi
[ -n "$in" ] || exit 2
[ -n "$out" ] || exit 2
cp "$in" "$out"
exit 0
"#;
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn locate_finds_binary_via_env() {
        // env mutation is global; serialize it against the other env test.
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let fake = fake_pdfk_script();
        std::env::set_var("PROTEUS_PDFK_BINARY", &fake);
        let found = PdfkCliProtect::locate();
        std::env::remove_var("PROTEUS_PDFK_BINARY");
        assert!(found.is_some(), "env-var lookup must find the fake");
    }

    #[test]
    fn fake_cli_roundtrip_passes_bytes_through() {
        let cli = PdfkCliProtect { binary: fake_pdfk_script() };
        let pdf = crate::pdf::testutil::one_page_pdf("mark");
        // The fake performs byte copies: protect/unlock must not mangle data
        // (argv + temp-file plumbing oracle; real crypto is pdfk's own).
        let locked = cli.protect(&pdf, "right", "right").unwrap();
        assert_eq!(locked, pdf);
        let unlocked = cli.unlock(&locked, "right").unwrap();
        assert_eq!(unlocked, pdf, "fake lock/unlock must round-trip bytes");
    }

    #[test]
    fn fake_cli_wrong_password_on_plain_file_is_not_encrypted() {
        let cli = PdfkCliProtect { binary: fake_pdfk_script() };
        let pdf = crate::pdf::testutil::one_page_pdf("mark");
        // The fake cannot encrypt; the wrapper's check-gate maps the failure
        // onto the document's actual state (plain file → NotEncrypted).
        let locked = cli.protect(&pdf, "right", "right").unwrap();
        let err = cli.unlock(&locked, "wrong").unwrap_err();
        assert!(matches!(err, ProteusError::NotEncrypted), "{err:?}");
    }

    #[test]
    fn empty_binary_path_rejected() {
        let cli = PdfkCliProtect { binary: PathBuf::new() };
        let pdf = crate::pdf::testutil::one_page_pdf("mark");
        let err = cli.protect(&pdf, "a", "a").unwrap_err();
        assert!(matches!(err, ProteusError::InvalidArgument { .. }));
    }

    #[test]
    fn temp_file_is_removed_on_drop() {
        let tmp = unique_temp_path("drop-me.pdf").unwrap();
        std::fs::write(&tmp.path, b"x").unwrap();
        let path = tmp.path.clone();
        assert!(path.is_file());
        drop(tmp);
        assert!(
            !path.exists(),
            "TempFile drop must delete the scratch file (secrets must not linger)"
        );
    }

    #[test]
    fn cli_failure_yields_domain_error() {
        // A fake pdfk that *always* exits non-zero: the wrapper must surface a
        // Pdf domain error instead of panicking or returning Ok with a bogus
        // empty output (covers the `==`/`!=` on status.success in protect).
        let dir = std::env::temp_dir().join(format!(
            "proteus-failing-pdfk-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pdfk");
        std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cli = PdfkCliProtect { binary: path };
        let pdf = crate::pdf::testutil::one_page_pdf("mark");
        let err = cli.protect(&pdf, "a", "a").unwrap_err();
        assert!(matches!(err, ProteusError::Pdf(_)), "{err:?}");

        let err = cli.unlock(&pdf, "a").unwrap_err();
        // check fails first: plain input → NotEncrypted (mapped, not a panic)
        assert!(matches!(err, ProteusError::NotEncrypted | ProteusError::WrongPassword), "{err:?}");
    }
}