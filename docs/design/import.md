# Library import: set up a new Mac from a napm manifest

napm can already export the Shared Library to JSON or Markdown (File menu, `exportLibrary` in `frontend/index.html:935-948`, backed by the `export_library` command at `src-tauri/src/lib.rs:103-114`), but nothing reads that file back. The obvious use of an export is the thing everyone actually wants from a package manager: point it at a manifest and recreate your CLI toolchain on a new machine. napm is well placed to do this because one manifest can span npm, brew, and pip, and the install machinery to act on it already exists end to end (`ops::run_op` in `src-tauri/src/ops.rs:58-134`, wired through `run_op` at `src-tauri/src/lib.rs:39-55`). The new work is small: a manifest contract, a preview that is honest about what can and cannot be installed, and batch execution that reports failures instead of swallowing them. No roadmap entry currently promises this feature; it is inferred from the asymmetry between export and import, and needs a maintainer call before any of the design below is built.

**Decision: PENDING maintainer yes/no**

Everything past this point is the design contingent on a yes, so the maintainer can see the whole shape before deciding.

---

## 1. The pitch, and the honest caveat

The pitch: export your library on one Mac, import it on another (or after a clean reinstall), and napm reinstalls everything it can, tells you exactly what it skipped and why, and never pretends to install something it can't.

The caveat that has to be stated up front, not discovered later: import cannot be a full "clone this machine" tool. Three categories of row in the Shared Library have no install path:

