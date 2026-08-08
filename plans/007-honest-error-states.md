# Plan 007: Surface backend failures honestly, fix response races, and give invoke() one seam

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- frontend/index.html src-tauri/src/lib.rs`
> If either file changed since this plan was written (plans 004/005 legitimately
> touch index.html — reconcile against their diffs), compare the "Current
> state" excerpts against the live code before proceeding; on any other
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none (composes with 004/005; execute after them if all are selected)
- **Category**: bug
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

napm's differentiator is honesty ("napm never implies safe when a check could not run" — README), and the codebase already has the right pattern in places: `SECURITY_OK`/`WIRE_OK` flags and a `detailFailed` state render explicit "check unavailable, this is not an all-clear" messages. But the three most common failures all render as confident wrong assertions: a failed `scan_installed` renders an empty library (indistinguishable from "you have no packages"), a failed `get_history` renders "No version changes yet.", and a failed changelog fetch renders "No changelog available." Eight further invoke calls are fire-and-forget with no feedback at all — worst is Export library, whose entire purpose is producing a file and which reports neither success, failure, nor the path. Finally, two response races let stale data overwrite fresh: search responses apply in network order (results for "http" can be labeled "httpie"), and a rescan during an in-flight What's New load is silently dropped, leaving pre-rescan verdicts marked as fresh.

## Current state

All frontend excerpts from `frontend/index.html`:

- `:869` — scan failure renders empty: `.catch(function(e){ console.error("scan_installed failed", e); renderRows(); ... })`
- `:694-700` — history failure renders the empty-state copy:
  ```js
  iv("get_history").then(...).catch(function(){ history=[]; renderHistory([]); });
  function renderHistory(hist){ if(!hist || !hist.length){ histWrap.innerHTML='<div class="empty">No version changes yet.</div>'; return; } ... }
  ```
- `:650-664` — changelog: the card sets `it.loaded=true` before the fetch; the catch just re-renders, and `renderFeed` (`:600-601`) prints "No changelog available." whenever `loaded` is true with an empty list. (The advisory path right above it does this correctly with `detailFailed` — copy that pattern.)
- `:428-440` — `runSearch` has no request sequencing; the `.then` unconditionally assigns `SWARM = results` and the `.catch` overwrites the results pane.
- `:520-553` — `loadWhatsNew` guards with a boolean: `if(LOADING_FEED) return;` — so `scanLibrary` (`:866`) setting `FEED_LOADED=false; loadWhatsNew();` while a load is in flight drops the reload, and the in-flight one then sets `FEED_LOADED=true` (`:551`) with pre-rescan data.
- `:913` — `transfer-done` sets `FEED_LOADED=false` but never re-renders the feed, so a user sitting on What's New keeps a stale "Get X" card and the tab badge keeps the old count.
- Fire-and-forget invoke sites: `:640` (`set_pin`, after an optimistic `t.pinned` flip at `:639`), `:948` (`export_library`), `:953` (`open_data_dir`), `:982` (`clear_caches`), `:628-629` (`open_external`, `reveal_in_finder`), `:750` (`run_op` — has its own event-driven feedback; leave it).
- The invoke-lookup expression `window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke` is inlined at `:432`, `:521`, `:652`, `:748`, `:861` even though the helper `inv()` exists at `:922`.
- Dead code (safe deletions, all verified unused): `HANDLES`/`ping()`/`handleFor()` (`:327`, `:333-334`); the duplicate final branch of `reBlurb` (`:559-560` — the last two branches return the same string).
- Backend: `export_library` (`src-tauri/src/lib.rs:103-114`) returns `()` and swallows the write result:
  ```rust
  #[tauri::command(async)]
  fn export_library(app: tauri::AppHandle, filename: String, content: String) {
      ...
      if std::fs::write(&path, content).is_ok() {
          let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
      }
  }
  ```
- There is a status bar the app renders into via `renderStatus()`; locate it with `grep -n "function renderStatus" frontend/index.html` and reuse its element for transient error text rather than inventing new chrome.
- UI copy rule: no em dashes in any user-facing string.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Backend tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | manual checklist below |

## Scope

**In scope**:
- `frontend/index.html` (the functions named above)
- `src-tauri/src/lib.rs` (`export_library` return type only)

**Out of scope**:
- `renderXfers`/transfer render internals (plan 011).
- The wire partial-failure semantics (plan 008 — backend).
- Any new UI chrome beyond reusing the status bar and existing empty-state/`signal` styles.

## Git workflow

- Branch: `advisor/007-honest-errors`
- Commits: `fix(ui): failed loads render as failures, not empty states`, `fix(ui): sequence search and feed responses`, `feat: export_library reports its path`, `chore(ui): one invoke seam, drop dead helpers`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: One invoke seam with an error surface

Add near `inv()` (`:922` — move it up next to `esc()` so it is defined before first use; function declarations hoist, so this is optional but tidy):

```js
// Single seam for backend calls. onErr (optional) runs on rejection AFTER the
// default surface: a transient status-bar message. Returns the promise.
function call(cmd, args, onErr){
  var i=inv();
  if(!i){ var p=Promise.reject(new Error("backend unavailable")); p.catch(function(){}); return p; }
  return i(cmd, args).catch(function(e){ flashStatus(String(cmd)+" failed: "+summarizeErr(e)); if(onErr) onErr(e); throw e; });
}
function summarizeErr(e){ return (e && (e.message||e.toString&&e.toString()) || "unknown error").slice(0,120); }
function flashStatus(msg){ /* write msg into the status bar element renderStatus uses; restore normal status after 6s via renderStatus() */ }
```

Convert the five inlined lookups (`:432`, `:521`, `:652`, `:748`, `:861`) and the `iv=inv()` sites to `call(...)` where a promise is consumed; keep raw `inv()` only where the code intentionally no-ops without a bridge (browser preview), preserving each site's existing no-bridge behavior.

### Step 2: Failure states for scan, history, changelog

- Scan: add `var SCAN_FAILED=false;` set true in the catch, false on success. In `renderRows`, when `SCAN_FAILED && !TOOLS.length`, render into `rowsEl` a full-width row using the existing `empty` class: "Could not scan your library. This is not an empty library. Use File, then Rescan now, to retry." (colon/comma phrasing, no em dashes).
- History: add `HISTORY_FAILED`; in `renderHistory`, when set, render "Could not load history. This is not an empty history." instead of the empty-state copy.
- Changelog: replace the pre-set `it.loaded=true` with success/failure marking: on success `it.loaded=true`, on catch `it.loadFailed=true`, and in `renderFeed` render "Changelog unavailable, could not reach the registry." when `loadFailed` (mirror the `detailFailed` branch at `:594`).

### Step 3: Sequence the racy loads

- Search: add `var searchSeq=0;` — `runSearch` does `var seq=++searchSeq;` and both `.then` and `.catch` bodies return early when `seq!==searchSeq`.
- Feed: replace the `LOADING_FEED` boolean with a generation counter: `var feedGen=0;` — `loadWhatsNew` does `var gen=++feedGen;` (no early-return guard) and its `.then`/`.catch` discard when `gen!==feedGen`. Delete `LOADING_FEED`.
- `transfer-done` (`:913`): after `FEED_LOADED=false`, if the What's New tab is the active view (check the `.view.active` dataset), call `loadWhatsNew()`.

### Step 4: Feedback for the write-shaped commands

- Backend: change `export_library` to return `Result<String, String>` — `Ok(path.display().to_string())` after a successful write (keep the Finder reveal), `Err` with a short reason otherwise. Tauri serializes Result into a resolved/rejected promise; the frontend signature does not change shape.
- Frontend `exportLibrary`: `call("export_library", ...).then(function(p){ flashStatus("Exported to "+p); })`.
- Pin toggle: on rejection, revert `t.pinned` and re-render (pass an `onErr` that flips it back).
- `open_data_dir`, `clear_caches`, `open_external`, `reveal_in_finder`: route through `call` (default error surface is enough).

### Step 5: Dead code removal

Delete `HANDLES`, `ping()`, `handleFor()`, and collapse the duplicate final `reBlurb` branch. (If plan 004 landed, the unreachable npx fallback in the old `cmd` construction is already gone; otherwise leave `cmd` alone — it is plan 016's.)

### Step 6: Manual verification

`npm run tauri dev` and walk:
1. Normal launch: library loads, no error text.
2. Kill the network (Wi-Fi off), Swarm → Refresh registry caches, then run a search: results pane shows the unreachable message; status bar flashes a failure; reconnect and search again works.
3. Type a query, press Enter twice quickly with different terms: the results always match the LAST submitted term's label.
4. What's New: rescan while the feed is loading (File → Rescan now immediately after switching to What's New): the feed that finally renders reflects the post-rescan library.
5. Export library (JSON): status bar shows "Exported to <path>" and Finder reveals the file.
6. Toggle a pin with the backend intact: works; (failure path is exercised by code review, no easy simulation).

**Verify**: all six as described; webview console free of unhandled rejection warnings.

## Test plan

Manual checklist above; backend: `cd src-tauri && cargo test` → exit 0 (the `export_library` change is signature-only; no existing test covers it — add none, it has no pure logic).

## Done criteria

- [ ] `grep -c "window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke" frontend/index.html` → 1 (only inside `inv()`)
- [ ] `grep -n "LOADING_FEED" frontend/index.html` → no matches
- [ ] `grep -n "No version changes yet" frontend/index.html` → still present (genuine empty state) but a distinct failure branch exists
- [ ] `grep -n "HANDLES\|handleFor\|function ping" frontend/index.html` → no matches
- [ ] `export_library` returns `Result<String, String>` in `src-tauri/src/lib.rs`
- [ ] Manual checklist passes; `cd src-tauri && cargo test` exits 0
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match (esp. if plans 004/005 moved these functions — reconcile line numbers first via the named functions, then proceed only if the logic matches).
- Changing `export_library`'s return type breaks the invoke at the call site in a way that needs a Tauri-version-specific fix.
- `renderStatus` proves not to own a reusable element (report; do not invent new chrome).

## Maintenance notes

- Every future invoke call should go through `call()`; raw `inv()` use in review is a smell unless the no-bridge no-op is intentional.
- Plan 008 (wire) adds a partial-feed indicator that composes with the `WIRE_OK` rendering here.
- Deferred deliberately: an offline banner (network-state detection), and surfacing store-file corruption (backend reads corrupt JSON as defaults; noted in plan 003's tests).
