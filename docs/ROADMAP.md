# napm Roadmap

Living roadmap for napm (npstr AI Package Manager). The original design spec is
`docs/superpowers/specs/2026-05-30-napm-real-app-design.md`; this file tracks
current status and the path forward. Guiding principle: keep the late-90s
file-sharing look and feel, but **every element must carry real data or
function** (no decorative jokes).

## Done

- **M1 - Tauri shell + npm Shared Library.** Frameless native macOS window where
  the Win98 chrome IS the app (no native title bar). Real npm global scan
  driving the library. Native Rust backend, vanilla-JS frontend via `invoke()`.
- **M2 - brew, pip, npx sources.** All four ecosystems scanned offline and
  merged. npx is a first-class source (cached tools, neutral "freshness unknown"
  until M5). pip uses `pip3` detection and scans user + global site-packages.
- **Real-metadata pass** (on top of M1/M2, every column earns its place):
  - **Shared By** = real package publisher (author, else repository/homepage
    GitHub owner, else domain). ~97% coverage.
  - **Size** = real on-disk install footprint (dir walk / pip RECORD / brew keg).
  - **Updated** (replaced the fake **Ping**) = when the tool last changed on
    disk (brew INSTALL_RECEIPT.json time, else folder mtime), shown relatively.
  - **Description** = one-line summary under each tool name (offline).
- **Update Appetite dial** (replaced the dead "56k" throttle). Sets how big a
  version jump counts as "safe to take" (patch only / up to minor / bleeding
  edge), computed offline from semver distance. Re-classifies the library live
  (green safe vs amber held), drives a "N safe to take" count, scopes Update All,
  persists in localStorage.
- **Branding.** npstr cat-box logo as app icon, titlebar, and splash. No
  decorative emoji. House-style README, MIT license, scrubbed git history (public
  repo at github.com/umzcio, not yet pushed).
- **M3 - Transfers (real execution + history).** The Get / Update All / rollback
  buttons run real package commands. `run_op` streams live stdout/stderr into the
  active transfer row and reports the honest exit code, no fake progress bar.
  Every install/update/rollback is logged to a persistent JSON history store
  (app-data dir) with timestamp and from/to, surviving restarts. Rollback works
  for npm (`npm i -g pkg@ver`) and pip (`pip install pkg==ver`); brew is gated
  honestly (no old bottles); npx offers Promote to global. Pins are real
  (persisted, excluded from Update All).
- **M4 - Search the swarm (registry discovery).** The Search tab runs live
  federated queries across npm + brew + pip, replacing the seeded demo data.
  One "Find It!" search hits the whole swarm at once; results merge into one flat
  list sorted by weekly downloads (the trust signal), flame marker on the
  heavily-shared ones. Source chips (all / npm / brew / pip) default to all and
  narrow the view client-side, mapping to Napster's optional filter dropdowns.
  npm uses the registry search + downloads API; brew searches a cached formula
  catalog joined with 30-day analytics; pip is exact-name PyPI lookup only,
  tagged "exact match" so the gap is visible. Each source fails independently so
  one dead registry never blanks the grid. Installing from a result runs the same
  M3 Transfers path. New `src-tauri/src/search/` module (blocking `ureq` behind a
  single `http.rs`), one `search_registry` command.

## Done: M4.1 - Search performance

M4 was built blocking and sequential to get it correct first. A warm search took
a few seconds, which was too slow. This pass removed the avoidable latency
without changing behavior or honesty (verified live, much faster):

- **Run the three sources concurrently** (`std::thread::scope`) so total time is
  the slowest source, not the sum of all three. Biggest single win.
- **One process-global `ureq` agent** so connections keep-alive instead of a new
  TLS handshake per request.
- **Hold the parsed brew catalog in memory** (parse once per process, not the
  multi-MB re-parse every search), behind the existing 24h disk cache.
