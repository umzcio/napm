# Plan 031: The Shared Library table must fit the window

> **Executor instructions**: Follow the plan, run every verification, touch only the in-scope files.
> STOP and report if a STOP condition occurs. Update your row in `plans/README.md` when done.

## Status
- **Priority**: P1 (every user hits this on first launch) | **Effort**: M | **Risk**: MEDIUM (pure layout, but it is the app's main view)
- **Planned at**: `main` @ 2a9508a, 2026-08-08
- **Reported by**: the maintainer, from a screenshot of v0.1.5 running with a real 504-package library

## Why this matters

In the Shared Library, once real rows render, the table grows wider than the window and the last
five columns fall off the right edge. The user sees only the glyph, Tool, Installed, and a clipped
Latest. **Shared By, Size, Updated, Pin, and the action button are all off-screen.**

The action column is the one that matters: it holds the `Get` / `Install` button, the per-row
control of the primary view. Update All and the row context menu still work, so the app is not
unusable, but the discoverable way to update one package is off-screen. The panel scrolls
horizontally and macOS overlay scrollbars stay invisible until a scroll gesture starts, so nothing
on screen tells the user those five columns exist.

The empty table looks fine (all nine headers fit), so this only appears once a scan completes. That
is why it survived to a release.

## Root cause (verified, do not re-derive)

`src-tauri/tauri.conf.json` ships the window at **`width: 880`**, `minWidth: 720`.

`frontend/index.html`:
- **:92** `table{border-collapse:collapse; width:100%; font-size:12px;}` — auto layout, so column
  widths are driven by content and the table expands past 100% when content demands it.
- **:96** `tbody td{padding:3px 8px; white-space:nowrap; vertical-align:top;}` — nothing wraps, so
  every cell contributes its full natural width to the table's minimum.
- **:122-123** `.toold{display:block; ... white-space:nowrap; overflow:hidden; text-overflow:ellipsis;
  max-width:360px;}` — the per-row description. It ellipsizes, but at a **hardcoded 360px**, so the
  Tool column claims roughly 375px (360 + 16px of padding) regardless of how wide the window is.

375px of an 880px window is 43% spent on one column before the other eight get anything. The
descriptions are on by default (`VIEW.desc` initialises to `true` at **:363**), so this is the
out-of-the-box state, not an opt-in one.

The fix is to make the Tool column *yield* to the window instead of dictating to it.

## Current state

Row markup is built in `renderRows()` at **frontend/index.html:415-444**. The nine cells, in order:

| # | Column | Content |
|---|--------|---------|
| 0 | (glyph) | one status character |
| 1 | Tool | optional 📌, name, source pill, optional `.toold` description, optional npx drift note |
| 2 | Installed | version or `—` |
| 3 | Latest | version or `—` |
| 4 | Shared By | `@publisher` |
| 5 | Size | e.g. `1.2 MB` |
| 6 | Updated | relative age via `ago()` |
| 7 | Pin | 📌 toggle |
| 8 | (action) | `Get` / `Install` button or `—` |

The header lives at **frontend/index.html:242-245**.

## Approach

Switch the library table to **fixed layout with an explicit `<colgroup>`**, so column widths come
from the stylesheet rather than from content, and the Tool column absorbs whatever is left over.

1. Add a `<colgroup>` to the library table only (the Search results table at :502 is out of scope).
2. Give the library table `table-layout:fixed`. Scope this with a class or an id so the Search
   table's layout is unchanged.
3. Size columns 0 and 2-8 in px, sized to their real content (measure, do not guess: `Installed`
   and `Latest` must fit a long-but-real version like `3.0.0-beta.33`; `Updated` must fit the
   widest `ago()` output; the action column must fit the `Install` button, which is wider than
   `Get`). Leave the Tool column at `width:auto` so it takes the remainder.
4. Replace `.toold`'s `max-width:360px` with a width that follows the column (`max-width:100%`, or
   drop the constraint and let the fixed column clip it). The description must still ellipsize.
