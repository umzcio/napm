# Plan 024: Build cargo as a scanned ecosystem

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. Touch
> only the files listed as in scope. If any STOP condition occurs, stop and
> report. When done, update the status row in `plans/README.md`.
>
> **Gating**: Targets the codebase AFTER the 18-plan audit chain lands (PR #7,
> branch `advisor/018-disclosure`). Do NOT execute against `main` until that
> merges. Branch from the merge commit. Design approved by the maintainer
> (greenlit 2026-08-08).
>
> **Drift check (run first)**: `git diff --stat ac3389e..HEAD -- src-tauri/src/scan/ src-tauri/src/ops.rs src-tauri/src/store.rs frontend/index.html`

## Status
- **Priority**: P3 | **Effort**: M | **Risk**: LOW-MED | **Category**: direction (build)
- **Depends on**: PR #7 merged. Builds on 009 (parallel scan), 010 (registry cache), 013 (validation), 018 (disclosure).
- **Planned at**: commit `ac3389e`, 2026-08-08

## Why this matters

For any Rust developer, `cargo install`ed tools are invisible by construction: the manual scanner excludes `.cargo` as managed territory, but no scanner owns it, so those binaries appear in no source at all. cargo is the cheapest new ecosystem: one batch source (`.crates2.json`), a crates.io versions API in the shape the registry cache already uses, OSV coverage, and genuine rollback support (`cargo install --version`).

## Design decisions (approved — inlined)

1. **Installed set**: read `~/.cargo/.crates2.json` (structured JSON: version + source + binaries) as primary, `cargo install --list` (text) as fallback. Resolve the install root through `CARGO_INSTALL_ROOT` → `install.root` config → `CARGO_HOME` → `$HOME/.cargo`, not hardcoded.
2. **Latest**: crates.io sparse index primary (no stated rate limit), JSON API (`https://crates.io/api/v1/crates/<name>`) secondary. The shared `http.rs` agent's `napm` user-agent clears crates.io's block bar (note: not their contact-info best-practice — a cross-cutting follow-up, not cargo-scoped).
3. **git/path installs**: no registry "latest" — show with `latest == installed`, no Update action, "unmanaged-flavored" (honest, per the M9 pattern). Source lives in the `PackageId` map-key's parenthesized suffix, not a struct field.
4. **Metadata**: offline `Cargo.toml` `authors`/`description` primary (matches the offline-first convention); size = the installed BINARY under `<root>/bin/<name>`, not the source tree.
5. **Multi-binary crates**: one row per crate. The manual-scanner exclusion must get ALL of a crate's binary names (from `.crates2.json`'s `bins`), plus the resolved cargo install-root `bin/` dir added to `managed_roots`. Both an `scan_all` other-names test and a custom-root test are required (this is the twice-shipped M9 regression class).
6. **Go**: NO. `go install` records no manifest. Non-goal.

## POST-CHAIN reconciliation (design written against bb85e05; these changed — the design doc's own line refs are stale, verify live)

- **`scan_all` is now PARALLEL** (plan 009 wrapped npm/brew/pip/npx in `std::thread::scope`, then manual after the join, then pins, then `stamp_status_and_bump`). The design doc says "scan_all is sequential today" — that is no longer true. cargo joins the `thread::scope` as a FIFTH spawned source alongside npm/brew/pip/npx, and its rows must be in `other_names` before the manual sweep (which runs after the join). Preserve the pins + stamp order.
- **`InstalledTool` gained `status` and `bump` fields** (plan 006), stamped in `scan_all`. Your `scan_cargo` rows get them for free via the stamp step — construct rows with `String::new()` for those two fields.
- **Discovery lookups are memoized** via OnceLock (plan 009: `brew_prefix`/`npm_root`/`python_site`/`pip_bin`). If cargo needs its install-root resolved in more than one place, memoize it the same way.
- **`build_command` has `valid_pkg`/`valid_version` gates and `--` markers** (plan 013). The cargo arms must route `pkg`/`version` through them; add `"cargo"` to `valid_pkg`'s per-eco shape (crate names are `[A-Za-z0-9_-]`, no `/`). cargo supports `--`.
- **Registry-doc cache exists** (`intel::registry::doc`, plan 010, keyed by `(eco,pkg)`, 1h TTL, temp+rename disk layer). Route crates.io version lookups through a cargo arm of that cache rather than a fresh fetch per package. `clear_caches` already sweeps `regdoc_*` — a cargo doc keyed `regdoc_cargo_<pkg>.json` is covered automatically.
- **`Sources` gained `probe_manual` and `advisory_checks`** (014/018) beyond the five source flags. Add `cargo: bool` (default true) to the five-source group, update `Default for Sources`, and extend the settings-default test.
- **`osv_ecosystem`** is in `intel/osv.rs`; add `"cargo" => Some("crates.io")` (verified string).
- **Disclosure**: plan 018 added a "What napm sends" disclosure (About + README). Adding crates.io as a new automatic destination REQUIRES updating that disclosure text in the same change.
- **Preferences/View** now also render the `probe_manual` and `advisory_checks` toggles; add `"cargo"` to the sources array and a "Show: cargo" View entry (note plan 016 relabeled these "Show:" and made them track scan settings).
- **Search** is parallel via `search_all`'s `thread::scope`; a `search/cargo.rs` exact-name lookup joins as a fourth source. Extend the pip-style "exact match" disclosure tag to cargo (plan 005 templated `renderSearchResults` with `h`; use `h`/`raw` correctly).

