# Plan 042: Intel cache hygiene - atomic writes everywhere, honest TTLs, one sanitizer

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- src-tauri/src/intel src-tauri/src/search/brew.rs src-tauri/src/lib.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/037-store-write-honesty.md (both touch
  `src-tauri/src/lib.rs`; land 037 first)
- **Category**: bug / tech-debt
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

Four findings in the registry/advisory cache layer:

1. **Torn-write races.** Three cache writers use plain `std::fs::write`
   while every other persistence point in the codebase deliberately uses
   temp-file+rename: `intel/wire.rs:165` (wire.json), `intel/release.rs:355`
   (hold cache), `intel/release.rs:439` (changelog cache). `get_whats_new`
   can run concurrently with itself and with `clear_caches`, so two writers
   can interleave. Readers degrade gracefully (parse failure → refetch), so
   the cost is wasted refetches, but it breaks the codebase's own invariant.
2. **Stale fallbacks restart the freshness clock.** `intel/registry.rs:140-143`
   inserts the stale disk body into memory with a fresh `Instant::now()`, so
   one network blip suppresses retries for the whole 1h TTL.
   `search/brew.rs:254-263` sets `loaded: SystemTime::now()` even when the
   bytes came from the stale-disk fallback, so a failed brew-catalog refetch
   reads as fresh for 24h.
3. **Changelog cache pins empty results forever.** `release.rs:434-448`
   caches any successful HTTP response permanently, including "no release
   matched the tag" - which happens for monorepo tag shapes
   (`pkg@1.2.3`), and for a release published AFTER napm first looked. The
   changelog pane then stays empty forever for that (pkg, version).
4. **Four drifted copies of the filename sanitizer.** `lib.rs:141` and
   `release.rs:292-294,370-372` replace `/`, `\`, `..` but not `@`;
   `registry.rs:55-57` also replaces `@`, and its own comment claims it
   "mirrors the pattern already used" in release.rs. They already disagree;
   the next cache feature copies whichever variant is nearest.

## Current state

- `src-tauri/src/intel/registry.rs:63-71` - `write_disk`, the tmp+rename
  exemplar to share:

  ```rust
  fn write_disk(path: &Path, body: &str) {
      let tmp = path.with_extension("json.tmp");
      if std::fs::write(&tmp, body).is_ok() {
          let _ = std::fs::rename(&tmp, path);
      }
  }
  ```

- `src-tauri/src/intel/registry.rs:133-143` - network arm with stale
  fallback; the `Err` arm's `memory_put` (which stamps `Instant::now()`,
  `:46-50`) is the TTL-reset bug:

  ```rust
  match fetch(&url) {
      Ok(body) => { write_disk(&path, &body); memory_put(&key, &body); Some(body) }
      Err(_) => std::fs::read_to_string(&path).ok().inspect(|body| {
          memory_put(&key, body);
      }),
  }
  ```

  `doc_with` (`:106-144`) takes an injectable `fetch`, so this logic is
  unit-testable without network; existing tests already use that seam.
- `src-tauri/src/search/brew.rs:121-149` - `cached_or_fetch` returns
  `Option<String>` with the stale fallback indistinguishable from a fresh
  fetch; `:254-263` stamps `loaded: SystemTime::now()` unconditionally.
- `src-tauri/src/intel/release.rs:364-449` - `changelog()`: permanent cache,
  "Caches empty results too, to avoid re-hitting a rate limit" (the
  rationale only applies to rate-limit protection; a short TTL for empties
  preserves it). `:280-362` - `velocity_verdict` and its hold-cache write at
  `:355`.
- Sanitizer copies: `lib.rs:141` (`filename.replace(['/', '\\'], "_").replace("..", "_")`),
  `release.rs:292-294` and `:370-372` (same, plus `@` on the pkg fragment
  only), `registry.rs:55-57` (`@` on everything).
- `src-tauri/src/store.rs:121-133` (`write_json`) is plan 037's scope; do
  not touch it here.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Tests     | `cd src-tauri && cargo test`              | all pass            |
| Lint      | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 |
| Format    | `cd src-tauri && cargo fmt --all -- --check` | exit 0 |
| App check | `npm run tauri dev`                       | What's New cards + changelogs + brew search still work |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/cache.rs` (create)
- `src-tauri/src/lib.rs` (register the module; swap `export_library`'s
  inline sanitize for the shared one)
