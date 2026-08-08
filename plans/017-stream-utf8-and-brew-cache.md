# Plan 017: Lossy-decode streamed op output and single-flight the brew catalog cache

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/ops.rs src-tauri/src/search/brew.rs`
> Plans 003/013/016 legitimately touch ops.rs; reconcile against their diffs.
> Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S-M
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

1. **A single non-UTF-8 byte silently ends a transfer's log.** The op reader threads use `BufReader::lines().map_while(Result::ok)`, and `lines()` returns `Err` for invalid UTF-8 as well as IO errors — `map_while` then STOPS the iterator. A package manager emitting one mangled byte (locale issues, a postinstall script's binary noise) drops the entire remainder of that stream, including the error text of a failing install, while the exit code still reports confidently. The user sees "failed" with no reason. Bonus defect in the same loop: `\r`-progress output (npm/brew progress bars) arrives as one enormous line.
2. **The brew catalog (~10 MB) can be downloaded twice concurrently and a torn write is trusted for 24h.** `load_catalog` releases its lock BEFORE fetching, so the startup warm thread and a user's first search both miss and both download. The disk write is non-atomic (`fs::write`) and freshness is judged purely by mtime, so an interleaved/torn file counts as fresh for 24 hours; the in-memory guard refuses to cache an empty parse, which means a corrupt file re-runs the multi-MB parse and returns zero brew results on EVERY search for a day.

## Current state

- `src-tauri/src/ops.rs:101-119` — the reader threads:
  ```rust
  if let Some(pipe) = child.stdout.take() {
      ...
      handles.push(std::thread::spawn(move || {
          for line in BufReader::new(pipe).lines().map_while(Result::ok) {
              let _ = app2.emit("transfer-line", LineEvent { op_id: id2.clone(), stream: "stdout".into(), line });
          }
      }));
  }
  // stderr block identical at :111-119
  ```
- `src-tauri/src/search/brew.rs:84-112` — `cached_or_fetch`: mtime freshness check, `crate::http::get(url)`, best-effort `std::fs::write(path, &body)`, stale-fallback on fetch error.
- `src-tauri/src/search/brew.rs:124-145` — `catalog_cell()` (process-global `Mutex<Option<CatalogCache>>`) and `load_catalog`, which drops the guard at the end of the read block (`:134-145`) and then fetches unlocked.
- Concurrent callers: startup warm thread (`src-tauri/src/lib.rs:256-259`), post-clear re-warm (`lib.rs:182-186`), and any `search_registry` invoke (`lib.rs:58-66` → `search_all` → brew source thread).
- `invalidate_catalog()` exists in `brew.rs` (called by `clear_caches`, `lib.rs:170`) — your changes must keep it working.
- Convention: tests in-file; `brew.rs` has parser tests (`parse_catalog` area).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test ops:: && cd ../.. ; cd src-tauri && cargo test search::brew` | pass |
| All tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | search + transfers work |

## Scope

**In scope**:
- `src-tauri/src/ops.rs` (reader loop only)
- `src-tauri/src/search/brew.rs` (`cached_or_fetch`, `load_catalog`)

**Out of scope**:
- Event shapes, `renderXfers` (plan 011).
- The 24h TTL values.
- `search/npm.rs`, `search/pip.rs`.

## Git workflow

- Branch: `advisor/017-stream-and-brew-cache`
- Commits: `fix(ops): lossy UTF-8 decoding and CR splitting in streamed output` and `fix(search): single-flight brew catalog fetch with atomic cache writes`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Byte-oriented reader with lossy decoding

Extract a helper in `ops.rs` and use it in both reader threads:

```rust
/// Read a child pipe to EOF, emitting one event per line. Bytes are decoded
/// lossily (invalid UTF-8 becomes U+FFFD) so a stray byte never truncates the
/// stream. Splits on \n and also treats \r as a line break so progress-bar
/// rewrites arrive as lines instead of one giant blob.
fn stream_lines<R: std::io::Read>(pipe: R, mut emit: impl FnMut(String)) { ... }
```