- **Warm the brew catalog in a background thread at launch** so the first search
  is never cold.
- **Parallelize npm's per-scoped download lookups** instead of one sequential
  call per scoped package.

Result: a warm search is well under a second. Pure parsers and the
fail-independently behavior are preserved.

## Done: M5 - What's New (the decision feed + security intelligence)

Real release and security intelligence per tool, behind the existing card UI.
Three layers, priority order malicious -> vulnerable -> safe -> new, with a
supply-chain wire above everything.

- **Layer 1 - protect what you have.** One batched OSV query over EVERY installed
  tool at its current version (not just outdated). Flags `malicious` (OpenSSF
  malicious-packages / GitHub malware data, the supply-chain hijacks) and
  `vulnerable` (CVE/GHSA). Runs regardless of the appetite dial. A truncated or
  failed OSV response is treated as a failed check, never a clean result.
- **Layer 2 - the supply-chain wire.** A bulletin strip fed by GitHub's recent
  npm + pip malware advisories, cached hourly. Surfaces big ecosystem events even
  for packages you do not have.
- **Layer 3 - update verdicts.** Age-based `safe` (settled, older than 7 days) or
  `new` (fresh, little signal yet) for the appetite-scoped update set. Changelog
  loaded lazily on card expand from the upstream GitHub releases (repo derived
  offline from package metadata). The real issue-velocity `hold` stays v1.5.
- **Honesty.** npm + pip fully covered; brew is excluded from the OSV scan (no
  Homebrew ecosystem) and labeled, not shown as clean. The core rule throughout:
  never imply "safe" when a check could not run. A malicious package with no fix
  shows a copyable remove command, not a fake one-click.
- New `src-tauri/src/intel/` module (OSV, wire, release) behind a shared
  crate-root `http`; commands `get_whats_new` and `get_changelog`. Keyless by
  default; `GITHUB_TOKEN` raises the rate limit if present.

Deferred to v1.5: issue-velocity `hold`, brew/system-tool CVE mapping
(Debian/Alpine), a one-click uninstall op, and the appetite dial's "security-only"
far-left notch.

## Done: M6 - Menu bar (File/Edit/View/Swarm/Help)

The inert Win98 menu bar now works: real beveled dropdown menus from a small
data-driven engine (one open at a time, hover-switch, click-outside / Escape to
close, checks for toggles and dots for the sort radio).

- **View** (the standout, persisted in localStorage): **only tools I installed**
  (hides brew dependencies via `installed_on_request`, the ~287 -> ~40 collapse),
  only outdated, per-source toggles (npm/brew/pip/npx), sort by
  name/size/updated/status, and a descriptions toggle. `renderRows` filters and
  sorts a derived list while each row keeps its original index, so selection,
  pins, and Get keep working.
- **File**: Rescan now, Open data folder (Finder), Quit.
- **Swarm**: Refresh registry caches (deletes the cached brew/wire files, drops
  the in-memory catalog, re-warms in the background).
- **Help**: About napm (a Win98 modal with the npstr logo + live version + repo
  link), Repo on GitHub.
- **Edit**: Copy tool details (selected row to clipboard; greyed when nothing is
  selected).
- Backend: a `requested` flag on `InstalledTool` (brew `installed_on_request`,
  unknown -> shown) and three thin commands (`open_data_dir`, `open_external`
  https-only, `clear_caches`). No new plugins; `open` via std::process.

Deferred to M7 (Preferences / Settings, now next): the Preferences dialog and the
persisted settings store (GitHub token field, default-appetite setting,
enable/disable sources), plus Export library (JSON / Markdown), Keyboard
Shortcuts, and Alt+letter mnemonics.

Ordering note: Packaging moved to the back (now M10) so feature work continues
while the app stays local and fast to iterate. The signed .app comes once the
feature set is settled.

## Done: M7 - Preferences / Settings

A persisted settings store, a Win98 Preferences dialog, and Export library.