- `src-tauri/src/intel/registry.rs`
- `src-tauri/src/intel/release.rs`
- `src-tauri/src/intel/wire.rs`
- `src-tauri/src/search/brew.rs`

**Out of scope** (do NOT touch):
- `src-tauri/src/store.rs` and `src-tauri/src/ops.rs` (plan 037).
- Cache TTL values other than the new empty-changelog TTL (1h); do not
  retune the 1h registry TTL or the 24h brew TTL.
- `frontend/index.html`.
- Do not consolidate the in-memory cache LAYERS (the DocMap, the brew
  CatalogCache): that consolidation was considered and rejected (each layer
  is individually tested; the churn is not worth it). This plan shares only
  the write and sanitize helpers.

## Git workflow

- Branch: `advisor/042-intel-cache-hygiene`
- Commit per step; style: `fix(cache): ...` / `refactor(cache): ...`.

## Steps

### Step 1: `cache.rs` - shared `write_atomic` and `sanitize_key`

Create `src-tauri/src/cache.rs`:

```rust
//! Shared helpers for the app's JSON cache files: one atomic write and one
//! filename sanitizer, so every cache layer follows the same rules instead
//! of drifting copies.
use std::path::Path;

/// Write via a sibling temp file plus rename, atomic on the same
/// filesystem: readers always see a complete old or new file, never a
/// partial write from a crash or a concurrent writer.
pub(crate) fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// Sanitize a key fragment for use in a cache filename: no path separators,
/// no traversal, no `@` (scoped npm names).
pub(crate) fn sanitize_key(s: &str) -> String {
    s.replace(['/', '@', '\\'], "_").replace("..", "_")
}
```

Register `mod cache;` in `src-tauri/src/lib.rs` (alphabetical position in
the module list at `:1-8`). Replace the four sanitizer copies
(`lib.rs:141`, `release.rs:292-294`, `release.rs:370-372`,
`registry.rs:55-57`) with `cache::sanitize_key`, deleting the local `fn
sanitize` in registry.rs. Note the deliberate behavior change: `@` is now
replaced in eco/version fragments too, and export filenames gain `@`
replacement - all outputs remain filename-safe, and old cache files whose
names differ are simply re-fetched once. Say that in the commit message.

**Verify**: `cargo build` compiles; `grep -rn 'replace(\[.., .,\]' src-tauri/src | grep -v cache.rs` → zero sanitizer copies remain.

### Step 2: Atomic writes at the three plain-write sites

- `wire.rs:165`: `let _ = std::fs::write(&cache_path, &text);` →
  `let _ = crate::cache::write_atomic(&cache_path, &text);`
- `release.rs:355` (hold cache) and `release.rs:439` (changelog cache): same
  swap.
- Optionally re-point `registry.rs::write_disk` and
  `search/brew.rs::write_cache_atomic` at the shared helper and delete the
  locals. (`brew.rs`'s version uses `.tmp` not `.json.tmp`; standardize on
  the shared helper's extension - stale `.tmp` litter is harmless.)

**Verify**: `grep -rn "std::fs::write" src-tauri/src/intel src-tauri/src/search` → only
callers of the shared helper remain (plus `lib.rs` export, which writes a
user artifact, not a cache, and stays as-is).

### Step 3: Stale fallbacks do not restart the clock

- `registry.rs`: in the `Err(_)` arm, return the stale body WITHOUT
  `memory_put`. Rationale comment: a stale fallback serves this call but
  must not suppress the next call's retry; without a memory entry the next
  call re-attempts the fetch (bounded by the 6s read timeout in `http.rs`),
  matching the no-cache-at-all behavior. Trade-off to note in the comment:
  during an outage each `doc()` call retries the network once, which is the
  intended asymmetry (fresh data recovers immediately when connectivity
  returns).
- `brew.rs`: make `cached_or_fetch` return freshness with the body -
  e.g. `Option<(String, bool)>` where the bool is true only for a fresh
  network fetch or an in-TTL disk hit, false for the stale fallback. In
  `load_catalog`, stamp `loaded` with `SystemTime::now()` only when fresh;
  on the stale fallback, stamp it from the file's mtime (or simply
  `SystemTime::now() - 24h`) so the next search refetches. Update the two
  call sites (`:243`, `:247`) and any tests accordingly.

**Verify**: `cargo test` green; then the new tests in step 5 pin this.

