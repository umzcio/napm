# Plan 038: Frontend guards against stale async results and stale row identity

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

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none (sequence before plan 039, which edits the same file)
- **Category**: bug
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

Three async-identity races in `frontend/index.html`, all the same root
cause: work is keyed by array index or arrival order while the underlying
arrays can be rebuilt mid-flight.

1. **Last-writer-wins rescan.** `scanLibrary()` has no sequence guard, so
   clicking "Rescan now" twice (or saving Preferences mid-scan) lets the
   OLDER scan overwrite the newer one - the library then shows stale machine
   state until the next manual rescan. The codebase already has the pattern
   for this: `searchSeq` guards search, `feedGen` guards What's New.
2. **FEED cards cache `ti` indexes into `TOOLS`.** A rescan replaces `TOOLS`
   but the old feed stays live until the async reload resolves. In that
   window `TOOLS[it.ti]` is either `undefined` (a TypeError mid-render that
   leaves the feed half-painted) or, worse, a DIFFERENT tool - the card's
   Get/Uninstall buttons then act on the wrong package.
3. **Context menus capture indexes.** The library menu's Pin item captures
   `i` and calls `togglePin(i)`, which reads `TOOLS[i]`; a rescan between
   right-click and menu click pins/unpins whatever tool now sits at that
   index. The history menu half-fixed this already (its `run` closures
   re-resolve via `findToolIdx`) but its `disabled`/label closures use the
   build-time `ti`.

## Current state

All in `frontend/index.html` (single-file vanilla JS frontend, no build
step, house style is `var`/`function`, ES5-ish):

- The existing guard pattern to mirror (`:415`, `:573-582`):

  ```js
  var searchSeq = 0;
  ...
  var seq=++searchSeq;
  ... .then(function(r){ if(seq!==searchSeq) return; ... })
  ```

- Unguarded scan (`:1068-1082`):

  ```js
  function scanLibrary(){
    if(!inv()){ renderRows(); FEED_LOADED=false; loadWhatsNew(); renderStatus(); dismissSplash(); return; }
    call("scan_installed").then(function(tools){
      TOOLS = tools || [];
      ...
    }).catch(function(e){ ... });
  }
  ```

  `annotateNpxDrift` (`:1085-1098`) likewise writes `NPX_DRIFT` from a lazy
  `npx_latest` invoke with no guard.
- Feed items capture `ti` at build time (`:684`, `:694`:
  `feed.push({ti:ti, pkg:a.pkg, eco:a.eco, ...})`), and use it later:
  renderFeed reads `TOOLS[it.ti].name/.installed/.latest` (`:735-739`), the
  changelog expand reads `TOOLS[it.ti].latest` (`:831`), and the click
  handlers act on `TOOLS[it2.ti]` / `TOOLS[it3.ti]` (`:836-838`).
- `findToolIdx(pkg, eco)` already exists and is the identity re-resolver
  (used at `:595`, `:649`, `:906`).
- Library menu Pin (`:1015`): `{label: t.pinned?"Unpin":"Pin", run:function(){ togglePin(i); }}`;
  `togglePin` (`:801`) reads `TOOLS[i]`. Note the menu closure also captures
  `t` (the tool object), which stays the RIGHT tool even after a rebuild -
  so prefer capturing `t` and re-resolving its index at click time.
- `histMenu` (`:904-915`): `run` closures already re-resolve
  (`var c=findToolIdx(h.pkg, h.eco); ...`); the `disabled:function(){ return !canRoll || ti<0; }`
  and label still use the build-time `ti`.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Inline-style guard | `grep -n 'style=["]' frontend/index.html` | zero hits (the stylesheet comment is excluded by the character class) |
| App check | `npm run tauri dev`                       | see manual checklist below |
| Backend sanity | `cd src-tauri && cargo test`           | all pass (nothing Rust changed) |

## Scope

**In scope** (the only file you should modify):
- `frontend/index.html`

**Out of scope** (do NOT touch, even though they look related):
- Any Rust file - no backend change is needed; all identity data (`pkg`,
  `eco`) is already on the wire.
