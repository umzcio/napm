# napm M9 - Manual / standalone installs (best-effort source)

**Date:** 2026-05-31
**Status:** Approved, ready for implementation plan
**Milestone:** M9 (see `docs/ROADMAP.md`)

## Goal

Add a fifth library source for CLI tools installed outside any package manager:
the ones dropped by `curl | bash` scripts or direct downloads (the xAI `grok`
CLI, Google Antigravity, loose scripts in `~/.local/bin`). napm's four
package-manager sources cannot see these because no package manager owns them.
Surface them honestly as "unmanaged," with best-effort version, real on-disk
size and change time, and filesystem-level actions only. Never imply an update
path that does not exist.

## Why this is hard

The challenge is not finding binaries, it is **not falsely claiming** ones that
something else already owns. A broad `$PATH` sweep on a real machine turns up
mostly managed tools: Homebrew Cellar symlinks (`aws -> /usr/local/aws-cli`),
`.app` bundle CLIs (`docker -> /Applications/Docker.app/...`), cargo/rustup
shims (`~/.cargo/bin/*`), and the npm/pip/npx globals napm already scans. The
genuinely-manual tools are a small minority. Exclusion is therefore the heart of
this milestone.

Observed on the owner's machine (the canonical live test fixtures):

- `~/.grok/bin/grok` is a symlink to `../downloads/grok-0.2.14-macos-aarch64`
  (version encoded in the target filename); `grok --version` also prints
  `grok 0.2.14 (...)`.
- The same grok binary is reachable four ways: `~/.grok/bin/grok`,
  `~/.local/bin/grok` (-> `~/.grok/bin/grok`), and the separate `agent` binary
  via `~/.grok/bin/agent` and `~/.local/bin/agent`. Symlink chains plus dedup
  are mandatory.
- `~/.local/bin` also holds genuinely-manual entries like `agy` and loose
  scripts (`render`, `ssh-setup`).
- `/usr/local/bin` and `/opt/homebrew/bin` are dominated by brew/app-bundle
  symlinks that MUST be excluded.

## Architecture

A new backend scanner module `src-tauri/src/scan/manual.rs` exposing
`scan_manual(other_names: &BTreeSet<String>) -> Vec<InstalledTool>`, gated by a
new `Sources.manual` flag exactly like the existing four sources. `scan_all`
calls it last, after npm/brew/pip/npx, passing the set of tool names those four
scanners already returned (used as a belt-and-suspenders exclusion by basename).

No new Tauri commands. The existing `scan_installed`, library UI, View menu,
Preferences, and right-click engine absorb the new source through small
additions. No shell logic in the frontend.

### Detection: sweep -> resolve -> exclude -> dedup

1. **Enumerate** every entry in every `$PATH` directory that is a regular file or
   symlink with an executable bit. Skip directories that do not exist or cannot
   be read (degrade to fewer results, never error).
2. **Resolve** each candidate through its full symlink chain to a real path
   (`std::fs::canonicalize`). A candidate that fails to canonicalize (broken
   symlink) is dropped.
3. **Exclude** when the resolved real path lands under any managed root:
   - **Homebrew:** the resolved `brew --prefix` if `brew` is available, plus the
     hardcoded fallbacks `/opt/homebrew`, `/usr/local/Cellar`,
     `/usr/local/Homebrew`.
   - **App bundles:** any path containing `.app/`.
   - **Toolchain / version managers:** `~/.cargo`, `~/.rustup`, `~/.nvm`,
     `~/.pyenv`, `~/.volta`, `~/.asdf`, `~/go/bin`. This is a single tunable
     constant list.
   - **OS / system dirs:** `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`,
     `/usr/libexec`, `/System`, `/Library/Apple`.
   - **Already-managed by napm's other scans:** resolved path under an npm global
     prefix, a pip site-packages tree, or `~/.npm/_npx`; AND, independently, the
     candidate's basename appears in `other_names` (the names npm/pip/npx/brew
     returned this scan). Either condition excludes.
