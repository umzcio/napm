# Plan 039: Reconcile row state after a successful op, and close the run_op rejection hole

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- frontend/index.html`
> If the file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/038-frontend-stale-async-guards.md (same file;
  land 038 first so both drift checks stay clean)
- **Category**: bug
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

Two holes in the transfer lifecycle, both in `frontend/index.html`:

1. **A successful op leaves the row offering the op that just ran.** The
   `transfer-done` handler updates `installed` but never touches `status` or
   `bump`, which are the only fields `renderRows` reads. After an update,
   the row keeps its ↑ glyph and an active Get button targeting the version
   already installed; after a search-initiated install, the row keeps
   `status:"offline"` and shows the ✗ glyph with an Install button for a
   tool that is now installed. Both persist until a manual rescan.
2. **A rejected `run_op` invoke spins forever.** `queueTransfer` fires the
   invoke with no `.catch`; the backend emits `transfer-done` for op-level
   failures, but if the invoke ITSELF rejects (serialization error, handler
   panic) no event ever arrives: the row shows "running" indefinitely and
   the duplicate-op guard for that package stays locked.

## Current state

- `frontend/index.html:1140-1163` - the `transfer-done` handler:

  ```js
  x.done=true; x.ok=e.payload.success;
  if(x.ok){
    var ti=findToolIdx(x.pkg, x.eco);
    if(ti>=0){
      if(x.action==="remove"){
        TOOLS[ti].installed=null;
        if(TOOLS[ti].pinned){ TOOLS[ti].pinned=false; call("set_pin",{pkg:x.pkg,pinned:false}); }
      } else {
        TOOLS[ti].installed=x.to;
      }
    }
  }
  renderXfers(); renderRows();
  ```

- `statusOf(t)` is deliberately a plain reader of the backend-computed field
  (`:437-439`: `function statusOf(t){ return t.status; }`), and `bumpOf`
  likewise (`:490`). The architecture rule (CONTRIBUTING.md) is that version
  comparison and status classification live in Rust; the frontend may only
  map over backend-computed values.
- `queueTransfer`'s invoke (`:952`):
  `iv("run_op",{opId:opId,eco:t.eco,pkg:t.pkg,from:from,to:target,action:action});`
  - fire-and-forget by design (feedback is event-driven); the missing piece
  is only the invoke-rejection path.
- `summarizeErr` exists and is what `call()` uses to render invoke failures.
- The status vocabulary (`version.rs:87-120`, mirrored in the UI):
  `"unmanaged"`, `"offline"`, `"current"`, `"ahead"`, `"update"`.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Inline-style guard | `grep -n 'style=["]' frontend/index.html` | zero hits |
| App check | `npm run tauri dev`                       | manual checklist below |
| Backend sanity | `cd src-tauri && cargo test`           | all pass (nothing Rust changed) |

## Scope

**In scope** (the only file you should modify):
- `frontend/index.html` (the `transfer-done` handler and the `run_op`
  invoke site only)

**Out of scope** (do NOT touch):
- Any Rust file. A single-tool refresh command was considered and rejected
  for this plan: a full `scanLibrary()` per op is too heavy during the
  import queue's sequential installs, and exact-string reconciliation (see
  below) covers every real case.
- `renderRows`, `statusOf`, the appetite dial - readers stay as they are.
- Plan 038's territory (scan guards, feed identity, menus).

## Git workflow

- Branch: `advisor/039-post-op-reconcile`
- One or two commits; style: `fix(ui): ...`.

## Steps

### Step 1: Reconcile `status`/`bump` in the `transfer-done` handler

In the `if(x.ok)` block, after updating `installed`, reconcile the display
state using ONLY exact-string facts the backend already established (this is
presentation reconciliation, not version comparison - add a comment saying
so, and that any suffix-noise nuance resolves on the next scan):

```js
// Post-op display reconciliation. Not version logic: the op installed
// exactly x.to, so x.to===t.latest is the backend's "current" by string
// identity, and a rollback target below t.latest stays "update". Anything
// subtler (prerelease suffix noise) corrects itself on the next scan.
var t=TOOLS[ti];
if(x.action==="remove"){
  t.installed=null; t.status="offline"; t.bump="none";
  if(t.pinned){ t.pinned=false; call("set_pin",{pkg:x.pkg,pinned:false}); }
} else {
  t.installed=x.to;
  if(x.to===t.latest){ t.status="current"; t.bump="none"; }
  else if(t.status==="offline"){ t.status="update"; } // installed an older-than-latest version from search
}
```

(The `else if` covers installing a specific older version: the row was
"offline", is now installed, and a newer `latest` exists, so "update" is the
honest status. A rollback leaves `status:"update"` as-is, which is already
correct.)

**Verify**: `npm run tauri dev` - update a tool; its row immediately shows
the ✓/current treatment with no Get button, before any rescan. Install a
tool from Search; its library row no longer shows the offline glyph.

### Step 2: Add the invoke-rejection path to `run_op`

At `:952`, append a `.catch` that closes the row honestly, mirroring the
`!inv()` early-exit shape two lines above it:

```js
iv("run_op",{opId:opId,eco:t.eco,pkg:t.pkg,from:from,to:target,action:action})
  .catch(function(e){
    x.done=true; x.ok=false;
    x.lines.push({stream:"stderr", line:summarizeErr(e)});
    renderXfers();
  });
```

Keep the existing comment explaining why this is a raw invoke rather than
`call()`, and extend it by a phrase: the `.catch` exists because a rejected
invoke emits no `transfer-done`.

**Verify**: `npm run tauri dev` - run any op and confirm normal behavior is
unchanged (the catch must not fire on success; op-level failures still
arrive via `transfer-done`, not via this path).

### Step 3: Full gate

**Verify**: `grep -n 'style=["]' frontend/index.html` → zero hits;
`cd src-tauri && cargo test` → all pass; manual smoke of update, install
from search, uninstall, and rollback rows.

## Test plan

No new automated tests (frontend-only; verified by running the app per
CONTRIBUTING.md). Manual matrix: update a tool (row goes current), install
from search (row leaves offline), uninstall (row goes offline, pin cleared),
roll back (row goes back to update), and - if you can force an invoke
rejection by temporarily renaming the command - the transfer row ends as
failed with the error line instead of spinning.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `grep -n "catch" frontend/index.html | grep -c "run_op\|summarizeErr"` ≥ 1 new rejection path present (inspect the `run_op` site directly)
- [ ] `grep -n 'status="current"' frontend/index.html` shows the reconciliation
- [ ] `grep -n 'style=["]' frontend/index.html` → zero matches
- [ ] `cd src-tauri && cargo test` exits 0
- [ ] Manual matrix above passes
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The `transfer-done` handler or `queueTransfer` no longer matches the
  excerpts (drift since `3df72f5`, most likely plan 038 landed differently).
- You find a second writer of `t.status` in the frontend, which would mean
  the reconciliation belongs there instead.
- The reconciliation turns out to disagree with the backend on a real
  package (e.g. after the op the backend still reports "update" on rescan
  for the same versions). That means string identity is not a safe proxy -
  report the concrete package and versions.

## Maintenance notes

- The exact-string reconciliation is intentionally conservative; the full
  truth always arrives on the next `scanLibrary()`. If the app ever gains a
  single-tool refresh command, this block should be deleted in favor of it.
- A reviewer should scrutinize that no version comparison crept into the
  frontend beyond string identity - that is the architectural line
  (CONTRIBUTING.md).
- Deferred: doing the same reconciliation for `updated` (the row's
  relative-time column still shows the pre-op mtime until the next scan);
  cosmetic, and the next scan fixes it.