- **Settings store:** `settings.json` in the app-data dir (same JSON layer as
  pins/history), `{ githubToken, sources: {npm,brew,pip,npx} }`, defaults to empty
  token + all sources on; corrupt or partial file reads as defaults (never drops
  unspecified sources to off).
- **Preferences dialog** (Edit -> Preferences..., reusing the M6 modal): a GitHub
  token field and the four source toggles. Save persists then rescans. Appetite
  stays on the dial (not duplicated here).
- **Wiring:** the stored token is read by the intel HTTP calls (changelog + wire)
  with the `GITHUB_TOKEN` env var as fallback; disabled sources are skipped by
  `scan_all` and the federated `search_all`. Default settings reproduce prior
  behavior exactly.
- **Export library:** File -> Export (JSON / Markdown), written to the app-data
  dir as `napm-library-<date>.{json,md}` and revealed in Finder.

Deferred: a real "Save As..." picker (needs the Tauri dialog plugin), Keyboard
Shortcuts, and Alt+letter mnemonics. Appetite remains owned by the dial.

## Done: M8 - Right-click context menus (throughout)

Shipped: the M6 menu engine generalized into a cursor-positioned `openPopup`, and
four context menus (library, search, transfers, history) routing through existing
actions. Library: update/install, history-driven rollback (brew-gated), pin,
copy name/install-command, open registry page (npmjs/brew/pypi via
`open_external`), jump to What's New. Search: get, copies, open page, filter to
source. Transfers: copy log/command, re-run (re-resolves the tool by package).
History: rollback, copy entry, jump to tool. Registry page used instead of a
homepage field (no backend change).

Original design notes below.

Right-click anywhere meaningful and get a Win98-style beveled context menu of
the actions that apply to whatever was clicked. This is deeply period-accurate
(the old file-sharing clients leaned on right-click menus) and it is the fastest
path to actions that are currently buried in row buttons or not exposed at all.
Pairs with M6: reuse the same beveled menu chrome the menu bar uses.

Every item must do something real. Disabled items are allowed only when honestly
unavailable (e.g. brew rollback), shown greyed with a reason, never as filler.

Per surface, the menu is context-aware:

- **Library row:** Get / Update or Install (mirrors the row button); Roll back to
  a previous version (gated for brew, as today); Pin / Unpin; Copy package name;
  Copy the exact install command; Open homepage / repo (from the publisher
  metadata already scanned); jump to this tool's What's New card.
- **Search result:** Get / Install; Copy package name; Copy install command; Open
  the registry page (npmjs.org / formulae.brew.sh / pypi.org); filter the swarm
  to this source.
- **Transfers row:** Copy the streamed log output; Re-run; Roll back to this
  version; Copy the from -> to.
- **History entry:** Roll back to this version; Copy the entry; jump to the tool
  in the library.

Build notes: one reusable context-menu component (positioned at the cursor,
dismiss on outside-mousedown / Escape, stays inside the window bounds), fed a
per-surface action list. Actions route through the existing commands (install via
the M3 Transfers path, pins via `set_pin`, etc.); "Open ..." links use the
`open_external` command with the homepage/repo from the scanned metadata. No
shell logic in the frontend.

## Done: M9 - Manual / standalone installs (best-effort source)

A fifth library source for tools installed outside any package manager (the ones
dropped by `curl | bash` scripts or direct downloads, like the xAI `grok` CLI at
`~/.grok/bin/grok`). A new `scan/manual.rs` does a broad `$PATH` sweep, resolves
each binary through its symlink chain, and surfaces only what no package manager
owns. The hard part is exclusion, not detection.

- **Detection:** broad `$PATH` sweep -> canonicalize -> exclude anything under a
  managed root (Homebrew prefix + resolved `brew --prefix`, `.app` bundles,
  cargo/rustup/nvm/pyenv/volta/asdf/go, the npm global module dir from
  `npm root -g`, the pip user script dir, OS/system dirs) or already returned by
  the npm/brew/pip/npx scans -> dedup by resolved target (grok's several PATH
  entries collapse to one). `/usr/local/bin` is intentionally NOT excluded since
  it is a common manual-drop target.
