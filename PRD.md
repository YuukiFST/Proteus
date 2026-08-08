# PRD — Proteus

## 1. Overview

**Proteus** is a personal, local-first desktop application for PDF and image manipulation — a privacy-respecting alternative to cloud tools like iLovePDF. All processing happens on the user's own machine. No file, metadata, or usage data ever leaves the device.

The project is public on GitHub (MIT license) so anyone can use, audit, or build on it, but it is designed first and foremost for the author's own personal use across two machines: **NixOS** and **Windows 11**.

## 2. Goals

- Provide the most commonly used PDF and image tools (merge, split, compress, convert, protect, etc.) without uploading files anywhere.
- Run as a native, standalone desktop binary — no browser, no local web server, no Node runtime.
- Guarantee zero network calls at runtime. This is a hard product constraint, not a preference.
- Be free to build, run, and distribute (no paid services, no cloud infra, no paid APIs).
- Be verifiably correct via a rigorous testing discipline (see §7), since the app manipulates the user's real files.

## 3. Non-Goals (explicitly out of scope)

Do not implement these, even if iLovePDF/iLoveIMG offer them. Each was deliberately cut during scoping:

- **Video processing** of any kind.
- **PDF ↔ Word / PowerPoint / Excel** conversion (requires LibreOffice headless — rejected to avoid a heavy system dependency).
- **Visual/interactive PDF editing** (Edit PDF, Fill & Sign, form filling with a canvas UI).
- **PDF repair** (corrupted file recovery).
- **Any AI-powered feature**: OCR, summarization, translation, PDF→Markdown, image upscaling, background removal. (Would require local models or a paid API — both rejected.)
- **E-signature, PDF comparison, redaction.**
- **Photo editor / meme generator** for images.
- **Authentication of any kind.** The app is single-user, local-only.
- **A database.** No persistence, no history, no accounts.
- **Telemetry, analytics, crash reporting, or update checks.** Zero outbound network calls, period.
- **Generic multi-file batch processing.** Only "Merge PDF" is multi-file, because it's structurally multi-file by nature. Everything else is single-file per operation for v1.

## 4. Target Platforms

