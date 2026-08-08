# Plan 021: Spike a library import ("set up a new Mac from my manifest")

> **Executor instructions**: This is a DESIGN/SPIKE plan — the deliverable is a
> written design (`docs/design/import.md`); no build until the maintainer
> approves it. If anything in the "STOP conditions" section occurs, stop and
> report. When done, update the status row in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/lib.rs frontend/index.html`

## Status

- **Priority**: P3
- **Effort**: M-L overall (coarse); the design itself is S
- **Risk**: MED (a bulk import runs many real package-manager commands; honesty demands a preview and per-item failure reporting)
- **Depends on**: plans/019 conclusions are relevant (shared confirm-flow patterns); execution reuses 003/004's hardened transfer path
- **Category**: direction
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Export exists with no counterpart: napm can write the library to JSON/Markdown, but nothing can read one back. The obvious use of the exported file — recreate my CLI toolchain on a new machine, or hand it to a teammate — is the most-wanted capability in this tool category, and napm is uniquely positioned because one manifest spans npm + brew + pip (+ npx and manual, which import CANNOT install — the design's honesty problem to solve). The install machinery already exists end to end; the genuinely new work is the manifest contract, the preview, and batch failure reporting. Caveat stated plainly: unlike plans 019/020, no roadmap entry promises this — it is inferred from the surface asymmetry, so the FIRST question for the maintainer is whether they want it at all.

## Evidence (inline, verified)

- Export half: `src-tauri/src/lib.rs:103-114` (`export_library` writes frontend-composed JSON/Markdown to app-data and reveals in Finder); `frontend/index.html:935-948` (`exportLibrary` — the JSON flavor is `JSON.stringify(TOOLS, null, 2)`, i.e. raw `InstalledTool` rows including machine-local fields like `size`, `updated`, `pinned`).
- No import: `grep -n "import" src-tauri/src/lib.rs` → nothing; the `invoke_handler` list (`lib.rs:236`) has no such command.
- Install machinery ready for reuse: `installPackage` (`frontend/index.html:500-511`) already handles "not in library → push row → queueTransfer(install)"; `ops::run_op` streams and logs each op.
- Update All (`:774-776`) is the existing precedent for firing a batch of ops — and (pre-plan-003/004) its concurrency problems are exactly what a 40-item import would amplify; the design must sequence.

## Design questions the document must answer

1. **Does the maintainer want this?** One paragraph pitch, ask first. If no: record REJECTED in `plans/README.md` and stop — that is a successful outcome.
2. **Manifest schema**: a versioned, intentional format (e.g. `{schema: 1, tools: [{pkg, eco, version?}]}`) rather than the raw `TOOLS` dump; define whether today's export gains a third "manifest" flavor or the JSON export becomes the manifest (recommend: dedicated flavor; the current dump has machine-local noise). Versions: pin-to-exported-version vs install-latest — per-item choice or global toggle?
3. **The honest exclusions**: `manual` rows have no install path; `npx` rows have no meaningful install (promote?); brew rollback limits mean a pinned brew version cannot be honored. The preview must show three buckets: will install / already present / cannot install (with reasons).
4. **Preview-then-execute**: file picker (Tauri dialog plugin is NOT currently a dependency — note the addition) or drag-drop; a preview modal listing the buckets; execution as a SEQUENTIAL queue through the standard Transfers path (one op at a time, per plan 003's in-flight model), with a per-item and end-of-run summary ("14 installed, 2 failed, 3 skipped") that never collapses failures into silence.
5. **Cross-ecosystem collisions and safety**: importing `black` when the manifest says pip but brew has it installed; importing over an existing older version (that is just an update); a manifest naming a package that no longer exists in the registry (fails honestly in the transfer row).
6. **Scope of v1**: recommend npm + pip + brew install-latest with per-item preview, no version pinning in v1 (pinning adds rollback-shaped complexity per ecosystem); manifest schema field reserves room.

## Done criteria (design phase)

- [ ] `docs/design/import.md` exists answering the six questions, with the maintainer's yes/no recorded up front
- [ ] If yes: a build outline with the new command surface (`import_preview(manifest) -> buckets`, execution via existing `run_op`), effort estimate, and the dialog-plugin dependency decision
- [ ] If no: `plans/README.md` row marked REJECTED with the maintainer's one-line reason
- [ ] No source code modified (`git status` shows only the new doc)

## STOP conditions

- The maintainer answers no at question 1 (record and stop — do not build a smaller version).
- The design requires a new Tauri plugin with capability changes and the maintainer has not approved the dependency.

## Maintenance notes

- The manifest schema, once published in a release, is a compatibility surface; version it from day one.
- A future "diff against manifest" view (what drifted since export) falls out of the same preview machinery; note it as the natural v2.
