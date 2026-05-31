# napm M6 - Menu bar (File/Edit/View/Swarm/Help)

**Date:** 2026-05-31
**Status:** Approved, ready for implementation plan
**Milestone:** M6 (see `docs/ROADMAP.md`)

## Goal

Make the inert Win98 menu bar do real things: working period-accurate dropdown
menus, with the View menu's filters and sorts (especially "only tools I
installed") taming the library, plus the simple File/Swarm/Help/Edit actions.
Preserve the late-90s look; every item carries real function.

## Scope

This is the menu mechanics plus the high-value actions that need no settings
layer. Explicitly DEFERRED to a later "Preferences / Settings" milestone: the
Preferences dialog and anything needing a persisted settings store (GitHub token
field, default-appetite setting, enable/disable sources), plus Export library
(JSON / Markdown). The token already works via env var and the appetite already
persists in localStorage, so nothing breaks by waiting.

## Menu mechanics

The five titles (File/Edit/View/Swarm/Help) become real dropdown menus, driven by
a small data definition so they stay maintainable and the component is reusable
(M9 right-click can reuse it later).

- Clicking a title opens a beveled dropdown panel directly beneath it, raised
  border matching the existing window chrome.
- One menu open at a time. Clicking a different title switches; clicking an item
  runs its action and closes; clicking outside or pressing Escape closes. Once a
  menu is open, hovering another title switches to it (include only if cheap).
- A per-menu array of item descriptors, each one of: action (label + handler),
  toggle (shows a check when on), radio (shows a dot for the selected item in a
  group), separator, or disabled (greyed, no handler). A `renderMenu` function
  turns the data into markup, so adding or reordering items is a one-line data
  change.
- Underlined mnemonics stay decorative (Alt+letter accelerators deferred with
  Keyboard Shortcuts).

## Menu contents

### View (frontend over TOOLS, persisted in localStorage `napm.view`)

Independent, combinable filters (not a single radio group) - the combinations are
the value (only-outdated + only-installed = "updates to the CLIs I chose"):

- Filter -> Only tools I installed (toggle): hides brew dependencies via
  `installed_on_request`. The standout that tames ~287 rows to ~40.
- Filter -> Only outdated (toggle): hides current/up-to-date rows.
- Source (toggle per npm/brew/pip/npx, default all on): show/hide each ecosystem.
- Sort by (radio: Name / Size / Updated / Status), Name default.
- Show descriptions (toggle, default on): collapse the sub-line for a denser list.

Checkmarks and dots reflect current state each time the menu opens.

### File

- Rescan now: re-runs the scan and refreshes the feed.
- Open data folder: reveals the app-data dir in Finder.
- Quit: closes the window.

### Swarm

- Refresh registry caches: deletes the cached brew catalog/analytics + wire files
  so the next Search / What's New refetches, then re-warms brew in the background.
- (Source enable/disable deferred to the Settings milestone.)

### Help

- About napm: a Win98 modal with the npstr logo, the live version, the name, repo
  link, MIT, and one line of homage flavor.
- Repo link: opens github.com/umzcio/napm.

### Edit

- Copy tool details: copies the selected library row's details (name, source,
  versions, publisher, size) as plain text to the clipboard; greyed when no row
  is selected.

## Backend touches (no new plugins; same shell-out pattern as scan/ops)

- **`requested` field on `InstalledTool`** (bool). npm/pip/npx always `true`
  (top-level globals the user chose). brew reads `installed_on_request` from each
  formula's `INSTALL_RECEIPT.json` (already parsed for install time). Unknown ->
  defaults to `true` (better to show a tool than wrongly hide it). Drives "only
  tools I installed."
- **`open_data_dir()`** and **`open_external(url)`** commands: each runs macOS
  `open <path|url>` via `std::process` (no opener plugin, no new capability).
  `open_external` validates the arg starts with `https://`.
- **`clear_caches()`** command: deletes `brew_catalog.json`, `brew_analytics.json`,
  `wire.json`, and `changelog_*` files from the app-data dir, then re-warms the
  brew catalog in a background thread. Powers Swarm -> Refresh registry caches.
- Rescan, Quit, Copy details, and the About dialog are frontend-only (re-invoke
  `scan_installed`; `getCurrentWindow().close()`; `navigator.clipboard` with a
  hidden-textarea fallback; a modal using the live `getVersion`).

## Persistence

The View filter/sort/descriptions state lives in `localStorage` (`napm.view`),
loaded at boot and applied, same pattern as the appetite dial, so the preferred
view sticks across launches.

## renderRows change

`renderRows` derives a display list (filter by requested/outdated/source, then
sort) but each row keeps its original `TOOLS` index in `data-i`, so selection,
pins, and the Get button keep working unchanged.

## Error handling

Every backend action is best-effort: a failed `open`, a missing cache file, or a
clipboard failure degrades silently rather than erroring. The "only tools I
installed" filter never hides a tool whose request-status is unknown (defaults to
shown).

## Testing

A pure parser for `installed_on_request` from a receipt JSON, and the `requested`
defaults (npm/pip/npx -> true) get unit tests alongside the existing scan tests.
The menu mechanics, View filters, and dialogs are verified live (no automated UI
tests), consistent with the rest of the frontend.

## Out of scope (deferred)

- Preferences / Settings dialog and the persisted settings store (GitHub token,
  default appetite, enable/disable sources).
- Export library (JSON / Markdown).
- Keyboard Shortcuts dialog and Alt+letter mnemonic accelerators.
- Swarm -> jump to Search (redundant; the Search tab exists).
