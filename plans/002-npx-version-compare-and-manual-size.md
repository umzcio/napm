# Plan 002: Fix npx version dedup (lexicographic compare) and blank manual-install sizes

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/scan/`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Two confirmed display-correctness bugs in the Shared Library, an app whose core promise (README) is "no fake data":

1. **npx rows can show an older version than the one npx will actually run.** The npx scanner walks every `~/.npm/_npx/<hash>/` cache dir; the same tool commonly appears in several. Dedup keeps "the greatest installed version" but compares `Option<String>` values, which is lexicographic: `"1.9.0" > "1.10.0"` is true. The losing (newer) row's publisher, size, description, and updated date are discarded too, and the frontend then shows a spurious "npx pulls vX on next run" drift hint against the wrong baseline.
2. **Manual (unmanaged) rows always show a blank Size.** The manual scanner calls `dir_size()` on the canonicalized executable *file*; `dir_size` starts with `read_dir(path)`, which fails on a regular file, returning 0, which `human_size` renders as `""`. The one source where the user has no package manager to ask is the one where napm silently reports nothing.

## Current state

- `src-tauri/src/scan/npx.rs:23-35` — the buggy dedup:
  ```rust
  pub fn dedup_npx(rows: Vec<InstalledTool>) -> Vec<InstalledTool> {
      let mut map: BTreeMap<String, InstalledTool> = BTreeMap::new();
      for row in rows {
          map.entry(row.pkg.clone())
              .and_modify(|e| {
                  if row.installed > e.installed {   // <-- Option<String> lexicographic compare
                      *e = row.clone();
                  }
              })
              .or_insert(row);
      }
      map.into_values().collect()
  }
  ```
  The existing test (`npx.rs`, in its `#[cfg(test)] mod tests`) covers `1.0.0` vs `1.2.0`, which happens to work under string ordering.
- `src-tauri/src/scan/manual.rs:192-193` — the size call inside the `$PATH` walk:
  ```rust
  let version = resolve_version(&real, home.as_deref());
  let size = super::size::human_size(super::size::dir_size(&real));
  ```
  where `real` is `std::fs::canonicalize` of an executable regular file (checked at `manual.rs:175`: `meta.is_file()`).
- `src-tauri/src/scan/size.rs:29-34` — `dir_size` returns 0 for a file:
  ```rust
  pub fn dir_size(path: &Path) -> u64 {
      let mut total = 0;
      let entries = match std::fs::read_dir(path) {
          Ok(e) => e,
          Err(_) => return 0,
      };
  ```
  and `size.rs:13-14`: `human_size(0)` returns `""` by design ("0 bytes renders empty").
- There is no version-comparison helper anywhere in the Rust crate (the frontend has one in JS; a later plan moves it to Rust — your new module here is the seed for that).
- Convention: tests live in a `#[cfg(test)] mod tests` block at the bottom of the same file — see `src-tauri/src/scan/size.rs:63-84` for the exemplar shape.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0, all tests pass (81 before this plan; more after) |
| Targeted | `cd src-tauri && cargo test scan::` | scan module tests pass |

## Scope

**In scope**:
- `src-tauri/src/scan/version.rs` (create)
- `src-tauri/src/scan/mod.rs` (add `pub mod version;` line only)
- `src-tauri/src/scan/npx.rs` (dedup fix + tests)
- `src-tauri/src/scan/size.rs` (file-size branch + test)

**Out of scope**:
- `frontend/index.html` — the JS version logic is plan 006's job; do not touch it.
- `scan/brew.rs`, `scan/pip.rs`, `scan/npm.rs` — their version strings pass through verbatim; leave them.
- Any change to the `InstalledTool` struct shape.

## Git workflow

- Branch: `advisor/002-npx-version-and-manual-size`
- Commits: `fix(scan): numeric version compare in npx dedup` and `fix(scan): manual rows report the binary file size`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Create the version comparator module

Create `src-tauri/src/scan/version.rs` with a documented public function:

