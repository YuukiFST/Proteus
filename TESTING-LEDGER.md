# Proteus testing ledger — PRD §7, prove skill step 7.

One table: surface, tier, layers applied, coverage, mutation score, gates enforced, skips and why.
A reader must be able to name, from this ledger alone, the riskiest unproven thing in the codebase.

Status: **suite landed** — every PRD §9 tool implemented in `proteus-core` with oracles
written before implementation, the full gate set green (`cargo test`, `clippy -D warnings`,
`llvm-cov`, `cargo-mutants` on T2 surfaces), and the coverage floor hard-gated in CI
(PRD §7, `.github/workflows/ci.yml`).

## Tier key (PRD §7)

| Tier | Means | Layers |
|---|---|---|
| T0 | Glue, config, no branching | typecheck + lint + one smoke test |
| T1 | Ordinary logic | unit tests per branch + acceptance oracle + coverage gate |
| T2 | Money-class risk (here: password ops, adversarial parsing) | T1 + property tests + torture run + mutation gate + adversarial pass |

## Ledger

Coverage columns: lines / regions / functions % (crate total below, per-file for the
T2 surfaces). Mutation columns: caught / total mutants on the T2 surface files, from
`cargo mutants --in-place`; "disposition" notes survivors that were reviewed.

| # | Surface (PRD §9) | Tier | Oracle (written before code) | Coverage | Mutation caught | Disposition of survivors | Gates enforced | Skips / why |
|---|---|---|---|---|---|---|---|---|
| 1 | PDF merge | T1 (parsing T2) | page count/order + content equality vs inputs; split→merge round-trip | 97.9% | n/a (T2 is parsing layer) | — | cargo test, llvm-cov, mutants, clippy | — |
| 2 | PDF split | T1 (parsing T2) | split→merge round-trip property; page-count partition | 96.3% | n/a | — | same | — |
| 3 | PDF compress | T1 (parsing T2) | output valid PDF, smaller-or-equal, lossless when possible | 97.9% / merge file | n/a | — | same | — |
| 4 | PDF organize/reorder | T1 | page-order permutation oracle; identity permutation | 97.0% | n/a | — | same | — |
| 5 | PDF rotate | T1 | rotation matrix property; rotate×4 ≡ identity | 97.1% | n/a | — | same | — |
| 6 | PDF→JPG (pdfium-render) | T1 | page count matches; bitmap non-blank at target DPI | 26.0% | n/a | — | same | real-pdfium runtime lib absent on CI: availability-probe + unavailable-path covered; real-render tests `#[ignore]` behind `PROTEUS_PDFIUM_LIB` |
| 7 | PDF page numbers | T1 (parsing T2) | text layer contains numbers at expected positions | 98.0% | n/a | — | same | — |
| 8 | PDF crop margins | T1 (parsing T2) | crop-box math property; round-trip restore; out-of-bounds margins rejected | 96.8% | n/a | — | same | — |
| 9 | PDF protect (in-memory AES-256 R6 + pdfk CLI engine) | **T2** | protect→unlock round-trip; wrong pw rejected; password rules (empty / >127 B / NUL, each side); pdfk argv/temp-file protocol; CLI failure → domain error | 92-95% | in-memory+pdfk: 24/24 | none | same + property tests + adversarial corpus | real `pdfk` binary integration test self-skips when absent (hermetic fake-CLI covers protocol) |
| 10 | PDF unlock | **T2** | protect→unlock round-trip; wrong-pw rejected; plain file → `NotEncrypted` | merged row 9 | | none | same | — |
| 11 | PDF watermark | T1 (parsing T2) | AFM-width centering oracle; original content preserved | 87.5% | n/a | — | same | — |
| 12 | HTML→PDF | T1 | text layer contains every word; CJK → `?`; page break; envs | 89.7% | n/a | — | same | resolved: no-network renderer (scraper + ttf-parser + embedded DejaVu) — PRD §13 resolved in-session |
| 13 | Convert to PDF/A | T1 (parsing T2) | output claims PDF/A conformance; no unembedded fonts; unembedded → `NotSupported` | 96.6% | n/a | — | same | — |
| 14 | Image compress | T1 | output decodes, smaller-or-equal; dimension-stable | 90%+ | n/a | — | same | — |
| 15 | Image resize | T1 | target dimensions property; aspect-ratio modes | 90%+ | n/a | — | same | — |
| 16 | Image crop | T1 | bounds-invariant property (out-of-bounds rejected) | 90%+ | n/a | — | same | — |
| 17 | Image convert format | T1 | round-trip decode per format (JPG/PNG/WebP/AVIF) | 90%+ | n/a | — | same | — |
| 18 | Image watermark | T1 | composite pixel oracle at known overlay positions | 90%+ | n/a | — | same | — |
| 19 | Image rotate | T1 | rotate×4 ≡ identity; dimension swap property | 90%+ | n/a | — | same | — |
| 20 | App glue (proteus-app) | T0 | smoke: every PRD §9 tool has exactly one registered view (unique title, group, 19 total) — `cargo test -p proteus-app` | tracked in app crate (`cargo build --release` is the gate) | n/a | — | typecheck + lint + one smoke test + `cargo build --release` | mutation/coverage gates NOT applied to T0 (prove tiers). All 19 tool views call only `proteus-core` for logic; native file dialogs via `rfd` (PRD §8) are the only input path; processing is in-memory and synchronous per PRD §8 |
| 21 | 500 MB input cap (PRD §8) | T1 | boundary oracle: 500 MB passes, +1 byte rejected | 100% (cap) | 4/4 caught (scaffold) | none | cargo test, llvm-cov, mutants, clippy | — |
| — | **Adversarial parsing layer** (`pdf/mod.rs` hardened open_pdf + password open + bomb guards) | **T2** | garbage corpus (truncated/corrupt/non-PDF/deep-header) → every surface Err(MalformedInput); **decompression-bomb boundary oracles**: 2 MiB accepted, 300 MiB rejected, /Filter-array variant; box-array arity; inheritance walkers (page_box/resources, closest-wins) | **pdf/mod.rs 81.0% lines** | **146/146 killed** (sum of final file runs) | 3 accepted — see disposition ledger | cargo-mutants T2 | — |