- **Linux (NixOS)** and **Windows 11**. These are the only two platforms in scope — they are the author's actual daily-use machines.
- A `flake.nix` (or `shell.nix`) must be included at the repo root providing a reproducible dev shell: Rust toolchain, and whatever graphics/system libraries `gpui`/`gpui-component` need to build and run on NixOS (this doesn't "just work" on NixOS the way it does on most Linux distros — dependencies must be declared explicitly).

## 5. Technology Stack

This is a from-scratch decision, made deliberately after rejecting a TypeScript/web stack. Do not reintroduce Node, React, tRPC, or any web framework into this project.

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust | Entire project — UI, business logic, and tooling |
| UI framework | [`gpui`](https://github.com/zed-industries/zed) + [`gpui-component`](https://github.com/longbridge/gpui-component) | Native, cross-platform, shadcn-inspired component set |
| PDF structural ops | [`lopdf`](https://github.com/J-F-Liu/lopdf) | Merge, split, rotate, reorder, watermark, page numbering, crop margins, PDF→PDF/A |
| PDF page rendering | [`pdfium-render`](https://github.com/ajrcarey/pdfium-render) | Only needed for PDF→JPG (page rasterization); `lopdf` cannot render pages visually |
| PDF password protection | [`pdfk`](https://lib.rs/crates/pdfk) | External CLI, invoked as a subprocess. Offline, single binary, AES-256 (PDF 2.0 / R6). `lopdf` can only *decrypt* existing empty-password PDFs — it cannot *create* new password protection, so `pdfk` covers Protect PDF and complements/replaces `lopdf` for Unlock PDF. |
| Image processing | [`image`](https://crates.io/crates/image) | Compress, resize, crop, convert format, rotate, watermark |
| Native file dialogs | [`rfd`](https://github.com/PolyMeilex/rfd) | Both "Open File" and "Save As" — cross-platform native dialogs |
| Drag & drop | `gpui` native drag-and-drop support (if available) | Secondary input method; native file dialog via `rfd` is the primary/required path and must work standalone |
| Error handling | `thiserror` for T1/T2 domain errors, `anyhow` for T0/glue code | See §7 for tier definitions |
| Testing | `cargo test`, `proptest` (property tests), `cargo-llvm-cov` (coverage), `cargo-mutants` (mutation testing) | See §7 |

**Rejected/replaced during scoping** — do not reach for these: LibreOffice (headless), qpdf, Sharp, Node.js, TanStack (Start/Router/Query/Form/Table), tRPC, Better Auth, Drizzle/PostgreSQL, Electron, Tauri.

## 6. Project Structure

A Cargo workspace with a strict lib/bin split, to keep business logic testable without a display/GPU environment:

```
proteus/
├── flake.nix                # Nix dev shell (NixOS reproducibility)
├── Cargo.toml                # workspace root
├── proteus-core/             # library crate — NO UI dependencies
│   ├── src/
│   │   ├── pdf/               # merge, split, rotate, organize, watermark,
│   │   │                      # page numbers, crop margins, pdf_to_a, html_to_pdf
│   │   ├── pdf_render/        # pdfium-render wrapper for PDF→JPG
│   │   ├── pdf_protect/       # pdfk subprocess wrapper (protect/unlock)
│   │   ├── image_ops/         # compress, resize, crop, convert, rotate, watermark
│   │   └── error.rs           # thiserror domain error types
│   └── tests/
├── proteus-app/               # binary crate — gpui / gpui-component only
│   └── src/
│       ├── main.rs
│       └── views/              # one view/screen per tool
└── .github/workflows/         # CI: build + test + gates, release builds
```

`proteus-core` must have zero dependency on `gpui`/`gpui-component`. `cargo test`, coverage, and mutation testing run against `proteus-core` only, and must succeed in CI with no display/GPU available.

## 7. Testing Strategy — the `prove` skill

This project follows the `prove` skill methodology throughout development. Read and apply it as defined (tiers, oracles-before-code, gates, mutation testing, ledger reporting). Key points, restated for this project specifically:

### Tier assignment

| Tier | Surfaces in Proteus |
|---|---|
| **T2** | `pdf_protect` (password encryption/decryption via `pdfk`), malformed/adversarial PDF input handling across all PDF parsing |
| **T1** | All other `proteus-core` operations: merge, split, rotate, organize, watermark, page numbering, crop margins, PDF→PDF/A, HTML→PDF, PDF→JPG rendering, all `image_ops` |
| **T0** | `proteus-app` UI glue: view rendering, native dialog invocation wiring, drag-and-drop plumbing |

### Requirements

- Write oracles (unit tests, property tests for T1/T2, adversarial inputs for T2) **before** implementation, per the skill's step 2.
- Every constraint (coverage floor, mutation floor, tier rules) becomes an executable gate that exits non-zero — wire these into CI, not just documentation.
- Run `cargo-mutants` on all T2 surfaces at minimum; disposition every surviving mutant (killed or annotated as equivalent with reason).
- Produce the ledger described in the skill's step 7 (surface, tier, layers applied, coverage, mutation score, gates enforced, skips and why) as part of the deliverable — not optional polish.
- A failing test is a report of disagreement between code and requirement — resolve by fixing code or explicitly changing the requirement; never by loosening the assertion to reach green.

## 8. File Handling Behavior

- **Input:** native file picker (`rfd`) as the primary method; drag-and-drop onto the window as a secondary convenience if `gpui` supports it without disproportionate effort.
- **Processing:** fully in-memory. Read the source file into memory, process, and write the result — no intermediate temp files on disk.
- **Output:** native "Save As" dialog (`rfd`) after processing completes. The user chooses the destination path explicitly every time; there is no auto-save location or fixed output folder.
- **Max file size:** 500 MB per input file. Enforce this as a validation check before processing begins.
- **Processing model:** fully synchronous. The UI blocks (with a loading indicator) until the operation completes. No job queue, no progress polling, no background workers. This is intentional — video was cut from scope specifically because it was the one thing that would have required this complexity.
- **Batch:** only "Merge PDF" accepts multiple input files. Every other tool is strictly single-file-in, single-file-out for v1.

## 9. Feature Scope

### PDF tools (in scope)
Merge, Split, Compress, Organize/Reorder pages, Rotate, PDF→JPG, Add page numbers, Crop margins, Protect (add password), Unlock (remove password), Add watermark, HTML→PDF, Convert to PDF/A.

### Image tools (in scope)
Compress, Resize, Crop (parameter-based, no interactive canvas), Convert format (JPG/PNG/WebP/AVIF), Add watermark, Rotate.

Everything not listed here is explicitly out of scope per §3.

## 10. UI Structure

- One screen/view per tool (conceptually equivalent to "one route per tool" from the original web-based plan, adapted to `gpui-component`'s view/state model — there is no URL routing).
- Interface language: **English**, no i18n layer.
- No accounts, no settings persistence beyond what's trivially needed (e.g., last-used directory for dialogs, if convenient).

## 11. Non-Functional Requirements

- **Zero network calls, ever.** No telemetry, no crash reporting, no update checks — automatic or manual-triggered. This must be true of the shipped binary and should be easy for a third party to verify by inspection.
- **Zero cost.** No paid APIs, no paid cloud services, no paid infrastructure of any kind.
- **No system-level runtime dependencies** beyond what's bundled with the app itself (this is why LibreOffice and system `qpdf` were rejected in favor of `pdfk`, which is a self-contained Rust binary).

## 12. Distribution & Release

- **Repository:** public, on GitHub.
- **License:** MIT.
- **Project name:** Proteus.
- **CI/CD:** GitHub Actions automatically builds binaries for Windows and Linux on every release/tag.
- **Releases:** published as GitHub Releases with prebuilt binaries for both target platforms, so non-Rust-developer users can download and run directly without `cargo build`.

## 13. Open Questions / Deferred

These were consciously deferred, not decided — flag them if they become blocking during implementation rather than silently resolving them:

- Whether `gpui` supports drag-and-drop file input well enough to implement without disproportionate effort (§5/§8) — verify early; native dialog is the required fallback either way.
- Whether `pdfk`'s binary can be bundled/embedded inside the Proteus binary itself, vs. requiring a separate install step — investigate during `pdf_protect` implementation.