```rust
use std::cmp::Ordering;

/// Compare two version strings numerically. Split on '.', compare numeric
/// prefixes of each segment as integers; a segment with a non-numeric suffix
/// (prerelease like "0-rc1" or brew revision "3_1") sorts BELOW the same
/// numeric value without a suffix. Missing segments count as 0. Not full
/// semver; just strict enough that 1.10.0 > 1.9.0 and 1.0.0-rc1 < 1.0.0.
pub fn cmp(a: &str, b: &str) -> Ordering { ... }

/// Ordering over Option<&str>: None sorts below any Some.
pub fn cmp_opt(a: Option<&str>, b: Option<&str>) -> Ordering { ... }
```

Implementation guidance: split each version on `.`; for each segment, parse the leading digit run as `u64` (0 if none) and keep the remainder string; compare `(number, remainder_is_empty, remainder)` tuples — number first, then a segment with an empty remainder beats one with a non-empty remainder (so `1.0.0` > `1.0.0-rc1` when the suffix lands in the third segment), then the remainder lexicographically. Compare up to the longer of the two segment lists, treating missing segments as `(0, true, "")`.

Register the module in `src-tauri/src/scan/mod.rs` (add `pub mod version;` next to the other `pub mod` lines).

**Verify**: `cd src-tauri && cargo test scan::version` → new unit tests pass (write them in this step; cases listed in the Test plan).

### Step 2: Use it in the npx dedup

In `src-tauri/src/scan/npx.rs`, replace the `row.installed > e.installed` comparison with:

```rust
if super::version::cmp_opt(row.installed.as_deref(), e.installed.as_deref()) == std::cmp::Ordering::Greater {
    *e = row.clone();
}
```

**Verify**: `cd src-tauri && cargo test scan::npx` → all pass including the new two-digit case.

### Step 3: Make `dir_size` handle a file path

In `src-tauri/src/scan/size.rs`, at the top of `dir_size`, stat the path first:

```rust
if let Ok(meta) = std::fs::metadata(path) {
    if meta.is_file() {
        return meta.len();
    }
}
```

(Leave the existing `read_dir` flow for directories unchanged. Update the doc comment: it now sums a directory or returns a file's own length.)

**Verify**: `cd src-tauri && cargo test scan::size` → all pass including the new file-path test.

## Test plan

- In `scan/version.rs` `#[cfg(test)] mod tests`:
  - `cmp("1.10.0", "1.9.0")` → Greater (the bug)
  - `cmp("1.2.0", "1.10.0")` → Less
  - `cmp("1.0.0", "1.0.0")` → Equal
  - `cmp("1.0.0-rc1", "1.0.0")` → Less (prerelease)
  - `cmp("1.2.3_1", "1.2.3")` → Less (brew revision suffix sorts below per the chosen rule; if you decide revisions should sort ABOVE, document why in the code and adjust — but pick one and test it)
  - `cmp("2.0", "2.0.0")` → Equal
  - `cmp_opt(None, Some("0.0.1"))` → Less
- In `scan/npx.rs` tests: add a dedup case with rows at `1.9.0` and `1.10.0` for the same pkg → the `1.10.0` row (with its metadata) wins. Model after the existing dedup test in the same file.
- In `scan/size.rs` tests: write a temp file (use `std::env::temp_dir()` + a unique name, clean up after) with known byte length → `dir_size` returns that length; existing directory behavior unchanged.

**Verification**: `cd src-tauri && cargo test` → exit 0, all tests (81 + ~9 new) pass.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; new tests listed above exist and pass
- [ ] `grep -n "row.installed > e.installed" src-tauri/src/scan/npx.rs` → no matches
- [ ] `src-tauri/src/scan/version.rs` exists with `pub fn cmp`
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts above do not match the live code (drift).
- You find the frontend depends on the OLD dedup behavior in some way (it does not, per the audit, but if a manual app run shows npx rows disappearing entirely, stop).
- The prerelease ordering decision materially affects brew rows' `installed`/`latest` display (it should not — this plan only changes npx dedup and manual sizes).

## Maintenance notes

- `scan/version.rs` is deliberately the seed module for plan 006 (moving the frontend's `verCmp`/`bumpKind`/`statusOf` into Rust). Keep its comparison semantics documented; plan 006 extends it rather than re-implementing.
- Reviewer should scrutinize the prerelease/suffix ordering choice and the tuple-comparison edge cases (`2.0` vs `2.0.0`).