### Step 4: Empty changelog results get a 1h TTL

In `release.rs::changelog`, change the cache file format to:

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct ChangelogCache {
    checked_ts: i64,
    lines: Vec<String>,
}
```

Read path: parse `ChangelogCache` (legacy `Vec<String>` files fail to parse
and are treated as a miss - one refetch per old entry, acceptable; say so in
a comment). Hit rules: non-empty `lines` → permanent hit (unchanged
behavior); empty `lines` → hit only while `now - checked_ts < 3600`,
otherwise refetch. Write path: always write the new shape with the current
`now`. Extract the decision into a pure function for testing:

```rust
/// Whether a cached changelog entry counts as a hit. Non-empty results are
/// permanent (release notes for a published version do not change); empty
/// results expire after an hour so a release published after our first
/// lookup, or a tag shape we did not match, is retried.
fn changelog_cache_hit(lines: &[String], checked_ts: i64, now: i64) -> bool {
    !lines.is_empty() || now - checked_ts < 3600
}
```

**Verify**: `cargo build` compiles.

### Step 5: Tests

Following the existing seams (registry's injectable `doc_with`, table-style
module tests):

1. `registry.rs`: stale fallback does not suppress retry - prime a stale
   disk file (set mtime old via `filetime`-free trick: write the file, then
   ... if setting mtime is awkward without a dep, instead call `doc_with`
   with a failing fetch, assert the stale body returns; call again with a
   SUCCEEDING fetch and assert the fresh body returns and gets cached. The
   second call succeeding immediately is the regression assertion.)
2. `release.rs`: `changelog_cache_hit` table test - non-empty is a hit at
   any age; empty is a hit at 59 minutes, a miss at 61 minutes.
3. `cache.rs`: `sanitize_key` table test with `/`, `\`, `@`, `..`, `...`
   (`...` → `_​.`-style output; assert the exact string you implement);
   `write_atomic` round-trip in a tempdir.
4. `brew.rs`: if the freshness bool is testable without network (the
   function reads disk only until the fetch), add a test that a stale disk
   file + failed fetch reports `fresh == false`. `cached_or_fetch` calls
   `crate::http::get` directly, so if that is not injectable, extract the
   freshness decision the same way as step 4 and test the pure function;
   do NOT add a network mock framework.

**Verify**: `cargo test` → all pass, including the new tests.

### Step 6: Full gate + manual

`cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check` → all exit 0. `npm run tauri dev`: What's New loads, expanding
a verdict card loads its changelog, brew search returns results (the catalog
path), Swarm → Refresh registry caches works.

## Test plan

Covered in step 5. Patterns: `doc_with`'s injectable fetch
(`registry.rs:106-144`) and pure-function extraction for cache decisions.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cd src-tauri && cargo test` exits 0, including the new tests
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` exit 0
- [ ] `grep -rn "std::fs::write" src-tauri/src/intel src-tauri/src/search` → no direct cache writes remain
- [ ] `grep -rn "fn sanitize" src-tauri/src` → only the shared helper
- [ ] `grep -n "memory_put" src-tauri/src/intel/registry.rs` → no call in the `Err` arm of `doc_with`
- [ ] `grep -n "changelog_cache_hit" src-tauri/src/intel/release.rs` matches, with a table test
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live code (drift since `3df72f5`; plan 037
  landing first is expected and fine - it does not touch these files except
  `lib.rs` command signatures).
- The brew catalog's call sites turn out to need freshness for a third
  caller this plan did not list.
- Changing the changelog cache shape breaks an existing test that pins the
  old `Vec<String>` format; update that test deliberately and note it,
  rather than preserving the old format by accident.
- You find a FIFTH sanitizer copy not listed here (report it; do not silently
  extend scope - actually, if it is the same 3-line pattern, fixing it too
  is fine; report either way).

## Maintenance notes

- Any future cache file must use `cache::write_atomic` and
  `cache::sanitize_key`; a reviewer should reject new hand-rolled copies.
- The stale-fallback change means more network retries during outages (by
  design). If that ever shows up as log noise, the fix is a short negative
  TTL (e.g. retry at most once a minute), not restoring the full-TTL pin.
- The changelog TTL distinguishes "no notes exist" (permanent once found
  empty is no longer assumed) from "not yet"; tag-shape misses
  (`pkg@1.2.3`) now self-heal within an hour instead of never.
