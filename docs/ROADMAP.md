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
  repo at github.com/umzcio/napm, not yet pushed).

## Next: M3 - Transfers (real execution + history)

The pane where versions actually change. Makes the Get / Update All buttons real.

- Run the real install/update/rollback commands; stream stdout/stderr live into
  the active transfer row; show the honest exit code (success/failure), not a
  fake progress bar.
- **Persistent history store** (SQLite or JSON in the app-data dir): log every
  install/update/rollback with timestamp and from/to, surviving restarts. This
  is what answers "claude-code started misbehaving - what changed and when."
- **Rollback**: npm (`npm i -g pkg@ver`) and pip (`pip install pkg==ver`) work;
  brew is gated honestly (no old bottles); npx offers Promote to global.
- Makes **pins** real (persisted, actually exclude from Update All) and Update
  All actually execute the safe+unpinned set chosen by the appetite dial.
- The shared `run()` helper will need to surface stderr and exit codes (M2 only
  read stdout).

## M4 - Search the swarm (registry discovery)

Find and install tools you do NOT have yet (Library tracks what you have; Search
discovers what you don't).

- Federated across npm + brew by default, with source-filter chips. Sorted by
  weekly downloads (the trust signal), flame marker on heavily-shared packages.
- npm: registry search + downloads API. brew: cached full formula catalog,
  searched in process. pip: exact-name lookup only (PyPI has no search API),
  labeled honestly.
- Installing from a result runs the same install path as Transfers.
- **Note:** the Search tab currently shows leftover seeded demo data - a live
  violation of the "everything real" rule. Either wire it in M4 or show a
  "not wired yet" state sooner.
- Brings the network + caching layer that also enables npx "latest" freshness.

## M5 - What's New (the decision feed)

One card per available update: should you take it, and why.

- `security` (advisory exists, always take), `safe` (settled, no advisories),
  `hold` (fresh + elevated issue velocity - deferred scoring). Changelog from
  GitHub releases.
- Layers on top of the appetite dial: the dial is the quick offline policy;
  What's New is the detailed network-backed justification and can override
  (fresh major with a bug spike stays hold even on bleeding-edge; a security
  patch is always safe). Adds a stricter "security-only" notch at the dial's
  far left.

## M6 - Menu bar (File/Edit/View/Swarm/Help)

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

A real signed/bundled macOS `.app` for daily use. Fix the bundle identifier
(currently `com.tauri.dev` -> `com.napm.app`). Resolve the npm/brew PATH for
Finder-launched apps (dev inherits the terminal PATH; a bundled app may not).

## Deferred on purpose

- `hold` issue-velocity scoring (needs GitHub issue-rate data + judgment) - v1.5.
- npx usage-frequency intelligence (rank by how often you npx a tool) - v1.5.
- Cross-platform (macOS only for now).
