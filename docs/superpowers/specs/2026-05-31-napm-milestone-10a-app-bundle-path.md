# napm M10a - Real app bundle + PATH

**Date:** 2026-05-31
**Status:** Approved, ready for implementation plan
**Milestone:** M10a (see `docs/ROADMAP.md`, "M10 - Packaging")

## Goal

Produce a real macOS `.app` that runs from `/Applications`, finds all of the
user's tools when launched from the Dock/Finder, shows the npstr icon and the
correct version, and preserves existing history across the bundle-identifier
change. This is the first half of packaging; signing, notarization, the DMG
installer, and the auto-updater are M10b.

The riskiest, napm-specific piece is PATH resolution, so it leads.

## The PATH problem (the crux)

A GUI app launched from Finder/Dock does NOT inherit the user's shell PATH. macOS
gives it a bare `/usr/bin:/bin:/usr/sbin:/sbin`. napm shells out to `npm`,
`brew`, `pip3`, and the M9 manual scanner literally walks `$PATH`, so a bundled
napm launched from the Dock would find almost nothing: no Homebrew
(`/opt/homebrew/bin`), no npm globals, no pyenv, no manual tools. Under
`tauri dev` the bug is invisible because the binary inherits the terminal's PATH.

### Fix: capture the login-shell PATH at startup

At the very top of `run()` (before the Tauri builder, before anything spawns,
including the existing brew warm-up thread), capture the user's real shell PATH
and set it on napm's own process environment with `std::env::set_var("PATH", ...)`.
Every child process spawned afterward inherits it, and the manual scanner's
`$PATH` walk starts working from the Dock.

Mechanics, designed to be robust and non-hanging:

- Read the shell from `$SHELL`; fall back to `/bin/zsh` if unset.
- Run it as an interactive login shell so it sources both the login files
  (`.zprofile`/`.bash_profile`) and the interactive rc files (`.zshrc`/`.bashrc`),
  since PATH edits live in either: `"$SHELL" -ilc '<probe>'`.
- Use a sentinel-delimited probe so rc-file noise (banners, `echo`s) cannot
  corrupt the result. The probe prints `__NAPM_PATH_START__$PATH__NAPM_PATH_END__`,
  and we extract exactly the text between the markers:
  `printf '__NAPM_PATH_START__%s__NAPM_PATH_END__' "$PATH"`.
- Bound the call with a hard timeout (~2s, reusing the spawn-poll-kill pattern
  already in `scan/manual.rs::run_with_timeout`) so a misbehaving rc file can
  never hang startup.
- Apply the captured PATH only if it is non-empty and contains at least one `/`
  (a sanity check); otherwise leave the inherited PATH untouched. In `tauri dev`
  the captured value equals or supersets the existing PATH, so this is a safe
  no-op there.

### Pure, testable seam

The extraction is a pure function:

```rust
/// Pull the PATH from sentinel-delimited shell output. Returns the trimmed
/// substring between the markers, or None if absent/empty.
pub fn extract_path(output: &str) -> Option<String>
```

Tested against: a clean `__NAPM_PATH_START__/opt/homebrew/bin:/usr/bin__NAPM_PATH_END__`,
the same wrapped in rc-file noise lines, missing markers (-> None), and empty
between markers (-> None). The shell invocation itself is thin glue verified
live.

## Bundle identifier + app-data migration

Change the identifier in `tauri.conf.json` from `com.tauri.dev` to `com.napm.app`.

The store lives at `app.path().app_data_dir()` =
`~/Library/Application Support/<identifier>/`, so this change moves the directory
and would orphan the existing `history.json` (and `pins.json`/`settings.json` if
present). Add a one-time, best-effort migration:

```rust
/// Copy the user-data files from a legacy app-data dir into the current one,
/// only for files that do not already exist in the current dir. Caches are not
/// migrated (they regenerate). No-op if the legacy dir is absent.
pub fn migrate_legacy(current_dir: &Path, legacy_dir: &Path)
```

- Files migrated: `pins.json`, `history.json`, `settings.json` only.
- For each, copy from `legacy_dir` to `current_dir` ONLY when the target does not
  already exist (never clobber newer data).
