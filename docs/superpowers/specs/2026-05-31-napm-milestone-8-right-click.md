# napm M8 - Right-click context menus

**Date:** 2026-05-31
**Status:** Approved, ready for implementation plan
**Milestone:** M8 (see `docs/ROADMAP.md`)

## Goal

Right-click any meaningful row and get a Win98 beveled context menu of the
actions that apply to it. Reuse the M6 menu engine; route every item through
existing functions. Keep the late-90s look; every item does something real.

## Architecture: generalize the M6 engine into a cursor popup

Today `renderMenu(name, anchor)` reads `MENUS[name]` and positions the shared
`#menuPop` under a titlebar anchor. Refactor the core into `openPopup(items, x, y)`:

- Stores the rendered item array (`currentItems`), draws them into `#menuPop`
  with the existing item model (action / separator / disabled, plus checks/dots
  for the menubar), positions at `(x, y)` with the right/bottom edge clamp added
  in M6, and shows it.
- The menubar's `renderMenu` becomes a thin caller: `openPopup(MENUS[name],
  titleLeft, titleBottom)` plus the title-highlight.
- The `#menuPop` click handler reads `currentItems[idx]` instead of
  `MENUS[openMenuName][idx]`, so it serves both the menubar and context menus.
- Dismiss is unchanged: the M6 outside-`mousedown`-capture and Escape close any
  open popup, menubar or context.

Four `contextmenu` handlers (library `#rows`, search `#searchResults`, the
transfers list, the history list): each `preventDefault()` then
`openPopup(items, e.clientX, e.clientY)` with items built from the row under the
cursor. No new backend commands.

## Per-surface menu items

Supporting helpers (frontend): `registryUrl(eco, pkg)` (npm/npx ->
npmjs.com/package, brew -> formulae.brew.sh/formula, pip -> pypi.org/project,
else none), `installCmd(eco, pkg, version)` (`npm i -g pkg@ver` / `brew install
pkg` / `pip install pkg==ver`), and `priorVersion(pkg, eco)` (most recent history
entry for this tool that has a `from`, used by library rollback).

**Library row** (the tool under the cursor, by its `data-i`):

- Update to `<latest>` / Install (outdated/offline) - existing `queueTransfer`
  Get path; omitted or greyed when already current.
- Roll back to `<prev>` - via `priorVersion`; existing rollback path. Greyed when
  no prior version exists and for brew (gated exactly as the History pane).
- Pin / Unpin - existing `set_pin` toggle.
- Copy package name.
- Copy install command (`installCmd`).
- Open `<registry>` page - via `open_external`.
- What's New for this - switches to the What's New tab.

**Search result:**

- Get / Install (existing `installPackage`).
- Copy package name.
- Copy install command.
- Open `<registry>` page.
- Filter swarm to `<source>` - sets the source chip.

**Transfers row:**

- Copy log output (the streamed lines).
- Copy command (the exact `$ ...`).
- Re-run (re-queues the same op). Needs the transfer row to remember its
  `action` and `to`: stamp both onto the row object where `queueTransfer` builds
  it.

**History entry:**

- Roll back to this version - the existing entry rollback logic (brew-gated, has
  a `from`).
- Copy entry (pkg, action, from -> to).
- Jump to tool in library - switches to Library and selects the matching row.

The "manual / unmanaged" ecosystem (future M9) has no registry page, so that item
is simply absent for those rows.

## Supporting data changes

- Store the loaded history array on the frontend (so library rollback's
  `priorVersion` can find a prior version). `loadHistory` already fetches it;
  keep it in a module var.
- Stamp `action` and `to` onto each transfer row object in `queueTransfer` (for
  Re-run).

## Errors and edge cases

- Right-clicking empty space (below the last row, a header) opens no menu - the
  handler only fires when the cursor is on an actual row/result/transfer/entry.
- The menubar and context menus share `#menuPop`; opening either closes the
  other. The existing outside-mousedown / Escape dismiss covers both.
- Greyed items never act: rollback with no prior version or brew, registry-page
  for an ecosystem without one - shown disabled, no handler.
- A context menu acts on the row under the cursor (its `data-i`), independent of
  the blue-highlighted `selected` row, so right-clicking one row while another is
  selected does the right thing.
- Every action is best-effort and reuses M3-M7 paths; a failed `open_external`,
  clipboard, or transfer degrades exactly as it does from the buttons today.

## Testing

No backend and no pure functions worth unit-testing - this is contextmenu wiring
over existing commands. Verified live: right-click each surface, confirm the
items appear and act (install/rollback route to Transfers, pin toggles, copy puts
the right text on the clipboard, registry pages open, filters/jumps navigate),
and confirm dismiss (outside-click, Escape, picking an item) works for context
menus as for the menu bar. The milestone-end review adversarially checks the
surface handlers and the engine refactor.

## Out of scope (deferred)

- A true homepage/repo link and the backend metadata field it needs (registry
  page is used instead).
- Right-click on the menu bar items themselves (not meaningful).
- Keyboard-driven context menu (Shift+F10 / Menu key) - mouse only for v1.
