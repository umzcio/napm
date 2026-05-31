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

## Next: M6 - Menu bar (File/Edit/View/Swarm/Help)

Make the inert Win98 menu bar do real things.

| Menu | Items |
|------|-------|
| **File** | Rescan now; Export library (JSON / Markdown); Open data folder; Quit |
| **Edit** | Preferences (default appetite, GitHub token, which sources to scan); Copy tool details |
| **View** | Filter: **only tools I installed** (collapses ~287 to ~40 using brew `installed_on_request`); only outdated; by source; Sort by name/size/updated/status; toggle descriptions |
| **Swarm** | Refresh registry caches; Enable/disable sources (npm/brew/pip/npx); jump to Search |
| **Help** | About napm; Keyboard shortcuts; Repo link |

Standout: **View -> "only tools I installed"** is high-value and works offline
today (most library rows are brew dependencies the user never chose).

## M7 - Packaging

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

## M8 - Manual / standalone installs (best-effort source)

A fifth source for tools installed outside any package manager - the ones
dropped by `curl | bash` install scripts or direct downloads. Examples on the
owner's machine: the **xAI CLI** (`curl https://x.ai/cli/install.sh | bash` ->
`~/.grok/bin/grok` -> `grok-0.2.14-macos-aarch64`) and the **Google Antigravity
CLI** (`curl https://antigravity.google/cli/install.sh | bash`). napm's four
package-manager sources cannot see these because no package manager installed
them.

This is honestly the hardest and most degraded source, label it clearly (like
the pip search gap):

- **Detection:** scan PATH plus known install dirs (`~/.grok/bin`,
  `~/.local/bin`, `/usr/local/bin`, tool-specific dirs) for executables that no
  package manager claims.
- **Version:** no manifest. Sometimes the layout encodes it (grok ->
  `grok-0.2.14-...`); otherwise fall back to running `<tool> --version` and
  parsing wildly varying output. Best-effort, often blank.
- **Latest / update:** no registry and no uniform updater. Most we can do is
  "re-run the install script" or link to the project; no reliable safe/held
  classification or one-click Get for these. Do NOT fake an update path that
  does not exist.

Scope: list the tool and a best-effort version, mark the source clearly as
"manual / unmanaged," and be honest that updates are mostly out of napm's hands.

## M9 - Right-click context menus (throughout)

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
  metadata already scanned); jump to this tool's What's New card (once M5 lands).
- **Search result:** Get / Install; Copy package name; Copy install command; Open
  the registry page (npmjs.org / formulae.brew.sh / pypi.org); filter the swarm
  to this source.
- **Transfers row:** Copy the streamed log output; Re-run; Roll back to this
  version; Copy the from -> to.
- **History entry:** Roll back to this version; Copy the entry; jump to the tool
  in the library.

Build notes: one reusable context-menu component (positioned at the cursor,
dismiss on outside-click / Escape, stays inside the window bounds), fed a
per-surface action list. Actions route through the existing commands (install via
the M3 Transfers path, pins via `set_pin`, etc.); "Open ..." links need a real
URL, so surface the homepage/repo from the scanned metadata rather than guessing.
No shell logic in the frontend.

## Deferred on purpose

- `hold` issue-velocity scoring (needs GitHub issue-rate data + judgment) - v1.5.
- npx usage-frequency intelligence (rank by how often you npx a tool) - v1.5.
- Cross-platform (macOS only for now).