4. **Dedup** by resolved real target path, keeping one row per distinct binary.
   grok's multiple PATH entries collapse to one; the separate `agent` binary
   remains its own row.

The exclusion predicate is a pure function of `(resolved_real_path, basename,
other_names, managed_roots)` so it is unit-testable without a filesystem.

### Versioning: hybrid (layout first, then --version)

For each surviving tool, in order, stopping at the first that yields a version:

1. **Layout / metadata (free, safe, no execution):**
   - A semver in the resolved target's filename, e.g.
     `grok-0.2.14-macos-aarch64` -> `0.2.14`. Match the first
     `\d+\.\d+(\.\d+)?` token in the filename.
   - A sibling or parent `VERSION` file (`<dir>/VERSION`, `<dir>/../VERSION`)
     containing a semver.
   - A version path component (a path segment that is itself a semver).
2. **Execution fallback (`<tool> --version`):** ONLY if step 1 found nothing AND
   the resolved real path is under the user's home directory (`~/...`). Run the
   binary with arg `--version`, then `-v`, then `version`, each with a hard ~2s
   timeout; take the first that exits and prints a parseable semver-ish token
   (first `\d+\.\d+` match in stdout or stderr). A binary whose real path is
   system-wide is NEVER executed.
3. If nothing yields a version, leave it blank. `installed` is `Some("")` when
   the tool exists but has no resolvable version (it is installed, just
   unversioned); `latest` is always set equal to `installed` so no update is ever
   implied.

The filename and stdout parsers are pure and unit-tested.

### Row shape

Each manual tool is an `InstalledTool` with:

- `eco: "manual"`, `name` = the PATH basename (e.g. `grok`), `pkg` = same.
- `installed` = best-effort version string (possibly empty), `latest` = same
  value (never a different "latest").
- `publisher` = `"local"` (rendered in the Shared By column; there is no real
  publisher for a manual install).
- `description` = the resolved real path (genuinely useful: it tells the user
  exactly where the tool lives).
- `size` = on-disk size of the resolved target via the existing `size` helpers.
- `updated` = mtime of the resolved target via the existing `path_mtime`.
- `pinned` = honored from the pins set like any other row (pins key on `pkg`).
- `requested` = true (a manual install is always user-chosen).

## Frontend changes

Small, additive, reusing existing machinery.

- **Status rendering.** Introduce an honest "unmanaged" state for `eco ===
  "manual"`. `statusOf` checks `eco === "manual"` FIRST and returns
  `"unmanaged"`, before the existing `!installed -> "offline"` /
  `installed === latest -> "current"` logic. This ordering matters: an
  unversioned manual tool carries `installed: ""`, which is falsy in JS, so
  without the manual check first it would wrongly render "offline". The status
  badge renders neutral gray with the label "unmanaged" (not the green
  "current", which would falsely imply "up to date"). The latest-version cell
  renders a plain dash placeholder ("-") for manual rows.
- **No update participation.** Manual rows are excluded from the appetite-dial
  safe/held classification, the "N safe to take" count, and Update All (they have
  no `latest` to move to). The existing classification already keys off a real
  version delta; `"unmanaged"` simply is neither safe nor held.
- **Row actions.** No Get / Update / Roll back button for manual rows. The
  right-click menu for a manual row offers only filesystem actions: Reveal in
  Finder (`open_external` reuse / `open -R`), Copy path, Copy resolved target,
  Copy the tool name. (The library context menu gains a manual branch; the
  package-manager items are omitted for `eco === "manual"`.)
- **View menu.** Add a `manual` per-source toggle alongside npm/brew/pip/npx,
  persisted in the existing localStorage `napm.view`.
- **Preferences.** Add a `manual` source checkbox to the four existing source
  toggles. The settings store gains the field (default true).
- **Search.** Manual is NOT a searchable source. No Search source chip is added
  and `search_all` never touches it. The Search tab is unchanged.
