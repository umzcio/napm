# napm M7 - Preferences / Settings

**Date:** 2026-05-31
**Status:** Approved, ready for implementation plan
**Milestone:** M7 (see `docs/ROADMAP.md`)

## Goal

A persisted settings store plus a Win98 Preferences dialog for the two settings
with no other home (GitHub token, source enable/disable), and the Export library
action the M6 File menu pointed at. Keep the late-90s look; everything real.

## Scope

Preferences holds the GitHub token and source enable/disable. The appetite stays
owned by the dial (a prominent live control that already persists), so it is NOT
duplicated in Preferences. Export library is a File-menu action.

## Settings store

Extend the existing `Store` (the app-data JSON layer that holds pins and history)
with a `settings.json`:

```
Settings {
  github_token: String,                       // "" = none
  sources: { npm: bool, brew: bool, pip: bool, npx: bool },
}
```

Defaults: empty token, all sources enabled. Read at launch, written on Save.
Corrupt or missing file reads as defaults (same tolerance as pins/history). Two
thin commands: `get_settings()` and `set_settings(settings)`.

## Preferences dialog

Reuse the M6 modal chrome (`#modalBack` / `#modalBox`, the same beveled modal the
About box uses). Opened from a new **Edit -> Preferences...** item (Edit currently
only has Copy tool details).

- **GitHub token** text field, hint: "optional - raises the GitHub API rate limit
  for changelogs and the supply-chain wire."
- Four **source** checkboxes (npm / brew / pip / npx); unchecking one makes the
  scan skip that ecosystem.
- **Save** and **Cancel** buttons.

On open it loads current settings via `get_settings`. On Save it calls
`set_settings`, closes, and triggers a rescan (sources may have changed) plus a
feed refresh. Cancel just closes.

## Wiring the settings through

**GitHub token.** Today `release.rs` reads `GITHUB_TOKEN` from the env in one
place (the changelog and wire fetch). Since `settings.json` lives in the same
app-data dir that intel already receives as `cache_dir`, add a helper
`github_token(cache_dir)` that reads the `githubToken` field from `settings.json`
and falls back to the env var when the field is empty or the file is absent. Both
the changelog fetch and the wire fetch use it. No command signatures change. The
stored token never leaves the machine except as the `Authorization` header on
GitHub API calls (same as the env var does today).

**Source enable/disable.** The one place signatures grow:

- `scan_all(pins, sources)` skips any ecosystem whose flag is off (no rows, not
  even shelled out). The `scan_installed` command reads the stored `sources` and
  passes them.
- `search_all(query, cache_dir, sources)` skips a disabled registry the same way.
  The `search_registry` command passes them.
- What's New needs no separate filter: its security scan and verdicts operate on
  the installed list the frontend sends, which already reflects only the enabled
  sources after a scan.

Defaults (all sources on, empty token) reproduce today's behavior exactly; this
is purely additive. Disabling a source takes effect on the next scan (which Save
triggers), no relaunch needed.

## Export library

File menu: **Export as JSON** and **Export as Markdown**. The frontend serializes
the current library - JSON as an array of the tool objects, Markdown as a table
(name, source, installed -> latest, publisher, size). A backend command
`export_library(format, content)` writes the file.

Destination: write to the app-data dir with a dated filename
(`napm-library-<date>.json` / `.md`) and reveal it in Finder via `open`. No new
plugin; honest; the reveal makes it findable. A real "Save As..." picker (Tauri
dialog plugin) is deferred.

## Error handling

- `set_settings` and `export_library` are best-effort: a write failure degrades
  silently (the dialog still closes; export just does not reveal).
- An empty or whitespace token is treated as "no token" (falls back to env).
- A malformed `settings.json` reads as defaults (all sources on, no token).
- Disabling every source is allowed; the status bar honestly shows the resulting
  count rather than erroring.

## Testing

- The `Settings` round-trip and corrupt-file-reads-as-defaults get unit tests
  alongside the existing store tests.
- `scan_all` / `search_all` source-skipping: pass a restricted source set and
  assert the disabled ecosystem is absent from results.
- The Preferences dialog, Export, and the new menu items are verified live,
  consistent with the rest of the frontend.

## Out of scope (deferred)

- Default-appetite setting (the dial owns appetite; not duplicated).
- A real "Save As..." file picker (needs the Tauri dialog plugin).
- Keyboard Shortcuts dialog and Alt+letter mnemonics (carried from M6).