## Commands you will need
| Purpose | Command | Expected |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 |
| Targeted | `cd src-tauri && cargo test scan::cargo && cargo test ops::` | pass |
| JS gate | extract + node --check | exit 0 |
| fmt/clippy | `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings` | clean |

## Scope
**In scope**: `src-tauri/src/scan/cargo.rs` (create), `src-tauri/src/scan/mod.rs` (join the parallel scope + `pub mod cargo`), `src-tauri/src/scan/manual.rs` (managed-root + other-names for cargo bins), `src-tauri/src/store.rs` (`Sources.cargo`), `src-tauri/src/intel/osv.rs` (ecosystem arm), `src-tauri/src/ops.rs` (cargo build_command arms + tests), `src-tauri/src/search/cargo.rs` (create) + `search/mod.rs` (join), `src-tauri/src/intel/registry.rs` (cargo doc arm if needed), `frontend/index.html` (Preferences/View/search-tag), `README.md` (disclosure update).
**Out of scope**: Go; the crates.io contact-info UA follow-up; changing existing ecosystems.

## Git workflow
- Branch: `advisor/024-cargo` from the merged chain tip.
- Commits: split by layer — `feat(scan): cargo source`, `feat(ops): cargo install/rollback arms`, `feat(intel): crates.io OSV + version cache`, `feat(search): cargo exact-name lookup`, `feat(ui): cargo source toggles + disclosure`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: `scan/cargo.rs`
Parse `.crates2.json` (primary) / `cargo install --list` (fallback); resolve the install root per decision 1; classify source suffix (registry/git/path) per decision 3; build `InstalledTool` rows (with `String::new()` for status/bump). Fixture tests mirroring `scan/npm.rs`'s shapes plus the multi-binary fixture. Register `pub mod cargo;`.
**Verify**: `cargo test scan::cargo` → pass.

### Step 2: Join the PARALLEL scan_all + manual exclusion
Add cargo to the `thread::scope` in `scan_all` (fifth spawn, behind `sources.cargo`); ensure its rows and every crate's `bins` feed `other_names`; add the resolved cargo `bin/` dir to `manual.rs::managed_roots`. Add the `scan_all` other-names test and the custom-root test (decision 5).
**Verify**: `cargo test` → exit 0; the two regression tests pass.

### Step 3: ops + OSV + registry cache
Add `("cargo","install")`/`("cargo","update")`/`("cargo","rollback")` arms (`cargo install --version <v> <pkg>`, routed through `valid_pkg`/`valid_version`), with a test asserting cargo rollback is NOT None (contrast the brew-rollback test). Add `"cargo" => Some("crates.io")` to `osv_ecosystem`. Route crates.io version lookups through `intel::registry::doc`.
**Verify**: `cargo test ops:: && cargo test intel::` → pass.

### Step 4: search + UI + disclosure
`search/cargo.rs` exact-name lookup joining `search_all`. Add `cargo` to `Sources` (default true) + the settings-default test. Preferences checkbox, "Show: cargo" View entry, `VIEW.sources.cargo:true` default, the "exact match" tag for cargo. Update the plan-018 "What napm sends" disclosure to name crates.io.
**Verify**: node --check; `cargo test` green; `grep -in "crates.io" README.md` present.

### Step 5: Manual verification (HUMAN — reproduce)
On a machine with `cargo install`ed tools: they appear in the library under a cargo source; latest resolves for registry crates, git/path crates show no Update; Update/rollback run real `cargo install --version`; unchecking cargo in Preferences removes them and greys "Show: cargo (off in Preferences)". (Note: the audit machine had zero cargo installs — the executor must test on a populated machine or seed one.)

## Done criteria
- [ ] `cd src-tauri && cargo test` exits 0; scan/ops/OSV/settings + the two M9-regression tests pass
- [ ] cargo joins the PARALLEL scan scope (not a sequential extend); manual excludes all cargo bins
- [ ] cargo rollback is supported (test asserts it, contrasting brew)
- [ ] crates.io added to the plan-018 disclosure (About + README)
- [ ] node --check passes; fmt+clippy clean
- [ ] Manual checklist reproduced on a populated machine
- [ ] `plans/README.md` status row updated

## STOP conditions
- PR #7 has not merged.
- `scan_all` is not the parallel `thread::scope` form from plan 009 (drift — do not add a sequential extend).
- The registry cache or `valid_pkg` structure differs from plans 010/013.
- No `~/.cargo` installs exist to test against (seed one, e.g. `cargo install ripgrep`, or report).

## Maintenance notes
- Any new automatic network host (crates.io) is a disclosure-touching change — reviewers should treat a new `http` host as requiring the README/About update.
- Reviewer: the multi-binary `other_names` interaction is the one place this can regress existing sources; the M9 tests must be present.
