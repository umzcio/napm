# Plan 004: Key transfers by package identity, guard double-fires, and fix ecosystem-blind lookups (frontend)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- frontend/index.html`
> If the file changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it as
> a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED (touches the central transfer path in a file with no automated tests; verified by running the app)
- **Depends on**: none (plan 003 is the backend safety net; either order works)
- **Category**: bug
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Three related identity bugs in the transfer path of `frontend/index.html`:

1. **Transfers are keyed by array index into `TOOLS`.** A queued transfer stores `ti` (an index). `scanLibrary()` replaces `TOOLS` wholesale at any time (File → Rescan now, or saving Preferences). When the op completes, the handler writes `TOOLS[x.ti].installed = x.to` with no check that the tool at that index is still the same package — so a rescan during an install stamps the new version onto whatever row now sits at that index. The library then displays a version the user does not have, in an app whose whole brand is "no fake data".
2. **No in-flight guard.** `queueTransfer` never checks for an unfinished op on the same package; double-clicking Get, or clicking Update All twice, runs duplicate concurrent package-manager processes and writes duplicate history entries.
3. **`findTool(pkg)` ignores the ecosystem.** History rollback, the history context menu, and Transfers "Re-run" resolve tools by package name only. For a name that exists in two ecosystems (common between brew and pip: `black`, `httpie`), a pip rollback can build and run a brew command. A correct `findToolIdx(pkg, eco)` already exists and is used by the What's New feed.

## Current state

All excerpts from `frontend/index.html` (one inline `<script>`; ~830 lines of JS; ES5 style, `var`, function declarations — match it):

- `:337-340` — the two lookup helpers:
  ```js
  function findTool(pkg){ for(var i=0;i<TOOLS.length;i++) if(TOOLS[i].pkg===pkg) return i; return -1; }
  ```
  and at `:562` the correct one: `function findToolIdx(pkg, eco){ ... TOOLS[i].pkg===pkg && TOOLS[i].eco===eco ... }`
- `:737-751` — `queueTransfer` (index-based, builds a display command, fires invoke):
  ```js
  var opSeq=0;
  function queueTransfer(ti,target,action){
    var t=TOOLS[ti]; if(!t) return; var from=t.installed;
    var cmd=(t.eco==="npm"?"npm i -g "+t.pkg+"@"+target : ...);
    var opId="op"+(++opSeq);
    var x={opId:opId,ti:ti,pkg:t.pkg,name:t.name,user:t.publisher||"unknown",cmd:cmd,lines:[],done:false,ok:false,
           action:action,from:from,to:target};
    xfers.unshift(x); renderXfers();
    var inv=window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke;
    if(!inv){ ... }
    inv("run_op",{opId:opId,eco:t.eco,pkg:t.pkg,from:from,to:target,action:action});
  }
  ```
- `:907-915` — the completion handler that writes through the stale index:
  ```js
  ev.listen("transfer-done",function(e){
    var x=xfers.find(function(z){return z.opId===e.payload.op_id;});
    if(!x) return;
    x.done=true; x.ok=e.payload.success;
    if(x.ok && x.ti!=null && TOOLS[x.ti]){ TOOLS[x.ti].installed=x.to; }
    renderXfers(); renderRows();
    FEED_LOADED=false;
    loadHistory();
  });
  ```
- `:860-869` — `scanLibrary` replaces `TOOLS` (`TOOLS = tools || [];`) and is reachable mid-op from Rescan (`:952`) and Preferences save (`:1090-1092`).
- Call sites of `queueTransfer` (all pass a `TOOLS` index today): `:505` and `:509` (`installPackage` — `:509` pushes a synthetic row then uses `TOOLS.length-1`), `:671` (feed Get), `:716` (history Roll back), `:725` (history menu roll back), `:762` (Transfers Re-run), `:775` (Update All), `:781` (library row Get/Install), and three inside `libMenu` (the library row context menu): `:800` (Update to latest), `:801` (Install), `:805` (Roll back to prev). `libMenu` already binds `var t=TOOLS[i]` at `:784`, so these three closures pass `t` instead of `i` — no other change to `libMenu`. (Added 2026-08-08 after an executor STOP correctly caught the omission.)
- `findTool` call sites to fix: `:715` (`histWrap` click handler — note `eco` is read into a var and then unused), `:721` (`histMenu`), `:761-762` (`xferMenu` Re-run), `:503` (`installPackage` — search results have an `eco`, use it).
- Backend contract (`src-tauri/src/ops.rs:40-52`): `run_op` takes `opId, eco, pkg, from, to, action` — unchanged by this plan.
- If plan 003 landed, the backend also rejects a duplicate `(eco, pkg)` op with a stderr `transfer-line` and a failed `transfer-done`; this plan's guard prevents the double-fire in the first place.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Backend tests (regression only) | `cd src-tauri && cargo test` | exit 0 (this plan should not affect them) |
| Run the app | `npm run tauri dev` | app launches; manual checklist below |
| Sanity grep | see Done criteria | |

There is no JS test harness; frontend changes are verified by running the app (CONTRIBUTING: "Frontend changes are verified by running the app").

