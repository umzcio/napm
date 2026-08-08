# Plan 011: Incremental transfer rendering, cheap row selection, and no redundant search refetch

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- frontend/index.html`
> Plans 004/005/007 legitimately touch this file; reconcile against their
> diffs (this plan assumes 004's identity-keyed transfer records if it landed).
> Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (DOM restructuring in the transfer pane; verified only by running the app)
- **Depends on**: plans/004-frontend-transfer-identity.md (soft: land 004 first so records are identity-keyed)
- **Category**: perf
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Three quadratic-or-redundant render paths:

1. **Transfers re-render everything per streamed line.** The `transfer-line` listener pushes one line and calls `renderXfers()`, which clears the container and rebuilds EVERY transfer row, re-escaping and re-joining each row's ENTIRE accumulated log. A 500-line brew install performs on the order of 125,000 line escapes and 500 full subtree teardowns; Update All multiplies that by the number of concurrent rows — exactly when the UI must stay responsive. `xfers` is also unbounded: nothing ever trims completed transfers or their logs.
2. **Selecting a library row rebuilds the whole table.** Each row gets its own click listener whose body is `selected=i; renderRows();` — with 100+ brew formulae, one click tears down and rebuilds ~100 rows with ~900 innerHTML-parsed cells, purely to move a `sel` class. Dragging the appetite dial also calls `renderRows()` per `input` event (that one is legitimate — classification changes — but it makes the per-render cost matter more).
3. **Clicking the Search tab re-fires the network search** even when the query is unchanged and results are already loaded.

## Current state

All excerpts from `frontend/index.html`:

- `:903-906` — the per-line full re-render:
  ```js
  ev.listen("transfer-line",function(e){
    var x=xfers.find(function(z){return z.opId===e.payload.op_id;});
    if(x){ x.lines.push({stream:e.payload.stream,line:e.payload.line}); renderXfers(); }
  });
  ```
- `:677-693` — `renderXfers` clears `xferListEl` and rebuilds all rows; each row joins its whole `lines` array (`esc()` per line) into the row HTML; the active count updates the tab badge at `:691-692`:
  ```js
  var active=xfers.filter(function(x){return !x.done;}).length;
  badge.textContent=active; badge.style.display=active?"":"none";
  ```
- `:747` — `xfers.unshift(x); renderXfers();` — new transfers go to the front; nothing caps the array.
- `:394-411` — `renderRows` builds a `<tr>` per display row with `tr.dataset.i=i`, sets `sel` class from `selected===i`, and attaches a per-row listener:
  ```js
  tr.addEventListener("click",function(){selected=i; renderRows();});
  ```
  A delegated click handler ALREADY exists on `rowsEl` (`:777-782`) handling `[data-pin]` and `[data-get]` — selection belongs there too.
- `:833` — the tab handler refires search: `if(t.dataset.view==="search") runSearch(lastQuery); else ...`
- `:428-440` — `runSearch(q)` sets `lastQuery=q` and invokes `search_registry` unconditionally for a non-empty q. `SWARM` holds the last results; `renderSearchResults()` re-renders from `SWARM`.
- Selection is also written by `histMenu`'s "Jump to tool in library" (`:729`: `selected=c; switchTab("library"); renderRows();`) and reset by `scanLibrary` (`:865`: `selected=null`).
- If plan 004 landed, transfer records are `{opId, pkg, eco, name, user, cmd, lines, done, ok, action, from, to}`; if not, they also carry `ti` — either way this plan only changes HOW they render.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Backend regression | `cd src-tauri && cargo test` | exit 0 (should be untouched) |
| Run the app | `npm run tauri dev` | checklist below |

## Scope

**In scope**:
- `frontend/index.html`: `renderXfers`, the `transfer-line`/`transfer-done` listeners, `queueTransfer`'s render call, `renderRows`'s selection wiring, the `rowsEl` delegated handler, the tab-click handler.

**Out of scope**:
- Any Rust file.
- The event payload shapes (`op_id`, `stream`, `line`).
- Virtualizing the library table (100-300 rows renders fine when not rebuilt per click; do not add a virtual list).
- `renderFeed`, menus, modals.

## Git workflow

- Branch: `advisor/011-render-perf`
- Commits: `perf(ui): incremental transfer log rendering`, `perf(ui): row selection without full re-render`, `perf(ui): search tab reuses loaded results`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Incremental transfer rendering

Restructure so each transfer owns a persistent DOM node:

- `renderXfers()` becomes the FULL rebuild used only on structural changes (new transfer queued, transfer completed, empty state). Give each row's root `data-op="<opId>"` and its log container class `xout` (it exists today — confirm with `grep -n "xout" frontend/index.html` and reuse the class/markup exactly so CSS is untouched).
- `transfer-line` handler: push the line, cap the array (`if(x.lines.length>500){ x.lines.shift(); x.trimmed=true; }`), then find `xferListEl.querySelector('[data-op="'+x.opId+'"] .xout')`; if found, append ONE new element for the line (`document.createElement("div")` + `textContent` — textContent needs no escaping) and, when the pane was scrolled to bottom, keep it pinned to bottom; if not found (row not yet in DOM), fall back to `renderXfers()`.
- When `x.trimmed`, the full rebuild path renders a leading muted line: "earlier output trimmed". Live-append path does not need to show it mid-stream.
- `transfer-done` handler: keep calling `renderXfers()` (structural change: status glyph/exit code) — that is fine, it is once per op, not per line.
- Cap retention: in `queueTransfer` after `unshift`, drop completed transfers beyond the 50 most recent (`xfers = xfers.filter(...)` keeping all active + newest 50 done).

**Verify** (app run): a brew or npm install streams smoothly with the log growing in place; scroll position sticks to bottom while streaming; after completion the row shows the exit state; >500-line logs show the trim notice on tab revisit.

### Step 2: Delegated row selection

- Remove the per-row `tr.addEventListener("click", ...)` at `:410`.
- In the existing `rowsEl` delegated click handler (`:777`), after the `[data-pin]`/`[data-get]` branches, add: resolve `var tr=e.target.closest("tr[data-i]"); if(tr){ var i=+tr.dataset.i; if(selected!==i){ var prev=rowsEl.querySelector("tr.sel"); if(prev) prev.classList.remove("sel"); selected=i; tr.classList.add("sel"); } }`.
- `renderRows` keeps setting the `sel` class during full rebuilds (unchanged), so external writers (`histMenu` jump, rescans) still work.

**Verify** (app run): clicking rows moves the highlight instantly; pin and Get buttons still work (their branches `return` before selection); Edit → Copy tool details still copies the selected row; appetite dial drag still re-classifies live.

### Step 3: Search tab reuse

In the tab handler (`:833`), change the search branch to skip the refetch when the query is unchanged and results exist: `if(t.dataset.view==="search"){ if(lastQuery && SWARM.length){ switchTab("search"); renderSearchResults(); } else runSearch(lastQuery); }`. (An empty `lastQuery` keeps today's behavior: `runSearch("")` renders the search-prompt empty state.)

**Verify** (app run): search for something, switch to Library, switch back to Search — results render instantly with no "Searching the swarm" flash; pressing Enter in the box still re-queries.

## Test plan

Manual (no JS harness): the three Verify blocks above, plus a stress pass — queue Update All with several outdated tools and interact with the library (select rows, switch tabs) while transfers stream; the UI must stay responsive. Backend: `cd src-tauri && cargo test` → exit 0, untouched.

## Done criteria

- [ ] `transfer-line` handler contains no `renderXfers()` call on its hot path (only the missing-node fallback)
- [ ] `grep -n 'tr.addEventListener("click"' frontend/index.html` → no matches (selection is delegated)
- [ ] Per-op line cap (500) and completed-transfer cap (50) exist
- [ ] Search tab click with loaded results issues no `search_registry` invoke (confirm via console/network quiet)
- [ ] Manual stress pass done; `cd src-tauri && cargo test` exits 0
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond plans 004/005/007's expected diffs.
- The `xout` markup/CSS assumption fails (the class or structure differs) — reconcile with the real markup rather than restyling.
- Keeping scroll pinned proves flaky across WKWebView — ship without pinning and note it, rather than adding timers.

## Maintenance notes

- Plan 016 changes WHAT the command line shows (backend-resolved command); it renders through this structure unchanged.
- If plan 005's `h` template landed, use it for the structural `renderXfers` markup; the per-line append path uses `textContent` and needs neither.
- Reviewer: check the fallback path when a `transfer-line` arrives before the row exists in DOM (queueTransfer renders synchronously first, so it should not happen; the fallback is belt-and-braces).