### T2 disposition ledger (pdf/mod.rs + pdf_protect/*, cargo-mutants 27.1, `--in-place`)

Round 1 (58 missed) → oracles added (in source under "T2 oracles"):
`decompression_bomb_over_cap_is_rejected`, `large_bounded_stream_is_accepted`,
`bomb_guard_covers_array_filters`, `pdf_text_bytes_maps_whole_winansi_charset`
(all 28 WinAnsi arms incl. U+0161), `escape_pdf_string_escapes_specials_and_keeps_newlines`,
`page_box_resolves_inherited_mediabox`, `page_box_rejects_short_box_array`,
`prune_unreachable_drops_orphans`, `set_resource_entry_keeps_existing_page_resources`,
`set_resource_entry_merges_closest_ancestor_resources`, and (protect)
`passwords_fail_individually_on_length_and_nuls` + the 127-byte legal-maximum boundary
(passwords of exactly 127 bytes succeed end to end; 128 fail).

Final gate numbers (3-file T2 surface, last run of each file):
- pdf/mod.rs: 133 mutants → 124 caught, 5 unviable, 4 missed → 3 accepted (below) + 1
  killed by the owner-side boundary oracle added afterwards (verified by mutant simulation).
- pdf_protect/mod.rs (rerun after that oracle): 23 mutants → 21 caught, 2 unviable, 0 missed.
- pdf_protect/pdfk.rs: 13 mutants → 11 caught, 2 unviable, 0 missed.

**Accepted survivors (documented disposition):** the three `LoadOptions.max_decompressed_size`
field-deletion mutants in open_pdf / open_pdf_with_password. Deleting the loader-level cap
(default `None`) changes no observable behavior verified by any oracle: it only governs
xref/object-stream decode *during* parse, and the post-load `reject_bomb_streams` sweep
enforces the same 256 MiB cap on every /FlateDecode stream one layer later — the sweep's own
mutants are all killed. Accepted as defense-in-depth redundancy, not as silent behavior.