- The `transfer-done` handler and `queueTransfer` (plan 039's scope).
- `verdictScope()` and the `data-install` attributes (plan 040's scope).
- Do not add a frontend test harness; this repo verifies frontend changes by
  running the app (CONTRIBUTING.md).

## Git workflow

- Branch: `advisor/038-frontend-stale-guards`
- Commit per numbered fix; style: `fix(ui): ...` (see `git log`).

## Steps

### Step 1: Sequence-guard `scanLibrary` and `annotateNpxDrift`

Add `var scanSeq=0;` next to `NPX_DRIFT` (`:1067`). In `scanLibrary`, take
`var seq=++scanSeq;` at entry (after the `!inv()` early return - that path
is synchronous and needs no guard) and check `if(seq!==scanSeq) return;` as
the first statement of both the `.then` and the `.catch`. Pass `seq` into
`annotateNpxDrift(seq)` and, inside its `.then`, drop the result when stale
(`if(seq!==scanSeq) return;` before touching `NPX_DRIFT`). Keep the
`.catch(function(){})` as-is.

**Verify**: `grep -n "scanSeq" frontend/index.html` shows declaration, bump,
and at least three checks. `npm run tauri dev`: the library loads on launch
(splash dismisses), and File → Rescan now works.

### Step 2: FEED items stop caching `ti`

Every feed item already carries `pkg` and `eco`. Remove the `ti` field from
both `feed.push` sites (`:684`, `:694`) and re-resolve at use:

- renderFeed (`:735-739`): `var ti=findToolIdx(it.pkg, it.eco);` then guard
  `ti<0` (tool gone after a rescan: render the card without the
  installed/latest line rather than throwing).
- Changelog expand (`:831`): `var ti=findToolIdx(it.pkg,it.eco); var ver=it.fix||(ti>=0?TOOLS[ti].latest:it.pkg);`
- Get / Uninstall click handlers (`:836-838`): re-resolve
  `var ti=findToolIdx(it2.pkg, it2.eco); if(ti>=0){ ... }` (same for `it3`).

**Verify**: `grep -n "it.ti\|it2.ti\|it3.ti" frontend/index.html` → zero
hits. In `npm run tauri dev`: open What's New, expand a card (changelog
loads), click Get on a card (transfer queued for the correct package).

### Step 3: Menus re-resolve identity at click time

- Library menu Pin item (`:1015`): change the run closure to
  `run:function(){ var c=findToolIdx(t.pkg,t.eco); if(c>=0) togglePin(c); }`.
  (The closure already captures the right `t`; only the index lookup was
  stale-prone.)
- `histMenu` (`:904-915`): move the `ti` lookup inside the closures that use
  it - `disabled:function(){ return !canRoll || findToolIdx(h.pkg,h.eco)<0; }`
  and likewise for the "Jump to tool" item. Labels may keep the build-time
  text (cosmetic only).

**Verify**: `grep -n "togglePin(i)" frontend/index.html` → zero hits. Manual:
right-click a library row → Pin toggles the row you clicked; do the same
while a rescan is in flight if you can catch it.

### Step 4: Full gate

**Verify**: `grep -n 'style=["]' frontend/index.html` → zero hits (you added
no inline styles). `cd src-tauri && cargo test` → all pass. Manual smoke of
the whole app: scan, search, What's New expand + Get, Transfers, right-click
menus on library and history, Preferences save (it triggers a rescan - this
is the easiest way to exercise the step-1 guard: save Preferences twice
quickly and confirm the library ends consistent).

## Test plan

No new automated tests (frontend-only change; the repo convention is manual
verification per CONTRIBUTING.md). The verification is the grep guards plus
the manual checklist in step 4. If `scripts/check-csp.js` exists by the time
this runs (plan 036), `npm run check:csp` must also exit 0.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `grep -n "scanSeq" frontend/index.html` shows guard declaration, bump, and checks in `.then`/`.catch`/`annotateNpxDrift`
- [ ] `grep -n "it\.ti\|it2\.ti\|it3\.ti\|togglePin(i)" frontend/index.html` → zero matches
- [ ] `grep -n 'style=["]' frontend/index.html` → zero matches
- [ ] `cd src-tauri && cargo test` exits 0
- [ ] Manual checklist in step 4 passes in `npm run tauri dev`
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts above do not match the live file (drift since `3df72f5`) -
  line numbers will shift; the quoted code must match.
- `findToolIdx` does not exist or has a different signature than
  `(pkg, eco)`.
- Fixing the feed reveals that `FEED` is keyed/consumed somewhere else by
  `ti` that this plan did not list (grep first; if a new consumer appears,
  report it).

## Maintenance notes

- The rule going forward: nothing in this file may carry an array index
  across an await boundary or into a menu/click closure; capture `pkg`+`eco`
  (or the history entry's fields) and re-resolve with `findToolIdx`. A
  reviewer should reject new `data-*` attributes or closures that smuggle
  indexes.
- `scanSeq` makes "last scan wins" explicit; if a future change adds scan
  cancellation, the seq counter is where it hooks in.
- Deferred out of scope: the same re-resolution idea for the transfers list
  (`xfers` rows are already keyed by `opId`, which is stable, so no work
  needed there - verified during planning).