5. The **name line** must ellipsize too. Today it is nowrap with no clipping, so a long scoped
   package name (`@pnp/cli-microsoft365-mcp-server`) would otherwise overflow a fixed column and
   collide with the Installed column. Give the name line the same overflow/ellipsis treatment, and
   keep the source pill visible (it must not be the thing that gets truncated away).
6. Raise `minWidth` in `src-tauri/tauri.conf.json` to the narrowest width at which all nine columns
   are still legible, and raise the default `width` if 880 is not enough for a usable Tool column
   after the other eight are allocated. State the numbers you chose and why.

Keep the Windows-98 look exactly: same borders, same padding, same fonts, same sticky header.

## Verification harness (you cannot drive the Tauri window)

Build a throwaway measurement page in the scratchpad. Do **not** add it to the repo.

1. Copy the `<style>` block and the library table markup from `frontend/index.html` into a standalone
   HTML file under the scratchpad directory.
2. Hand-write ~20 `<tr>`s matching the real shapes above, including these adversarial cases:
   - a long scoped name with a long description: `@pnp/cli-microsoft365-mcp-server`
   - a prerelease version: `3.0.0-beta.33`
   - a manual row whose description is an absolute path: `/Users/x/.grok/downloads/grok-1.0.0-macos-aarch64`
   - a row with the `Install` button (status `offline`)
   - a row with a pin and an npx drift note
3. Open it with a browser tool at viewport widths **720, 880, and 1200**.
4. At each width assert, via `getBoundingClientRect()`:
   - `document.querySelector('table').scrollWidth <= panel.clientWidth` (no horizontal overflow)
   - every `th` has `right <= panel.clientWidth` (all nine headers on screen)
   - the last column's button is fully visible
   - the Tool cell's text is clipped with an ellipsis, not overflowing its column
5. Paste the measured column widths at each viewport into your report.

Then repeat with descriptions off (remove the `.toold` spans) and confirm the layout does not break.

## Commands
| Purpose | Command | Expected |
|---|---|---|
| Rust untouched | `cd src-tauri && cargo test` | 201 passing |
| Config parses | `python3 -c "import json;json.load(open('src-tauri/tauri.conf.json'))"` | no error |
| No stray files | `git status --short` | only the in-scope files |

## Scope
**In scope**: `frontend/index.html` (library table markup + the CSS rules named above),
`src-tauri/tauri.conf.json` (window `width` / `minWidth` only).
**Out of scope**: the Search results table (`:502`) and `.pkgcell`; the Transfers and What's New
panes; `renderRows()`'s data logic (which cells hold what); adding a column-resize feature; adding a
horizontal scrollbar affordance as a substitute for fitting; any Rust source.

## Git workflow
- Branch: `advisor/031-library-table-fit` from `main`.
- Commit: `fix(ui): library table fits the window so the action column is reachable`.
- Do NOT push, open a PR, or merge unless the operator asks.

## Done criteria
- [ ] At the shipped default window width, all nine columns including the action button are visible
      with no horizontal scrolling (harness measurements pasted in your report)
- [ ] Same at `minWidth`
- [ ] Long names and long descriptions ellipsize inside the Tool column; neither overflows
- [ ] The source pill is never truncated away
- [ ] Toggling descriptions off does not break the layout
- [ ] Visual chrome is unchanged (borders, padding, fonts, sticky header)
- [ ] `cargo test` still 201 passing; `tauri.conf.json` parses
- [ ] Only the in-scope files changed (`git diff --stat main..HEAD`)

## STOP conditions
- All nine columns cannot be made legible at `minWidth: 720` without shrinking a column below
  usefulness. Report your measurements and recommend a new `minWidth` rather than shipping a layout
  that is broken at the low end.
- Fixing the layout would require changing which columns exist, or hiding columns responsively.
  That is a product decision, not a layout fix: report and stop.

## Maintenance notes
- Every future column added to this table must be added to the `<colgroup>` too, or fixed layout
  will silently give it zero width. Leave a comment on the colgroup saying so.
- Reviewer: check this at both window extremes and with descriptions both on and off, and confirm
  the Search results table was not affected.
