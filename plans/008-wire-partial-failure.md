# Plan 008: Stop presenting a half-failed supply-chain wire as a complete feed

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/intel/wire.rs src-tauri/src/intel/mod.rs frontend/index.html`
> If the wire code changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

The supply-chain wire fetches recent npm and pip malware advisories with two independent GitHub API calls. When BOTH fail it correctly falls back to the stale cache. But when exactly one fails (the common case: a rate-limited 403 on one call), the code merges whatever succeeded, **writes that partial list into the 1-hour disk cache**, and returns it as a successful complete feed (`wire_ok = true`). A transient failure on the npm call therefore produces a pip-only wire presented as complete, and pins that truncated view for the next hour. This directly contradicts the project's core rule, stated in the README and enforced correctly by the OSV path in this same module: never imply a clean or complete result from a check that did not run.

## Current state

- `src-tauri/src/intel/wire.rs:65-101` — the flow:
  ```rust
  let npm_result = crate::http::get_with_headers(npm_url, &base_headers);
  let pip_result = crate::http::get_with_headers(pip_url, &base_headers);

  let npm_ok = npm_result.is_ok();
  let pip_ok = pip_result.is_ok();

  if !npm_ok && !pip_ok {
      // Both failed: return stale cache if available, else None.
      return std::fs::read_to_string(&cache_path)
          .ok()
          .and_then(|t| serde_json::from_str::<Vec<WireItem>>(&t).ok());
  }

  let mut merged: Vec<WireItem> = Vec::new();
  if let Ok(body) = npm_result { merged.extend(parse_advisories(&body, "npm")); }
  if let Ok(body) = pip_result { merged.extend(parse_advisories(&body, "pip")); }
  // ... sort by published desc, truncate(15) ...
  if let Ok(text) = serde_json::to_string(&merged) {
      let _ = std::fs::write(&cache_path, &text);
  }
  Some(merged)
  ```
  Earlier in the same function (around `:30-50`) a fresh (<1h) cache is returned directly; `WireItem` is defined in `intel/mod.rs:33-42` and carries an `eco` field ("npm" | "pip").
- `src-tauri/src/intel/mod.rs:132-135` — `fetch_wire`'s `Option` is mapped to the flag the frontend trusts:
  ```rust
  let (wire, wire_ok) = match wir {
      Some(w) => (w, true),
      None => (Vec::new(), false),
  };
  ```
- `frontend/index.html:566-574` — rendering: the "wire unavailable" note only appears when the wire list is EMPTY and `WIRE_OK` is false; a non-empty partial list with `wire_ok=false` would today render with no caveat:
  ```js
  if(WIRE.length){ ...render items, no caveat... }
  else if(!WIRE_OK){ ...'wire unavailable'... }
  ```
- Existing tests live in `wire.rs`'s `#[cfg(test)] mod tests` (parsers). Convention: pure helper + tests in the same file.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test intel::wire` | pass |
| All tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | What's New renders the wire |

## Scope

**In scope**:
- `src-tauri/src/intel/wire.rs`
- `src-tauri/src/intel/mod.rs` (only the `fetch_wire` return-type plumbing)
- `frontend/index.html` (only the `renderFeed` wire branch)

**Out of scope**:
- The OSV path (`intel/osv.rs`) — already correct.
- The wire's fetch URLs, parsing, sorting, or 15-item cap.
- The 1h cache TTL.

## Git workflow

- Branch: `advisor/008-wire-honesty`
- Commit: `fix(intel): partial wire fetch is not cached or shown as complete`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Restructure `fetch_wire` to report completeness

Change `fetch_wire`'s return type from `Option<Vec<WireItem>>` to `Option<(Vec<WireItem>, bool)>` where the bool is `complete` (both sources fetched fresh). Extract the merge decision into a pure function so it is testable:

```rust
/// Merge per-source fetch results with the cached items. For a source that
/// failed, fall back to that source's items from `cached` (stale is better
/// than silently absent). `complete` is true only when both fetches succeeded.
fn merge_wire(
    npm: Option<Vec<WireItem>>,   // None = fetch failed
    pip: Option<Vec<WireItem>>,
    cached: &[WireItem],
) -> (Vec<WireItem>, bool) { ... }
```

Rules: for each source, use fresh items when `Some`, else `cached.iter().filter(|w| w.eco == <that source>)`. Sort and truncate as today. `complete = npm.is_some() && pip.is_some()`.

Cache policy: write `cache_path` ONLY when `complete` is true. (A partial must not poison the cache; the stale cache is what backfills the failed source next time.)

Both-failed still returns the stale cache when present, now as `Some((cached_items, false))`, or `None` when no cache exists.

### Step 2: Plumb the flag through `whats_new`

In `intel/mod.rs`, the match becomes:

```rust
let (wire, wire_ok) = match wir {
    Some((w, complete)) => (w, complete),
    None => (Vec::new(), false),
};
```

(`wire_ok` now means "the wire is complete and fresh", which is what the frontend already assumes it means.)

### Step 3: Render the partial case

In `frontend/index.html` `renderFeed` (`:566-574`), when `WIRE.length && !WIRE_OK`, append after the items one muted wire-item line: "wire incomplete: one source could not be reached, showing what is known". Keep the existing empty+failed branch. No em dashes in the copy.

**Verify**: `npm run tauri dev` → What's New renders the wire normally when online.

## Test plan

In `wire.rs` tests (model after the existing parser tests; `WireItem` literals are easy to construct):
- both fresh → all items, `complete == true`
- npm failed, pip fresh, cache has old npm items → merged contains cached npm + fresh pip, `complete == false`
- npm failed, pip fresh, empty cache → pip only, `complete == false`
- both failed handled by caller (both-failed path returns stale cache with `false` — cover via `merge_wire(None, None, cached)` if you route that path through the helper too)
- ordering/truncation preserved (published-desc, max 15)

**Verification**: `cd src-tauri && cargo test` → exit 0, new tests pass.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; `merge_wire` tests exist and pass
- [ ] `grep -n "fn merge_wire" src-tauri/src/intel/wire.rs` → present
- [ ] Cache write is inside a `complete`-only branch (read the diff)
- [ ] Frontend renders a partial-wire caveat when `WIRE.length && !WIRE_OK`
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live code.
- `fetch_wire` has callers other than `intel/mod.rs::whats_new` (`grep -rn "fetch_wire" src-tauri/src` first).
- The fresh-cache early-return path (top of `fetch_wire`) cannot distinguish a previously-cached-partial file — with the Step 1 cache policy it cannot occur for NEW caches, but if you find an existing mechanism recording partiality in the cache file, reconcile rather than invent a second format.

## Maintenance notes

- If a third ecosystem joins the wire (e.g. crates.io via plan 022), `merge_wire` generalizes to a per-source map; keep the "complete = all sources fresh, cache only when complete" rule.
- Reviewer: check the cache file written by a complete fetch is byte-compatible with the old format (it is a plain `Vec<WireItem>` — do not serialize the tuple into the cache).