## Scope

**In scope**:
- `frontend/index.html` — ONLY the functions/handlers named in the steps.

**Out of scope**:
- Any Rust file.
- `renderXfers` internals beyond what the identity change requires (render performance is plan 011).
- Error-state rendering and the invoke helper consolidation (plan 007).
- The `cmd` display string (plan 016 replaces it with the backend's real command; keep the current construction working here).

## Git workflow

- Branch: `advisor/004-transfer-identity`
- Commit style: `fix(ui): key transfers by pkg+eco, guard double-fires, eco-aware lookups`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Change `queueTransfer` to take a tool object

Change the signature to `queueTransfer(t, target, action)` where `t` is the tool object (has `pkg`, `eco`, `name`, `publisher`, `installed`). Keep storing `pkg` and add `eco` on the transfer record `x`; DELETE the `ti` field. Add the in-flight guard as the first statement:

```js
function queueTransfer(t, target, action){
  if(!t) return;
  var dup=xfers.find(function(z){ return !z.done && z.pkg===t.pkg && z.eco===t.eco; });
  if(dup){ switchTab("transfers"); return; }
  ...
}
```

Update every call site to pass the tool object instead of an index:
- `:781` row Get: `queueTransfer(TOOLS[j], ...)` (it already has `j`)
- `:775` Update All: pass `t` directly (the `forEach` already has it)
- `:671` feed Get: `it2.ti` is a `TOOLS` index from `findToolIdx`; pass `TOOLS[it2.ti]`
- `:716`/`:725` history rollback: resolve with `findToolIdx(pkg, eco)` (Step 3) and pass the tool
- `:762` Re-run: same
- `:505`/`:509` `installPackage`: for the existing-tool branch pass `TOOLS[ti]`; for the new-package branch, build the row object, push it, and pass the same object reference (no `TOOLS.length-1`)

### Step 2: Resolve identity at completion time

In the `transfer-done` handler, replace the index write with a lookup:

```js
if(x.ok){
  var ti=findToolIdx(x.pkg, x.eco);
  if(ti>=0) TOOLS[ti].installed=x.to;
}
```

If the tool no longer resolves (rescan removed it), drop the write silently — the follow-up `loadHistory()` and the next scan carry the truth.

### Step 3: Route every lookup through `findToolIdx` and delete `findTool`

- `:715`: `var ti=findToolIdx(pkg, eco);` (the handler already reads `eco` from `data-roll-eco` — it is currently unused; use it)
- `:721-729` `histMenu`: `findToolIdx(h.pkg, h.eco)` (history entries carry `eco`)
- `:761-762` `xferMenu`: the transfer record now carries `eco` (Step 1); use `findToolIdx(t.pkg, t.eco)`
- `:503` `installPackage`: `findToolIdx(pkg, p.eco)` (the search result `p` has `eco`; move the `findTool` call below the `p` lookup)
- Delete the `findTool` function.

### Step 4: Manual verification run

`npm run tauri dev`, then walk this checklist:

1. Install/update a tool from the library; while it streams, File → Rescan now. When the op completes, the CORRECT row shows the new version (or, if the tool vanished from the scan, no row was mislabeled).
2. Double-click Get rapidly on an outdated tool → exactly one transfer row appears.
3. Click Update All twice quickly → each package appears at most once in Transfers.
4. History tab → Roll back on an entry → the transfer's command matches that entry's ecosystem.
5. Transfers → right-click a finished transfer → Re-run works.
6. Search → install a package not in the library → it installs and appears in the library after the follow-up scan.

**Verify**: all six behaviors as described; no errors in the webview console (right-click → Inspect in dev builds).

## Test plan

Manual checklist in Step 4 (no JS harness exists). Backend regression: `cd src-tauri && cargo test` → exit 0, unchanged count.

## Done criteria

- [ ] `grep -c "function findTool(" frontend/index.html` → 0 (only `findToolIdx` remains)
- [ ] `grep -n "ti:ti" frontend/index.html` → no matches (transfer records carry pkg+eco, not an index)
- [ ] `grep -n "TOOLS\[x.ti\]" frontend/index.html` → no matches
- [ ] `grep -cn "queueTransfer(" frontend/index.html` → every call site passes a tool object (visually confirm each)
- [ ] Manual checklist (Step 4) passes, all six items
- [ ] `cd src-tauri && cargo test` exits 0
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live file (drift — especially if plan 007/011/016 already landed and moved these functions).
- You find an additional `queueTransfer` call site not listed above (`grep -n "queueTransfer(" frontend/index.html` first; the list must be complete before you start).
- Step 4 item 1 still mislabels a row — that means an index survived somewhere; report the location rather than patching around it.

## Maintenance notes

- Plan 016 will replace the `cmd` display string with the backend's resolved command line; it assumes transfer records carry `pkg`+`eco` from this plan.
- Plan 011 (render performance) rewrites `renderXfers` DOM handling; land this first so it builds on identity-keyed records.
- Reviewer: check every `queueTransfer` call site diff — a missed index-passing site fails silently (`t` would be a number, `if(!t)` passes for nonzero, then `t.pkg` is undefined).