- **Version (hybrid):** a token in the resolved filename first (free, no
  execution; `grok-0.2.14-macos-aarch64` -> `0.2.14`); only if that fails and the
  binary resolves under `$HOME`, a timeout-bounded `<tool> --version` / `version`
  (never `-v`, never a system-wide binary). Blank when unknown, never fabricated.
- **Status:** a distinct neutral "unmanaged" badge (not green "current"). No
  latest, no Update, no Roll back, and excluded from Update All, the safe-count,
  the outdated count, and What's New. Right-click offers filesystem actions only:
  Reveal in Finder, Copy path, Copy name, Copy version.
- **Wiring:** a `manual` flag on `Sources` (default on), a View-menu toggle, and a
  Preferences checkbox. Not a searchable source (no registry).

Found and fixed in the milestone-end review: npm/npx globals and pip user scripts
leaking in as "manual" (the name-based exclusion could not catch them because the
scan rows carry package/distribution names, not binary basenames; fixed by adding
the `npm root -g` and pip user-script path roots), and an over-broad first cut
that wrongly excluded all of `/usr/local/bin`.

## M10 - Packaging

A real signed/bundled macOS `.app` for daily use.

- Fix the bundle identifier (currently `com.tauri.dev` -> `com.napm.app`).
- Resolve the npm/brew PATH for Finder-launched apps (dev inherits the terminal
  PATH; a bundled app may not).
- Set the real shipping version in `tauri.conf.json` / `Cargo.toml`. The titlebar
  wordmark now reads it live (`napm v<version>`), so packaging is the single place
  that version is set and it flows everywhere.
- The npstr logo icon (`icons/icon.icns`, already generated and configured) only
  embeds in a packaged `.app`. In `tauri dev` the unbundled binary has no icon, so
  the Dock, Finder, and the About panel show a generic placeholder. Verify the
  packaged build shows the npstr logo in all three.

## M11 - AI tooling ecosystem (skills + MCP connectors)

A candidate post-packaging milestone, wants a short research spike first. Treat
skills and MCP servers as new best-effort ecosystems in the swarm, alongside
npm/brew/pip/npx. The version and update story is less standardized than the
package managers, so this is best-effort and labeled honestly, in the same
spirit as the pip-search gap and the M9 manual source.

- **MCP connectors.** Partly covered already, since many MCP servers ship as npm
  or pip packages and ride the existing scanners. The new, napm-specific value is
  the connector config layer: scan what is actually wired up in `~/.claude.json`,
  Claude Desktop's `claude_desktop_config.json`, and project `.mcp.json` files,
  then show each server's backing package, version, and whether it is outdated.
  A "what connectors do I have plugged in, and are any stale" view that does not
  exist anywhere today.
- **Skills / plugins.** Claude Code plugins under `~/.claude/plugins/` have a real
  marketplace install and version model. Scan installed plugins and check their
  marketplace source for updates.
- **Honesty.** Where a source or version cannot be resolved, label "unmanaged"
  rather than implying an update path, exactly as M9 does for curl|bash tools.

Research spike (do first): pin down the config file formats, where each install
type records its version, and what "latest" reliably means per type before
committing to a scanner.

## Deferred on purpose

- `hold` issue-velocity scoring (needs GitHub issue-rate data + judgment) - v1.5.
- npx usage-frequency intelligence (rank by how often you npx a tool) - v1.5.
- **npx latest-drift hint** (display-only): resolve the real npm latest for npx
  rows and show a passive "last ran X, latest Y" hint with no Update action,
  since the action is just re-running. Never a fake update path - v1.5.
- Cross-platform (macOS only for now).