- The legacy dir is derived from the current dir's parent:
  `current_dir.parent().join("com.tauri.dev")` (no hardcoded home path).
- Called once in the `.setup()` hook, before the brew warm-up, after computing
  the app-data dir. Best-effort: any IO error is ignored, never blocks startup.

Tested with temp dirs: copies when target missing, skips when target present
(no clobber), no-op when legacy dir absent, ignores cache files.

## Version + metadata

- Version is already `0.1.0` in both `tauri.conf.json` and `Cargo.toml`; confirm
  they stay in sync (`tauri.conf.json` is the single source the titlebar reads
  live). No bump now; the `1.0.0` public-release bump is M10b.
- Fill the placeholder metadata in `src-tauri/Cargo.toml`: `description` (a real
  one-line description, no em dashes), `authors` (`["umzcio <umzcio@users.noreply.github.com>"]`),
  `license` (`"MIT"`), `repository` (`"https://github.com/umzcio/napm"`).
- Leave the crate `name = "app"` and `[lib] name = "app_lib"` unchanged; they are
  internal and renaming them is churn with no packaging benefit. `productName`
  (`"napm"`) already drives the `.app` name.

## Icon verification

The npstr `icon.icns` is already generated and wired into `bundle.icon`. It only
embeds in a packaged `.app` (the unbundled `tauri dev` binary shows a generic
placeholder). No code change; this milestone runs a real `tauri build`, installs
the resulting `napm.app` to `/Applications`, and verifies the npstr logo appears
in the Dock, Finder Get-Info, and the in-app About panel.

## File structure

- Create: `src-tauri/src/pathenv.rs` - `extract_path` (pure) + `fix_path()` (the
  shell capture + `set_var`). Declared `mod pathenv;` in `lib.rs`.
- Modify: `src-tauri/src/lib.rs` - call `pathenv::fix_path()` as the first line of
  `run()`; call the migration in `.setup()`.
- Modify: `src-tauri/src/store.rs` - add `migrate_legacy(current, legacy)` + tests.
- Modify: `src-tauri/tauri.conf.json` - identifier `com.napm.app`.
- Modify: `src-tauri/Cargo.toml` - fill description/authors/license/repository.

## Errors and edge cases

- `$SHELL` unset or the probe failing/timing out: leave the inherited PATH as is.
  napm still runs; from a terminal launch it works, and the failure is logged.
- Non-POSIX login shells (notably `fish`) will not accept the `-ilc 'printf ...'`
  probe; `extract_path` returns None and we keep the inherited PATH. Accepted v1
  limitation: a fish user launching from the Dock falls back to the bare PATH. A
  shell-specific probe is a later refinement, not in M10a scope.
- A captured PATH missing a tool's dir (exotic setup): same best-effort behavior
  as today; the user can still see the tool if it is on the captured PATH.
- Identifier change with no legacy dir (fresh machine): migration is a clean
  no-op.
- Migration never clobbers: if the user has already run the new build and created
  history under `com.napm.app`, re-running migration does nothing.
- The captured PATH is set process-wide via `set_var`, which is safe here because
  it happens once at the very start of `run()` before any threads spawn.

## Testing

- Unit: `extract_path` (markers, noise, missing, empty) and `migrate_legacy`
  (copy-when-missing, skip-when-present, absent-legacy no-op, caches ignored).
- Live (the real verification): `tauri build`, install `napm.app` to
  `/Applications`, launch from Finder (NOT a terminal), and confirm:
  - The Shared Library populates (npm/brew/pip/npx/manual all found) - proves the
    PATH fix.
  - Search and a real install/update both work from the Dock launch.
  - The npstr icon shows in Dock, Finder, and About; the titlebar shows
    `napm v0.1.0`.
  - Prior history is present (migration worked).
  - First launch requires right-click -> Open to pass Gatekeeper (expected; it is
    unsigned until M10b).

## Out of scope (M10b)

- Code signing (Developer ID), hardened runtime, entitlements.
- Notarization + stapling, the styled `.dmg` installer.
- The Tauri auto-updater (minisign keys, `latest.json`, in-app update UI).
- Local build/release scripts and going public on GitHub.
- The `1.0.0` version bump.
