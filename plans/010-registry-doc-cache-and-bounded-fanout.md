# Plan 010: Cache registry documents and bound the What's New thread fan-out

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/intel/ src-tauri/src/lib.rs src-tauri/src/http.rs`
> If these changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it
> as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW (additive cache layer; staleness bounded by TTL)
- **Depends on**: none
- **Category**: perf
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

The full npm packument (`https://registry.npmjs.org/<pkg>` — every version and its metadata, megabytes for popular CLIs) is downloaded up to three times for the same package with no cache anywhere: once per feed refresh by `release_verdict` (to read one publish timestamp), once per library render by `npx_latest` (to read `dist-tags.latest` — and that path fetches SEQUENTIALLY, one package at a time), and once more per changelog expand (to read `repository.url`). The PyPI JSON doc has the same double-fetch shape. Separately, `whats_new` spawns one OS thread per in-scope package with no cap — a user with 40 outdated tools fires 40 simultaneous threads each holding 1-3 blocking HTTP calls, thrashing the shared connection pool and inviting GitHub secondary rate limits, which then degrade every verdict to "new".

## Current state

- `src-tauri/src/http.rs` — the entire HTTP layer (73 lines): one shared `ureq` agent, `get`, `post_json`, `get_with_headers`, `encode`. No caching of any kind.
- The three packument fetch sites:
  - `src-tauri/src/intel/release.rs:169-183` (`release_verdict`): `let url = format!("https://registry.npmjs.org/{}", crate::http::encode(pkg)); let body = crate::http::get(&url).ok();` (npm arm; pip arm fetches `https://pypi.org/pypi/<pkg>/json`)
  - `src-tauri/src/intel/release.rs:282-300` (`changelog`): same two URLs re-fetched to extract `repository.url` / `project_urls`
  - `src-tauri/src/lib.rs:140-150` (`npx_latest`): same npm URL per package, sequential `filter_map`
- The unbounded fan-out, `src-tauri/src/intel/mod.rs:103-114`:
  ```rust
  std::thread::scope(|inner| {
      let handles: Vec<_> = scope_tools.iter().map(|t| {
          ...
          inner.spawn(move || -> ReleaseInfo { ... release::release_verdict(...) ... })
      }).collect();
      handles.into_iter().filter_map(|h| h.join().ok()).collect::<Vec<_>>()
  })
  ```
- `npx_latest` currently has NO access to the app-data dir (its command signature is `fn npx_latest(pkgs: Vec<String>)`); adding `app: tauri::AppHandle` as the first parameter is invisible to the frontend (Tauri injects it; the JS call at `frontend/index.html:877` passes only `{pkgs}` and needs no change).
- Cache-dir conventions to follow: `intel/release.rs:222-232` (hold cache) and `:266-279` (changelog cache) — sanitized key fragments, `serde_json` payloads, files directly in the app-data dir. `clear_caches` (`src-tauri/src/lib.rs:162-187`) removes named cache files and prefixed families (`changelog_*.json`, `hold_*.json`) — your new cache family must be added there.
- Existing bounded-fan-out exemplar: `search/npm.rs` bounds its scoped-download fan-out at 25 (per the audit; verify with `grep -n "25" src-tauri/src/search/npm.rs`).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | What's New + npx hints work; second refresh visibly faster |

## Scope

**In scope**:
- `src-tauri/src/intel/registry.rs` (create — the doc cache)
- `src-tauri/src/intel/mod.rs` (bounded fan-out; `pub mod registry;`)
- `src-tauri/src/intel/release.rs` (use the cache)
- `src-tauri/src/lib.rs` (`npx_latest` signature + cache + parallelism; `clear_caches` addition)

**Out of scope**:
- `http.rs` internals (keep the cache a layer above it, not inside it — `search/` deliberately has its own cache strategy).
- The brew catalog cache (plan 017).
- Changing verdict logic or output shapes.

## Git workflow

- Branch: `advisor/010-registry-cache`
- Commits: `perf(intel): shared TTL cache for registry documents`, `perf(intel): bound the verdict fan-out`, `perf: npx_latest cached and parallel`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: The registry-doc cache module

Create `src-tauri/src/intel/registry.rs`:

```rust
//! Short-TTL cache for registry documents (npm packuments, PyPI JSON docs).
//! Memory layer: process-global map. Disk layer: regdoc_<eco>_<pkg>.json in the
//! app-data dir, freshness by file mtime. TTL 1h: long enough to dedupe the
//! 2-3 fetches per package per session, short enough that a just-published
//! release shows up on the next feed refresh.

/// The registry document for (eco, pkg), from cache or network. None when the
/// fetch fails and no cached copy exists. eco: "npm"/"npx" -> packument,
/// "pip" -> PyPI JSON. Other ecosystems: None.
pub fn doc(eco: &str, pkg: &str, cache_dir: &Path) -> Option<String>
```