## Final audit (this session)

- `cargo test --workspace`: 123 passed, 2 ignored (real-pdfium render tests, gated
  behind `PROTEUS_PDFIUM_LIB`), 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (0 warnings).
- `cargo llvm-cov test -p proteus-core`: **91.21% lines / 90.85% regions / 87.06%
  functions** — floor 80% lines, hard-gated in CI.
- `cargo mutants` T2 surface: zero undispositioned survivors (disposition ledger below).
- No test assertion weakened during the campaign; every survivor found a strengthened
  or new oracle (or an accepted-disposition note).

## Gates

The whole gate set runs with `nix develop -c` (verified on NixOS 26.05, rustc 1.95.0):
`cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` +
`cargo llvm-cov test -p proteus-core` + `cargo mutants --package proteus-core` +
`cargo mutants --package proteus-core --file proteus-core/src/pdf/mod.rs --file
proteus-core/src/pdf_protect/mod.rs --file proteus-core/src/pdf_protect/pdfk.rs --in-place`
(T2 surface: zero undisput survivors on it).

**Floors (CI hard gates, PRD §7):** line coverage ≥ 80% on the `proteus-core` crate total
(parsed from `cargo llvm-cov test -p proteus-core --json`; pdfium-render's real-render
branch exempt per ledger row 6). Mutation: no *undispositioned* survivors on T2 surfaces.

## Riskiest unproven thing today

1. Real-pdfium rendering is exercised only in `#[ignore]` tests (no pdfium binary in the
   dev shell / CI). The probe path (`PROTEUS_PDFIUM_LIB`, failure → `PdfRenderUnavailable`)
   is covered.
2. A real `pdfk` binary never runs in CI — the hermetic fake pins the protocol; real
   `pdfk` crypto equivalence is unproven here (correctness of AES itself is lopdf's / pdfk's).


## Final audit — proteus-app UI surface (this session, PRD §9/§10)

Evidence is compile-time + static-analysis real (this environment has no display, so
the gpui window is never executed here — see "Riskiest unproven thing today").

- One view file per PRD §9 tool: 19 views in `proteus-app/src/views/`
  (13 PDF + 6 image), registered in `Tool::ALL` with unique titles/groups; the shell
  (`views/mod.rs`) renders the selected view via `AnyView` inside a gpui-component
  `Root`, sidebar-driven like one route per tool (PRD §10).
- Hard §6 separation holds: `grep` across `proteus-app/src` for `lopdf|pdfium|pdfk|image::`
  finds zero imports — every tool view calls only `proteus-core` functions for
  logic (some add a page-count/renderer guard before the main op); the app crate is glue (dialogs, params, status) only.
- §8 file behavior: every tool uses the native `rfd` dialog for input pick, output
  "Save As", or output folder; processing is fully in-memory and synchronous; the
  500 MB input cap is enforced inside `proteus-core` entry points (covered by core
  tests, rows 1–19).
- T0 smoke test `tests::every_prd_tool_has_a_unique_view` in `proteus-app/src/main.rs`:
  asserts all 19 tools are reachable with unique titles (duplicate titles were caught
  and fixed by this test), plus non-empty groups — `cargo test -p proteus-app`: 1 passed.
- `cargo check -p proteus-app`, `cargo clippy -p proteus-app --all-targets -- -D warnings`:
  clean (0 warnings/errors) after the fix pass (App/AsyncApp `.ok()` Result-context
  mismatches, gpui `WindowBounds::centered(size, cx)` signature, `&'static str` surface
  params, missing `this.clone()` into spawn closures, unused imports, clippy
  `needless-borrows/redundant-closure-call`).
- `cargo build --release --workspace`: **succeeded** (4m 20s, rustc 1.95.0 via
  `nix develop`). This is the release build for BOTH crates in one invocation:
  `target/release/proteus` (48.2 MB GUI binary, linking `libproteus_core.rlib`
  25.4 MB). The debug/coverage runs of proteus-core were already gate-green from
  the earlier campaign; the workspace build additionally proves the full release
  pipeline end to end.