- **What's New.** Manual rows are excluded from the OSV scan and the verdict feed
  (no ecosystem), the same way brew is excluded today. They simply do not appear
  as cards.

## Data model / persistence

- `Sources` gains `pub manual: bool`, defaulting to true, in both the struct and
  its `Default` impl. The existing `#[serde(rename_all = "camelCase", default)]`
  on `Sources`/`Settings` means an old `settings.json` without the field reads
  manual as true (all sources on), and a partial file never drops it. Mirror the
  field in the frontend Preferences and View toggles.
- No other store changes. Pins and history already key on `pkg` and work for
  manual rows unchanged (though history will rarely have manual entries since
  there are no install ops for them).

## Reveal-in-Finder action

`open -R <path>` reveals a file in Finder (already used by `export_library`).
The manual row's "Reveal in Finder" routes through the existing reveal path so
no new command is strictly required; if a dedicated command reads cleaner, a thin
`reveal_in_finder(path)` that shells `open -R` (validating the path exists) is
acceptable. Either way, the path is validated and no arbitrary shell string is
interpolated.

## Errors and edge cases

- A `$PATH` dir that does not exist or is unreadable is skipped silently.
- A broken symlink (fails to canonicalize) is dropped, never surfaced.
- A binary with no resolvable version shows a blank version and the "unmanaged"
  badge, never a fabricated one.
- `--version` that hangs is killed by the ~2s timeout; that tool shows a blank
  version rather than blocking the scan.
- A tool that is BOTH on PATH manually and owned by a package manager is excluded
  from the manual source (the managed scanners own it); it appears once, under
  its real ecosystem.
- The exclusion list is conservative-by-omission: an unusual managed dir not on
  the list could show a false "manual" row. This is acceptable and tunable; the
  list is a single constant.
- Performance: the sweep is many `lstat`/`canonicalize` calls (cheap) plus
  `--version` execution only on the few user-home survivors, each time-bounded.
  Total cost is dominated by the bounded executions, not the stat walk. The
  manual scan runs alongside the existing four; if it proves slow it can move
  into the same concurrency used elsewhere, but a sequential first cut is fine.

## Testing

Pure functions, unit-tested without touching the real filesystem:

- **Version-from-filename:** `grok-0.2.14-macos-aarch64` -> `0.2.14`;
  `tool-v1.2` -> `1.2`; a name with no version -> none.
- **Version-from-stdout:** `grok 0.2.14 (e0d895d)` -> `0.2.14`; `v1.4.0` ->
  `1.4.0`; noise with no semver -> none.
- **Exclusion predicate:** given a resolved path plus the managed-roots list and
  an `other_names` set, assert that a Homebrew Cellar path, an `.app` path, a
  `~/.cargo` path, a `/usr/bin` path, and a basename in `other_names` are all
  excluded, while a `~/.local/bin/agy` real path passes.
- **Dedup-by-real-target:** several rows resolving to the same target collapse to
  one; distinct targets stay separate.
- **Sources default + serde:** `manual` defaults to true; a `settings.json`
  missing the field reads manual = true; a partial file disabling only npm keeps
  manual on. (Extends the existing store tests.)

Verified live on the owner's machine: grok and `agy` appear as single
"unmanaged" rows with the right versions (grok `0.2.14` from the filename), brew
/ Docker / cargo tools do NOT appear, the View and Preferences manual toggles
hide/show the source, and the right-click filesystem actions work (Reveal in
Finder opens the right folder, Copy path/target/name put the right text on the
clipboard). The milestone-end adversarial review checks the exclusion filter for
false positives and the version logic for fabricated versions.

## Out of scope (deferred)

- Identifying install origin or any re-run-install / project-link affordance
  (origin is usually unknowable; filesystem actions only for v1).
- Non-executable manual tools (app data, libraries without a CLI entry point).
- Treating version managers (cargo, nvm, pyenv, go) as their own ecosystems -
  that is M11-adjacent; here they are simply excluded.
- Any update / latest / rollback for manual tools. There is no reliable source
  for it and faking it is forbidden.
