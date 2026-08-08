# Plan 022: Design cargo as a scanned ecosystem (currently invisible by construction)

> **Executor instructions**: This is a DESIGN plan — the deliverable is a written
> design (`docs/design/cargo-source.md`) plus maintainer sign-off before any
> build. A build outline is included to keep the design grounded. If anything
> in the "STOP conditions" section occurs, stop and report. When done, update
> the status row in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/scan/ docs/ROADMAP.md`

## Status

- **Priority**: P3
- **Effort**: M (coarse; design S, build M)
- **Risk**: LOW-MED (additive source behind a flag; the M9 lesson about manual-scanner interaction needs a test)
- **Depends on**: plans/009 (joins the parallel scan scope), plans/013 (command validation gate), plans/018 (disclosure line for crates.io)
- **Category**: direction
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

napm's premise is "one view of every CLI tool you have" — and for a Rust or Go developer (this project's own author writes Rust), `cargo install`ed tools are invisible BY CONSTRUCTION: the manual scanner's managed-roots list excludes `.cargo` as "someone else's territory", but no scanner owns it, so those binaries appear in no source at all — not even as unmanaged. cargo is likely the cheapest ecosystem left to add: one batch command lists name+version, crates.io has a versions API in the same shape as the npm/PyPI lookups, OSV covers the crates.io ecosystem for the security scan, and `cargo install <pkg> --version <v>` supports genuine rollback (which brew cannot). Not roadmap-promised (M11 is the stated next milestone), so this is an option the maintainer ranks, not a commitment.

## Evidence (inline, verified)

- The invisibility mechanism: `src-tauri/src/scan/manual.rs` managed-roots include `.cargo` (and `go/bin` etc.; the file's tests assert `is_managed(".../.cargo/bin/cargo")` is true), while `scan/mod.rs:62-77` runs only npm/brew/pip/npx/manual — nothing claims cargo's territory.
- The pattern to mirror: `src-tauri/src/scan/npm.rs` (batch command → JSON/line parse → `InstalledTool` rows); `cargo install --list` is the direct analogue (line format: `pkg vX.Y.Z:` followed by indented binary names).
- OSV: `src-tauri/src/intel/osv.rs` maps ecosystems via `osv_ecosystem` (npm/npx→"npm", pip→"PyPI"; one new arm → "crates.io").
- Sources plumbing: `store.rs:20-29` (`Sources` flags), Preferences checkboxes (`frontend/index.html:1046-1049`), View menu filters — each gains a `cargo` entry mechanically.
- Ops: `ops.rs::build_command` gains `("cargo", ...)` arms — `cargo install <pkg>` (latest), `cargo install <pkg> --version <v>` (pinned/rollback).

## Design questions the document must answer

1. **Installed-set truth**: parse `cargo install --list`, or read `~/.cargo/.crates2.json` (structured, includes version + source + binaries)? Recommend `.crates2.json` with `--list` as fallback; document both formats with real captures from this machine.
2. **Latest resolution**: crates.io API (`https://crates.io/api/v1/crates/<name>`) — note its required user-agent etiquette and rate norms; where does it slot into plan 010's registry cache (a third eco arm)?
3. **The `--git`/path installs**: `.crates2.json` records source; git/path installs have no registry "latest" — label them honestly (the M9 "unmanaged-flavored" pattern: shown, version known, no update path claimed).
4. **Metadata columns**: publisher (crates.io `owner`s or the crate's `authors`?), size (the installed BINARY, not the source tree — `~/.cargo/bin/<bin>` file size via the plan-002-fixed `dir_size`), description (crates.io or offline from `.crates2.json`? — it has no description; decide offline-first policy vs a registry fetch, and note the tension with the "offline metadata" convention the other sources follow).
5. **Multi-binary crates**: one crate can install several binaries (`cargo-edit` → `cargo-add`, `cargo-rm`...). One row per crate (recommend) or per binary? How does the manual scanner's `other_names` exclusion get ALL the binary names so nothing double-appears (this is the M9 twice-shipped regression class — the design must specify the `scan_all` ordering and a test)?
6. **Go too?** The audit's answer: no — `go install` records no manifest of what is installed; keep Go binaries as manual/unmanaged and say so in the design's non-goals.

## Build outline (after sign-off)

`scan/cargo.rs` mirroring `scan/npm.rs` (+ tests with captured fixtures); `Sources.cargo` flag + Preferences/View entries; OSV arm + wire consideration; `build_command` cargo arms through the validation gate; registry-cache arm; disclosure line (crates.io) per plan 018; joins the plan-009 parallel scope; `scan_all` `other_names` test covering multi-binary crates.

## Done criteria (design phase)

- [ ] `docs/design/cargo-source.md` exists answering all six questions with real captured examples (`.crates2.json` excerpt, `cargo install --list` output from this machine)
- [ ] Maintainer sign-off recorded (including the priority call vs M11)
- [ ] No source code modified (`git status` shows only the new doc)
- [ ] `plans/README.md` status row updated

## STOP conditions

- This machine has no `~/.cargo` installs to capture real examples from (unlikely — the project builds with cargo; report if so).
- The maintainer ranks M11 (plan 020) first and wants this parked — record the ranking and stop.

## Maintenance notes

- The multi-binary `other_names` interaction is the one place this can regress existing sources; whatever ships must carry the `scan_all` test from day one.
- crates.io asks for a descriptive User-Agent on API calls; the shared `http.rs` agent sets `napm` already — confirm that satisfies their policy in the design.
