# Plan 025: Build library import (recreate a toolchain from a manifest)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. Touch
> only the files listed as in scope. If any STOP condition occurs, stop and
> report. When done, update the status row in `plans/README.md`.
>
> **Gating**: Targets the codebase AFTER the 18-plan audit chain lands (PR #7,
> branch `advisor/018-disclosure`). Do NOT execute against `main` until that
> merges. Branch from the merge commit. The maintainer answered YES to building
> this (greenlit 2026-08-08), resolving the design's open question 1.

> **Drift check (run first)**: `git diff --stat ac3389e..HEAD -- src-tauri/src/lib.rs frontend/index.html src-tauri/tauri.conf.json`

## Status
- **Priority**: P3 | **Effort**: M-L | **Risk**: MED (bulk real installs) | **Category**: direction (build)
- **Depends on**: PR #7 merged. Builds on 003/004 (the hardened transfer path).
- **Planned at**: commit `ac3389e`, 2026-08-08

## Why this matters

Export exists with no counterpart. The obvious use of the exported file — recreate a CLI toolchain on a new machine — is the most-wanted capability in this category, and napm is one command away from it while being the only tool spanning npm+brew+pip in one manifest. The new work is the manifest contract, an honest preview, and batch failure reporting.

## Design decisions (approved — inlined)

1. **Yes, build it** (maintainer confirmed). Honest caveat baked into the UI: import cannot clone a machine — manual and npx rows have no install path, brew cannot pin a version.
2. **Manifest schema**: a dedicated, versioned export flavor, NOT the raw `TOOLS` dump (which carries machine-local fields). Shape: `{schema: 1, generatedAt, tools: [{pkg, eco, version}]}`, only npm/brew/pip rows written (manual/npx excluded at export). Import refuses an unrecognized `schema`. v1 installs LATEST for every eco (no pinning — brew cannot honor a pin, and a manifest where 2/3 of rows respect a pin and 1/3 silently cannot is exactly the dishonesty the project avoids). The `version` field is kept for display ("exported at X, will install Y") and the "already present" comparison; the schema field reserves room for future pinning.
3. **Three buckets** in preview: will install / already present / cannot install (with a specific reason each), mirroring the existing brew-rollback disable-with-reason pattern.
4. **Flow**: get the file in via a File menu "Import library..." item (native picker) or drag-drop. **The Tauri dialog plugin is NOT currently a dependency** — a native picker adds `tauri-plugin-dialog` + a capabilities entry; drag-drop needs no new Rust dep and is the cheaper v1. Then a preview modal (existing modal chrome) showing the buckets, then SEQUENTIAL execution (one op at a time) through the standard Transfers path, then an end-of-run summary ("14 installed, 2 failed, 3 skipped, 1 already present") that lists failed names explicitly and never folds failures into the success count.
5. **Collisions/safety**: same name in two ecosystems → two independent rows keyed by (eco, pkg) — MUST use the eco-aware lookup, not a pkg-only one (see reconciliation). Importing over an older version is just an update. A package that no longer exists fails honestly in its transfer row and is counted "failed", not dropped.
6. **v1 scope**: npm+pip+brew, install-latest, per-item preview, sequential execution, honest summary. No pinning. v2 (noted, not built): "diff against manifest".

## POST-CHAIN reconciliation (design written against bb85e05; verify live)

- **`queueTransfer(t, target, action)` takes a TOOL OBJECT with an in-flight dup guard** (plan 004). `installPackage` was rewritten to push a real tool object and pass it (no `TOOLS.length-1`). Import's execution reuses this — build a tool object per will-install row and pass it.
- **The design's "sequential is a deliberate departure" note is correct and MORE important now**: `queueTransfer` fires `run_op` immediately (fire-and-forget); the backend now REJECTS a duplicate (eco,pkg) op (plan 003) but does NOT serialize different packages. Import's sequential requirement is new frontend logic: drive the will-install bucket with a loop that starts ONE `queueTransfer`, waits for its `transfer-done` event (keyed by `op_id`), then starts the next.
- **Use `findToolIdx(pkg, eco)`, not `findTool`** — `findTool` was DELETED in plan 004. The "already present" bucket check and collision handling must be eco-aware.
- **`export_library` now returns `Result<String,String>`** (plan 007). Add the manifest export flavor alongside the existing JSON/Markdown flavors; it returns the path like the others.
- **`transfer-done` resolves via `findToolIdx` and the renderer is incremental** (plans 004/011). The sequential loop keys on `op_id` from the transfer record (records carry `pkg`+`eco`).
- **Escaping is by-construction** (`h`/`raw`, plan 005). The preview modal and summary must build markup with `h`.
- If you add the dialog plugin: `src-tauri/Cargo.toml`, `src-tauri/capabilities/default.json`, and the plan-018 disclosure are all touched. Recommend drag-drop for v1 to avoid the dependency; decide explicitly and note it.