Implementation: sanitize the key exactly like `release.rs:269-271` does (`replace(['/', '@', '\\'], "_").replace("..", "_")`); memory layer is `static DOCS: OnceLock<Mutex<HashMap<(String,String),(Instant,Arc<String>)>>>`; check memory (fresh) → disk (mtime fresh) → fetch via `crate::http::get` → write disk via temp+rename (copy the pattern from plan 003's `write_json` if landed, else write it here) and populate memory. On fetch failure, fall back to a stale disk copy if present (stale beats nothing for verdicts; the OSV security path is unaffected by this module).

### Step 2: Route the three fetch sites through it

- `release_verdict` npm/pip arms: `let body = registry::doc(eco, pkg, cache_dir);`
- `changelog` repo-url derivation: same call (it already receives `cache_dir`).
- `npx_latest` in `lib.rs`: add the `app: tauri::AppHandle` parameter, resolve the app-data dir (same 3 lines as `get_changelog` at `lib.rs:78-81`), and use `registry::doc("npm", &pkg, &dir)`.

**Verify**: `cd src-tauri && cargo test` → exit 0; `grep -n "registry.npmjs.org" src-tauri/src` → the raw URL now appears only inside `intel/registry.rs` (and `search/npm.rs`, which is out of scope and keeps its own).

### Step 3: Bound the fan-outs

- In `intel/mod.rs`, replace the per-item spawn with a fixed worker pool over chunks: compute `let workers = scope_tools.len().min(8);`, split the indexed work via an `AtomicUsize` cursor or `chunks()` per worker, each worker draining its share and pushing `ReleaseInfo` results; collect and re-order is unnecessary (order of `verdicts` is not meaningful to the frontend — it re-keys by pkg/eco — but preserve input order anyway by writing results into a pre-sized `Vec<Option<ReleaseInfo>>` by index, then flattening, to keep behavior identical).
- In `npx_latest`, same pattern with `min(8)` workers over the pkg list, each using the cache from Step 2.

**Verify**: `cd src-tauri && cargo test` → exit 0.

### Step 4: `clear_caches` covers the new family

Add the `regdoc_` prefix to the prefixed-family sweep in `clear_caches` (`lib.rs:172-180`), alongside `changelog_`/`hold_`, and clear the in-memory map (expose `registry::invalidate()` mirroring `search::brew::invalidate_catalog()` at `lib.rs:170`).

**Verify**: `grep -n "regdoc_" src-tauri/src/lib.rs src-tauri/src/intel/registry.rs` → present in both.

### Step 5: Manual verification

`npm run tauri dev`: open What's New (feed loads); refresh it again within a minute — the second load is visibly faster and the app-data dir (File → Open data folder) contains `regdoc_*.json` files. npx rows still show correct drift hints. Swarm → Refresh registry caches removes the `regdoc_*.json` files.

## Test plan

In `intel/registry.rs` tests (temp dir):
- disk hit: pre-write a fresh `regdoc_npm_x.json`, `doc()` returns it without network (network failure path: use an eco the fetch arm rejects, or structure `doc` so the fetch fn is a parameter injectable in tests — prefer the latter: `fn doc_with(fetch: impl Fn(&str) -> Result<String,String>, ...)` with `doc` as the thin production wrapper)
- stale fallback: stale file + failing fetch → returns stale content
- miss + failing fetch + no file → None
- key sanitization: `@scope/pkg` produces a path-safe filename (no `/` in the file name)
- Chunking: a unit test on the worker-split helper (n items, ≤8 workers, all indexes covered exactly once)

**Verification**: `cd src-tauri && cargo test` → exit 0, new tests pass.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; registry cache tests exist and pass
- [ ] `release_verdict`, `changelog`, and `npx_latest` all obtain registry docs via `intel::registry::doc`
- [ ] `intel/mod.rs` verdict fan-out and `npx_latest` are bounded at ≤8 concurrent workers
- [ ] `clear_caches` removes `regdoc_*.json` and invalidates the memory layer
- [ ] Manual verification (Step 5) passes
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live code.
- `npx_latest`'s signature change breaks the frontend call (it must not — Tauri injects `AppHandle`; if the invoke errors, report the Tauri behavior rather than passing the dir from JS).
- Verdicts change for the same package/version between a cached and uncached run (they must not; the doc content is identical — a difference means the stale-fallback is serving something too old, report it).

## Maintenance notes

- The 1h TTL is the freshness ceiling for "released N days ago" labels and npx drift hints; if users report a just-published release not appearing, this is the knob.
- `search/npm.rs` keeps its own fetch path by design (search hits a different endpoint); do not unify them without a reason.
- Reviewer: watch for the memory map growing unboundedly in a long session (hundreds of packages × Arc'd megabyte strings — acceptable today; an LRU is the follow-up if RSS becomes a complaint).