- **manual** rows (tools napm found on `$PATH` but did not install and has no registry package for): there is nothing to run.
- **npx** rows (ad hoc `npx` invocations, not installed packages): running `npx <pkg>` again does not "install" anything meaningful; the closest analog is npm-installing the package globally, which is a different action than what the row represents (see `ops::build_command`'s `("npx", "promote")` case at `src-tauri/src/ops.rs:24-28`, which already exists for exactly this reason).
- **brew version pinning**: Homebrew keeps no old bottles, so an exported brew version cannot be reproduced; only "install whatever `brew install <pkg>` gives you today" is possible (see `build_command`'s `("brew", "install") | ("brew", "update")` arm at `src-tauri/src/ops.rs:21-23`, and the existing rollback-gating precedent in `src-tauri/src/ops.rs:169-171`, `brew_rollback_is_unsupported`).

Import's job is to make these limits visible before the user commits to anything, not to paper over them.

## 2. Manifest schema

**Recommendation: a dedicated, versioned export flavor, not the raw `TOOLS` dump.**

The existing JSON export is `JSON.stringify(TOOLS, null, 2)` (`frontend/index.html:946`), which serializes whatever is in the in-memory `TOOLS` array verbatim. Reading the real `InstalledTool` struct (`src-tauri/src/scan/mod.rs:15-31`) confirms that array carries machine-local fields that mean nothing on a different machine: `size` (bytes on this disk), `pinned` (a local preference, not a property of the package), `description`/`publisher` (fetched metadata, redundant to reconstruct), and an install-time timestamp. Round-tripping that shape as "the import format" would mean every future field added to `TOOLS` silently becomes part of an on-disk contract, and it would ask the importer to look at fields it should ignore.

Instead, add a third export flavor, e.g. "Export library (Import manifest)", alongside the existing JSON and Markdown options in the File menu (`frontend/index.html:939-940`), producing:

```json
{
  "schema": 1,
  "generatedAt": "2026-08-08T00:00:00Z",
  "tools": [
    { "pkg": "@anthropic-ai/claude-code", "eco": "npm", "version": "1.4.2" },
    { "pkg": "ripgrep", "eco": "brew", "version": "14.1.1" },
    { "pkg": "httpie", "eco": "pip", "version": "3.2.2" }
  ]
}
```

- `schema` is an integer, bumped on any breaking shape change. Import refuses (with a clear message) any `schema` it does not recognize, rather than guessing.
- `version` is the version installed at export time, kept for future pinning (see next section) and for the preview's "already present" comparison, but v1 import does not attempt to honor it as a pin target.
- Only `npm`, `brew`, and `pip` rows are ever written to this flavor; manual and npx rows are excluded at export time (not filtered at import time), since a manifest that never contained them is a more honest artifact than one that filters them silently on the way back in.

**Versions: install-latest, not pin-to-exported, for v1.** Pinning to an exact exported version is trivially correct for npm and pip (`ops::build_command`'s `("npm", _)` and `("pip", _)` arms both take an explicit version), but brew has no mechanism to honor it at all. Presenting a single manifest where two-thirds of the rows can respect a pinned version and one-third silently cannot is exactly the kind of quiet inconsistency the project's "do not fake what is not possible" rule warns against. Recommendation: v1 import always installs latest for every ecosystem, uses the recorded `version` field only for display ("exported at 1.4.2, will install 1.6.0") and for the "already present" comparison, and reserves per-item pinning as a v1.5/v2 feature once brew's limitation can be surfaced item-by-item rather than manifest-wide. The schema field is kept from day one specifically so this is additive later, not a breaking migration.

## 3. Honest exclusions and the three buckets

Preview classifies every manifest row into exactly one bucket before anything runs:

- **Will install**: eco is npm/brew/pip, package not currently in the Shared Library (or present but at an older version).
- **Already present**: installed version matches (or exceeds) the manifest version; nothing to do, shown so the user isn't left wondering why a row didn't run.
- **Cannot install**: with a specific reason, not a generic "skipped": manifest schema too new, malformed row, or (if a manual/npx export flavor is ever fed back in by mistake) "manual tools have no install source" / "npx entries are not installed packages, see Promote to global instead".

This mirrors the honesty rule already applied to brew rollback in Transfers (`src-tauri/src/ops.rs:169-171` and the frontend's re-run disabling logic in `frontend/index.html:761-763`, which disables re-running a rollback whose target eco is brew): the UI has an existing pattern of disabling an action and stating why rather than letting it fail silently, and import should reuse that pattern rather than invent a new one.

## 4. Preview-then-execute flow

1. **Getting the file in.** Two options, not mutually exclusive: a File menu item ("Import library...") that opens a native file picker, and drag-drop of a `.json` manifest onto the app window. **Flag for the maintainer:** the Tauri dialog plugin is not currently a project dependency (`src-tauri/Cargo.toml` lists `tauri-plugin-log` and `tauri-plugin-updater` only, no `tauri-plugin-dialog`). Adding a native file picker means adding that plugin (and its `dialog` permission scope in the capabilities config), which is a small but real new surface area. Drag-drop can be implemented in the existing frontend with plain browser APIs and needs no new Rust dependency, so it is the cheaper of the two to ship first if the maintainer wants to defer the dialog plugin decision.
2. **Preview modal.** Reuses the app's existing modal chrome (the same beveled-window pattern used elsewhere, e.g. the settings dialog referenced at `frontend/index.html:1062`). Shows the three buckets from section 3, each row showing name, eco, exported version vs. what will actually install, and (for "cannot install") the reason. A single "Import N tools" button, disabled when the will-install bucket is empty.
3. **Execution.** Confirmed rows are pushed through the exact same path Search already uses for a fresh install (`installPackage` in `frontend/index.html:500-511`: push a row into `TOOLS`, call `queueTransfer`, switch to the Transfers tab), just looped over the will-install bucket. Reusing this path means import gets streamed stdout/stderr, real exit codes, and history logging for free, since that is all already inside `ops::run_op`.
4. **Sequential, not parallel, and this is a deliberate departure from existing precedent, not a continuation of it.** Update All's `forEach` (`frontend/index.html:774-776`) calls `queueTransfer` once per outdated tool, and `queueTransfer` (`frontend/index.html:738-751`) is not actually a queue: each call immediately invokes the `run_op` command, and `run_op` spawns its child process on a new background thread right away (`ops::run_op`, `src-tauri/src/ops.rs:72`). So today, Update All already fires every outdated package's install concurrently as separate OS processes: there is no throttling to "reuse." That is fine at Update All's typical scale (a handful of outdated packages) but is exactly the pattern the plan warns against at import's scale (up to dozens of packages in one manifest): running many `npm i -g`/`brew install`/`pip install` calls at once risks lock contention (npm and pip both take a lock on their global environment) and makes streamed output from `ops::run_op` (one `LineEvent` stream per op, `src-tauri/src/ops.rs:40-52`) unreadable when interleaved across many concurrent rows in the Transfers pane. Import therefore needs new sequencing logic in the frontend, not reuse of `queueTransfer`'s current fire-and-forget behavior. It should drive the will-install bucket with an explicit loop that starts exactly one `queueTransfer` call, waits for that op's `transfer-done` event, then starts the next. This is a small, self-contained addition (a promise chain or async loop keyed on `op_id`), and worth calling out as new work rather than something Update All already provides.
5. **End-of-run summary.** A single line/toast once the queue drains: "14 installed, 2 failed, 3 skipped, 1 already present" (numbers illustrative). Failures are never folded into the success count or dropped after the transfer log scrolls away; the summary text lists failed package names explicitly so a failure is discoverable without scrolling back through Transfers history.

## 5. Collisions and safety

- **Same name in two ecosystems** (e.g. a `black` in both brew and pip): treated as two independent manifest rows, each evaluated against the Shared Library keyed by `(eco, pkg)`. This needs care rather than a free ride from existing code: the search merge logic already dedupes on `(eco, pkg)` (`search/mod.rs:25-36`), but the frontend has two different lookup helpers and they are not equivalent. `findTool(pkg)` (`frontend/index.html:340`), which `installPackage` currently uses, matches on `pkg` alone and would resolve to whichever row happens to be first for that name regardless of ecosystem. `findToolIdx(pkg, eco)` (`frontend/index.html:562`) matches on both. Import's row-matching (bucket classification and the post-install "is this already in the library" check) must use the `(eco, pkg)`-aware lookup, not `findTool`, or a manifest with the same name in two ecosystems will silently collide. This is a real gap in the current code worth flagging to the maintainer regardless of the import decision.
- **Importing over an older installed version:** this is just an update, not a new install. The "will install" bucket already covers this (section 3); the row runs through `queueTransfer(..., "update")` instead of `"install"` and is labeled as such in preview, matching the distinction `installPackage` already draws between install and update (`frontend/index.html:503-506`).
- **A package that no longer exists in its registry:** import cannot know this until it tries. It is not filtered out in preview (napm has no reliable existence check for pip/brew without an extra network round trip that would slow preview down for uncertain benefit); it fails honestly in the transfer row with the real npm/brew/pip error text streamed from `ops::run_op`, and is counted in the "failed" bucket of the end-of-run summary, not silently dropped.

## 6. v1 scope recommendation

Ship: npm + pip + brew, install-latest only, per-item preview with the three buckets, sequential execution through the existing Transfers path, end-of-run summary. No version pinning. Manual and npx rows never appear in the manifest (excluded at export). The `schema` field exists from the first release specifically so pinning can be added later without a breaking format change.

Natural v2, once v1 ships and the maintainer has seen real manifests in use: **diff against manifest**. Instead of "install everything in this file," compare the current Shared Library against a manifest and show only the delta (what's missing, what's older, what's extra locally), useful for keeping two machines in sync rather than only bootstrapping a fresh one. This reuses the same three-bucket preview logic with an inverted default (skip "already present" by default instead of listing it) and is a natural follow-on rather than new machinery.

## Build outline

New command surface:

- `import_preview(manifest_json: String) -> ImportPreview`: Rust-side, parses and validates the manifest (schema check, row shape), cross-references against a fresh `scan_installed()` result, and returns the three buckets. Read-only, no side effects, safe to call repeatedly (e.g. if the user reopens the picker).
- Execution reuses the existing `run_op` command (`src-tauri/src/lib.rs:39-55`) unchanged, called once per will-install row from the frontend's queue loop; no new execution primitive needed.
- Frontend: new "Import library..." menu item, a preview modal component (new, but built from the existing modal/table chrome already used elsewhere), and a sequential queue-drain loop that is a small variant of what Update All already does.

Effort estimate: small-to-medium. The install and streaming path is fully reused. New work is the manifest parser and bucket classifier (a pure function, straightforward to unit test given `build_command`'s existing test style in `src-tauri/src/ops.rs:136-172` as a model), the preview modal UI, and the sequential-queue wiring. Rough sizing: half a day for the Rust-side `import_preview` command and its tests, half a day to a day for the preview modal and queue integration, plus the dialog-plugin decision (adds a dependency and a capabilities-file entry if the maintainer wants a native picker rather than drag-drop only).
