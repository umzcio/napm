# Plan 043: Cargo rows stop printing an unverified "Latest"

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- src-tauri/src/scan/cargo.rs frontend/index.html`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none (land after 038/039 if they are in flight; shares
  `frontend/index.html`)
- **Category**: bug (honest display)
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

The project's core rule is that no element implies data napm does not have.
`plans/README.md` recorded two milder instances of a "Latest" overclaim and
noted they were "worth one small plan together if a third instance appears".
The third instance has appeared (found in the 2026-09-16 audit):

1. `scan/cargo.rs:324-334` - git/path crates set `latest = installed`, so
   the Latest column prints the crate's own version as though the registry
   confirmed it.
2. `scan/cargo.rs:428-431` - for REGISTRY crates, a failed crates.io lookup
   (offline, crate unknown, network error) also sets `latest = installed`,
   so every cargo row claims "current" with its own version under Latest
   whenever the lookup fails. Offline, that is every cargo row.
3. `frontend/index.html:1007` - the library context menu's disabled
   "Up to date" label carries the same implication for an `ahead` row.

Every other ecosystem leaves `latest` empty when unresolved, and
`version::status_of` already maps empty latest to "current"
(`version.rs:111-117`), so the honest-empty behavior exists and is the
house pattern. The frontend already renders "—" for npx/manual/ahead rows
via `noLatest` (`index.html:533,548`); it just does not yet treat an empty
`latest` as no-latest.

## Current state

**Revision note (2026-09-16, after first dispatch STOPped):** the empty
`latest` sentinel is load-bearing in a way the original plan missed.
`resolve_latest` selects which rows to resolve against crates.io by
`r.latest.is_empty()` (`scan/cargo.rs:394`), so giving git/path rows an
empty `latest` would route them into registry lookups; a git/path crate
whose name also exists on crates.io would then show a false "update" for an
unrelated registry package (name shadowing). The fix must therefore thread
the parsed `CrateSource` into `resolve_latest` (rows and installs are
index-aligned at the call site, `scan/cargo.rs:498`) so selection is by
source, not by the sentinel. `CrateSource` is `Copy + PartialEq`
(`cargo.rs:21-31`), deliberately not an `InstalledTool` field, and
`CargoInstall.source` (`cargo.rs:36-45`) carries it.

- `src-tauri/src/scan/cargo.rs:324-334`:

  ```rust
  // Registry crates start with an empty `latest` sentinel, resolved by
  // `enrich` against crates.io. git/path crates have no registry "latest":
  // latest == installed makes `version::status_of` read "current" ...
  let latest = match c.source {
      CrateSource::Registry => String::new(),
      CrateSource::Git | CrateSource::Path | CrateSource::Other => c.version.clone(),
  };
  ```

- `src-tauri/src/scan/cargo.rs:390-399` — `resolve_latest(rows, cache_dir)`
  selects rows via `.filter(|(_, r)| r.latest.is_empty())`; called from
  `scan_cargo_with_bins` at `:498` as `resolve_latest(&mut rows, cache_dir)`,
  where `rows` is `installs.iter().map(...).collect()` (index-aligned with
  `installs`).
- `src-tauri/src/scan/cargo.rs:428-431`:

  ```rust
  for (j, latest) in results.into_iter().enumerate() {
      let row = &mut rows[idxs[j]];
      row.latest = latest.unwrap_or_else(|| row.installed.clone().unwrap_or_default());
  }
  ```

- `src-tauri/src/scan/version.rs:107-119` - `status_of`: `cmp("", installed)`
  is `Less`, and the empty-latest guard returns "current". So removing the
  self-assignment does not change any row's status. A test at
  `version.rs:303-311` pins cargo empty-latest -> "current".
- `frontend/index.html:533`: `var noLatest = npx || manual || ahead;` and
  `:548`: `${noLatest?"—":t.latest}` - an empty `latest` currently prints as
  an empty cell, which is why the frontend half of this plan exists.
- `frontend/index.html:1007` (may shift a few lines after plan 040):
  `: {label: npx?"npx tool (cached)":"Up to date", disabled:function(){return true;}},`
- Existing cargo tests live in `scan/cargo.rs`'s test module;
  `to_installed_tool_git_and_path_rows_have_latest_equal_installed`
  (`:593-603`) pins the old self-assignment and must be updated
  deliberately.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Tests     | `cd src-tauri && cargo test`              | all pass            |
| Lint      | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 |
| Format    | `cd src-tauri && cargo fmt --all -- --check` | exit 0 |
| App check | `npm run tauri dev`                       | cargo rows show "—" under Latest when unresolved |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/scan/cargo.rs`
- `frontend/index.html` (the `noLatest` expression and the menu label only)

**Out of scope** (do NOT touch):
- `src-tauri/src/scan/version.rs` - `status_of` already does the right
  thing; do not "improve" it here.
- npx's `latest == installed` pattern: npx rows are explicitly labeled
  "freshness unknown" and the drift hint (`npx_latest`) covers them; that
  is a documented honest-limit pattern, not this bug class.
- Any change to how crates.io lookups are fetched or cached.

## Git workflow

- Branch: `advisor/043-cargo-honest-latest`
- One commit; style: `fix(scan): ...`.