## Commands you will need
| Purpose | Command | Expected |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 |
| Targeted | `cd src-tauri && cargo test intel:: ; cargo test import` | pass |
| JS gate | extract + node --check | exit 0 |
| fmt/clippy | `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings` | clean |

## Scope
**In scope**: `src-tauri/src/lib.rs` (`import_preview` command + the manifest export flavor), a small `src-tauri/src/importer.rs` or inline module (parse + bucket classifier, pure + tested), `frontend/index.html` (Import menu item, drag-drop or picker, preview modal, sequential queue loop, summary, manifest export flavor in `exportLibrary`). If a native picker is chosen: `src-tauri/Cargo.toml`, `src-tauri/capabilities/default.json`, `README.md` (disclosure).
**Out of scope**: version pinning; the "diff against manifest" v2; importing manual/npx rows; changing install/update behavior.

## Git workflow
- Branch: `advisor/025-import` from the merged chain tip.
- Commits: `feat(export): versioned import-manifest flavor`, `feat(import): preview command and bucket classifier`, `feat(ui): import preview modal and sequential queue`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Manifest export flavor
In `exportLibrary` (frontend) add a third "Import manifest" flavor producing `{schema:1, generatedAt, tools:[{pkg,eco,version}]}` for npm/brew/pip rows only. It flows through the existing `export_library` `Result<String,String>` path.
**Verify**: node --check; a manual export produces the shape (or add a tiny JS-shape assertion).

### Step 2: `import_preview` + bucket classifier
Add a pure `classify(manifest, installed) -> {will_install, already_present, cannot_install}` (with per-row reasons), unit-tested in the `ops.rs` test style. Wrap it in `#[tauri::command(async)] fn import_preview(manifest_json: String) -> ImportPreview` that parses+validates the manifest (reject unknown `schema`) and cross-references a fresh scan. Register in `generate_handler!`.
**Verify**: `cargo test` → classifier tests pass (schema-mismatch rejected, already-present detected via eco-aware match, unknown-eco → cannot-install with reason).

### Step 3: Preview modal + sequential execution
Frontend: "Import library..." File menu item; drag-drop of a `.json` onto the window (recommended v1) OR a native picker if the dialog plugin is added (decide + note). Preview modal (existing chrome, `h` markup) showing the three buckets with reasons and an "Import N tools" button disabled when will-install is empty. On confirm, drive the will-install bucket with a sequential loop: one `queueTransfer(toolObj, latest, "install")`, await its `transfer-done` (by `op_id`), then the next. End with a summary line listing failed names explicitly.
**Verify**: node --check.

### Step 4: Manual verification (HUMAN — reproduce)
Export a manifest on one state, uninstall a package, import the manifest → preview shows the missing one in "will install", the rest in "already present", any manual/npx-shaped junk in "cannot install" with a reason; confirm runs installs ONE AT A TIME (Transfers shows them sequentially, not all at once); the summary reports counts and names failures. Feed a manifest with a nonexistent package → it fails honestly and is counted, not dropped.

## Done criteria
- [ ] `cd src-tauri && cargo test` exits 0; classifier tests (schema reject, already-present eco-aware, cannot-install reasons) pass
- [ ] `import_preview` registered; manifest export flavor produces the versioned schema
- [ ] Execution is sequential (one op awaited before the next); summary lists failures explicitly
- [ ] Preview uses eco-aware matching (`findToolIdx`), never `findTool`
- [ ] If the dialog plugin was added: Cargo.toml + capabilities + disclosure updated; else drag-drop only, noted
- [ ] node --check passes; fmt+clippy clean
- [ ] Manual checklist reproduced
- [ ] `plans/README.md` status row updated

## STOP conditions
- PR #7 has not merged.
- `queueTransfer` is not the tool-object form / `findTool` still exists (drift from plan 004).
- The sequential loop cannot reliably key on `transfer-done` by `op_id` (report rather than firing all ops concurrently — that would be the least honest version of this feature).

## Maintenance notes
- The manifest `schema` is a compatibility surface the moment a release ships it; version it from day one and refuse unknown versions.
- v2 "diff against manifest" reuses the same three-bucket classifier with an inverted default — note it, don't build it here.
- Reviewer: scrutinize the sequential-queue wiring (a race that starts the next op before the previous `transfer-done` reintroduces the concurrency problem this feature is meant to avoid).