Implementation: `BufReader::read_until(b'\n', &mut buf)` loop until 0 bytes; for each chunk, trim the trailing `\n`/`\r\n`, then `split('\r')` the lossy-decoded string and emit each non-empty piece. The reader threads become `stream_lines(pipe, |line| { let _ = app2.emit(...) })`.

**Verify**: `cd src-tauri && cargo test ops::` → new `stream_lines` tests pass (Test plan).

### Step 2: Atomic cache write + corrupt-file recovery

In `cached_or_fetch`:
- Write via temp+rename: `let tmp = path.with_extension("tmp"); fs::write(&tmp, &body)` then `fs::rename(&tmp, path)`.
- Add a validity check on the fresh-cache path: the caller (`load_catalog`) already detects an unusable body via an empty `parse_catalog` result — plumb that back by having `load_catalog`, when a "fresh" disk body parses to zero formulae, DELETE the cache file and retry the fetch once. (Keep `cached_or_fetch` dumb about JSON; the retry loop lives in `load_catalog`.)

### Step 3: Single-flight the fetch

In `brew.rs`, add a fetch gate so only one thread downloads/parses:

```rust
fn fetch_gate() -> &'static Mutex<()> { static G: OnceLock<Mutex<()>> = OnceLock::new(); ... }
```

`load_catalog` flow becomes: (1) read `catalog_cell` under its lock — fresh hit returns as today; (2) acquire `fetch_gate` lock; (3) RE-CHECK `catalog_cell` (another thread may have filled it while you waited) — hit returns; (4) fetch + parse + store into `catalog_cell`; release. The gate is held across the network fetch by design — that serializes cold-start brew searches behind one download, which is the desired behavior (the second caller wants the same bytes).

**Verify**: `cd src-tauri && cargo test search::brew` → existing parser tests plus new tests pass. App run: launch and immediately search a brew formula — results appear; Swarm → Refresh registry caches → search again works (re-warm + single flight coexist).

## Test plan

- `ops.rs` `stream_lines`:
  - plain lines round-trip
  - input containing an invalid UTF-8 byte mid-stream: output contains U+FFFD and CONTINUES to later lines (the regression this fixes)
  - `\r`-separated progress input yields multiple lines
  - no trailing newline: final partial line still emitted
- `brew.rs`:
  - temp+rename: after `cached_or_fetch`-style write, no `.tmp` file remains alongside the target (test the write helper directly with a temp dir)
  - corrupt fresh file: seed a fresh-mtime garbage file, stub the fetch (structure `load_catalog`'s retry so the fetch call is injectable, or test the delete-and-refetch decision as a pure function) → the garbage file is removed
  - single-flight: two threads calling a gated closure that counts invocations → the underlying fetch closure runs once (test the gate + re-check pattern with a counter, no network)

**Verification**: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; `stream_lines` and brew-cache tests exist and pass
- [ ] `grep -n "map_while(Result::ok)" src-tauri/src/ops.rs` → no matches
- [ ] `grep -n "fs::write(path" src-tauri/src/search/brew.rs` → cache writes go through temp+rename
- [ ] Single-flight gate with re-check present in `load_catalog`
- [ ] App run: brew search works cold and after cache refresh
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond the named plans' expected diffs.
- Holding the fetch gate across the download deadlocks with `catalog_cell`'s mutex in any path (`invalidate_catalog` in particular — check its lock ordering; if both locks are ever held, the order must be gate → cell, everywhere).
- The `\r` splitting floods the UI with progress lines in a real brew install (hundreds of rewrites): if observed, coalesce consecutive `\r` pieces to the LAST piece per read chunk and note the choice.

## Maintenance notes

- Plan 011 caps per-op retained lines; with `\r` splitting producing more lines, that cap matters more — land 011 alongside or before heavy use.
- The single-flight gate pattern is the same one plan 010's registry cache can adopt if its thundering-herd ever matters (per-package docs are small; it does not today).
- Reviewer: lock-ordering between `fetch_gate` and `catalog_cell`, and the retry-once bound in the corrupt-file path (must not loop).