## Steps

### Step 1: cargo backend - select rows by source, and leave `latest` empty without registry truth

Coordinated edits in `src-tauri/src/scan/cargo.rs`:

1. `to_installed_tool` (`:324-334`): ALL sources get `String::new()` as the
   initial `latest`. Rewrite the comment: an empty `latest` reads "current"
   via `status_of`'s empty-latest rule and renders as "—", which is the
   honest state for a source with nothing to compare against; which rows get
   a registry lookup is decided by source in `resolve_latest`, not by this
   sentinel.
2. `resolve_latest` (`:390`): change the signature to
   `fn resolve_latest(rows: &mut [InstalledTool], installs: &[CargoInstall], cache_dir: &Path)`
   and select indices by source instead of sentinel:

   ```rust
   let idxs: Vec<usize> = rows
       .iter()
       .enumerate()
       .filter(|(i, r)| r.latest.is_empty() && installs[*i].source == CrateSource::Registry)
       .map(|(i, _)| i)
       .collect();
   ```

   Add a debug_assert that rows and installs are the same length (they are
   index-aligned by construction at the single call site). Update the
   doc comment: only registry-sourced rows are resolved, because a git/path
   crate whose name exists on crates.io would otherwise get a false
   "update" for an unrelated registry package (name shadowing).
3. Call site (`:498`): `resolve_latest(&mut rows, &installs, cache_dir);`
4. Keep the lookup-failure fix from the original plan: in the final loop
   (`:428-431`), on lookup failure leave the empty sentinel:

   ```rust
   for (j, latest) in results.into_iter().enumerate() {
       if let Some(l) = latest {
           rows[idxs[j]].latest = l;
       }
   }
   ```

5. Tests: update `to_installed_tool_git_and_path_rows_have_latest_equal_installed`
   (`:593-603`) to assert empty `latest` for git/path rows (rename it
   accordingly). Add a test for the source-based selection: build installs
   with mixed sources, and assert only Registry rows are selected. Since
   `resolve_latest` calls `registry::doc` directly (not injectable), extract
   the selection into a tiny pure helper (e.g.
   `fn registry_row_indices(rows: &[InstalledTool], installs: &[CargoInstall]) -> Vec<usize>`)
   and table-test THAT; do not add a network mock.

**Verify**: `cd src-tauri && cargo test scan::cargo` → pass.

### Step 2: Frontend - empty latest renders as "—", and the menu label is honest

- `:533`: `var noLatest = npx || manual || ahead || !t.latest;`
- `:1007`: give the disabled label the ahead variant:

  ```js
  : {label: npx?"npx tool (cached)":(st==="ahead"?"Up to date (registry lists nothing newer)":"Up to date"), disabled:function(){return true;}},
  ```

  (`st` is already in scope at `:1000`.) No em dashes in the label (repo
  rule); the parenthetical is the house style.

**Verify**: `grep -n '!t.latest' frontend/index.html` matches; manual:
`npm run tauri dev` with at least one cargo tool installed - a git/path
crate (or any cargo row while offline) shows "—" under Latest, keeps its
✓/current glyph, and offers no Update action.

### Step 3: Full gate

`cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check` → all exit 0.

## Test plan

- Update/extend cargo scan tests: git/path crates produce empty `latest`;
  a failed registry resolution leaves `latest` empty; a successful one sets
  it. Model on the existing `scan/cargo.rs` test module (table style,
  no network).
- Frontend: manual check only (repo convention).

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cd src-tauri && cargo test` exits 0
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` exit 0
- [ ] `grep -n "unwrap_or_else(|| row.installed" src-tauri/src/scan/cargo.rs` → zero matches
- [ ] `grep -n "c.version.clone()" src-tauri/src/scan/cargo.rs` → no longer assigned to `latest`
- [ ] `grep -n "npx || manual || ahead || !t.latest" frontend/index.html` matches
- [ ] Manual: unresolved cargo rows show "—" under Latest with no Update action
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- An existing test or frontend path turns out to DEPEND on cargo
  `latest == installed` (e.g. the drift hint or What's New scoping reads
  cargo latest in a way this plan did not anticipate). Grep for
  `.latest` uses on cargo rows before editing; report surprises.
- `status_of`'s empty-latest rule has changed (drift in `version.rs`).
- The frontend renders empty latest somewhere OTHER than the table cell
  (e.g. export, copy-details) in a way that reads badly; the export formats
  print whatever is in the field, which is fine, but report anything that
  would print "undefined".

## Maintenance notes

- The rule this plan enforces: `latest` contains a registry-confirmed
  version or nothing; presentation concerns ("current" glyph, "—" cell) are
  derived downstream, never encoded by self-assignment.
- If cargo ever gains a batch "outdated" source (cargo does not ship one
  today; resolution is per-crate via crates.io), revisit `resolve_latest`'s
  fan-out, not this sentinel logic.
- The parallel npm-side gap (unscoped bulk-downloads names interpolated
  unencoded, `search/npm.rs:111-112`) and the OSV id interpolation
  (`intel/osv.rs:193,230`) are recorded in `plans/README.md` as known,
  unplanned small hardening items; they are NOT this plan's scope.
