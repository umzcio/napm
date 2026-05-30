# napm — design spec: turning the prototype into a real app

Date: 2026-05-30
Status: Approved (pending final spec read)

## Product

**npstr AI Package Manager (napm)** = **n**pstr **A**I **P**ackage **M**anager. A desktop package manager for command-line dev tools wearing a late-90s peer-to-peer file-sharing client as its interface. It tracks the CLIs you have installed across npm, Homebrew, pip, and npx, tells you what is out of date and whether an update is safe, lets you search the registries and install from results, and keeps a rollback-able history.

The word "Napster" is never used in any user-facing string, filename, or metadata. No Napster trade dress or logo. The 90s flavor (dial-up splash, gray beveled chrome, VT323 wordmark, peer handles, decorative throttle slider, downloads-per-week trust signal with the flame marker) is intentional identity and is preserved exactly. No em dashes in generated copy or UI text.

## Goal and scope

This is a tool the owner will actually use on their own Mac. Correctness and daily usefulness lead; distribution does not. Therefore:

- **Target: macOS only** for v1. Written so the Rust shell logic does not gratuitously hardcode mac-isms, but no cross-platform testing or support is in scope.
- No code signing, notarization, installers, or public distribution in v1.
- The prototype `napm-prototype.html` is the source of truth for UX, layout, and visual identity. We do not redesign it.

## Architecture

A **Tauri** app. The backend is **native Rust** (no Node runtime shipped). Two layers:

1. **Rust backend** (`src-tauri/`) owns every shell command and all network calls, exposed to the frontend as Tauri commands invoked via `invoke()`. The frontend never shells out. This is the single source of truth.
2. **Frontend** is the prototype's HTML + CSS kept verbatim. The seeded `TOOLS` / `SWARM` / `FEED` arrays and the fake `setInterval` progress bars are removed and replaced with `invoke()` calls plus Tauri event listeners for streamed output. All era styling is untouched.

`scanner.js` is the logic reference; its batch detection is reimplemented in Rust.

### Rust module layout (each unit has one job, unit-testable in isolation)

- `scan/` — one submodule per source (`npm`, `brew`, `pip`, `npx`), each returns `Vec<InstalledTool>`.
- `semver.rs` — version comparison and the `current != latest` status derivation. Isolated and unit-tested.
- `ops.rs` — install / update / rollback / promote. Spawns the real command and streams stdout + stderr.
- `registry/` — npm search + brew cached index + pip exact lookup, plus weekly download counts. Cached.
- `whatsnew/` — GitHub Releases changelogs and advisory lookups; produces `ReleaseInfo`.
- `store.rs` — persistence (pins, history, registry/advisory caches).

### Frontend wiring

- Library/Search/What's New render from `invoke()` results instead of seeded arrays.
- Install/update/rollback emit Tauri events carrying stdout/stderr lines; the JS listener appends them to the active Transfers row, replacing the fake progress bar. The honest exit code drives success/failure display.
- The dial-up splash stays but reflects the real scan (real installed count and real number of updates found).

## Data model

The brief's four interfaces are canonical. One extension: `npx` joins the ecosystem set.

```ts
type Ecosystem = "npm" | "brew" | "pip" | "npx";

interface InstalledTool {
  name: string;
  eco: Ecosystem;
  pkg: string;
  installed: string | null;   // null means not installed
  latest: string;
  size: string;
  pinned: boolean;
  cachePath?: string;         // internal: npx rows, location under ~/.npm/_npx/
}

interface SearchResult {
  name: string; eco: Ecosystem; pkg: string; version: string;
  weeklyDownloads: number; size: string; description: string;
}

interface ReleaseInfo {
  pkg: string; version: string; ageLabel: string; changelog: string[];
  recommendation: "safe" | "hold" | "security";
  signals: { level: "good" | "warn" | "danger"; label: string; text: string }[];
}

interface HistoryEntry {
  ts: number; pkg: string;
  action: "install" | "update" | "rollback";
  from: string | null; to: string;
}
```

## Sources and scan strategy

Each source runs one batch command and merges by key. A source not present on PATH is skipped gracefully, never an error.

| Source | Installed | Latest | Rollback |
|--------|-----------|--------|----------|
| npm | `npm ls -g --depth=0 --json` | `npm outdated -g --json` (read stdout from the nonzero exit path) | Yes: `npm i -g <pkg>@<ver>` |
| brew | `brew list --versions` | `brew outdated --json=v2` | No: gated in UI (Homebrew keeps no old bottles) |
| pip | `pip list --format=json` | `pip list --outdated --format=json` | Yes: `pip install <pkg>==<ver>` |
| npx | scan `~/.npm/_npx/*/node_modules` | npm registry latest per package | n/a: offers Promote to global |

Status derivation: installed equals latest is current; they differ is update; no installed version is not installed.

### npx specifics

- npx tools appear as library rows with an `npx` source pill alongside npm/brew/pip.
- Each npx row offers a **Promote to global** action that runs the normal install path (`npm i -g <pkg>`), after which the row re-tags as `npm` and napm manages it for real.
- v1 only offers the button. Usage-frequency intelligence (ranking by how often a tool is npx-run, proactive nudges) is v1.5.

## Panes

### Shared Library
Real scan results. Status glyph (up arrow / check / x) derived in Rust. Pin toggles persist; a pinned tool stays visible as outdated but is excluded from Update All. Size comes from the real package where cheap to obtain. The "Shared By" handle and decorative ping use the kept deterministic flavor functions.

### Transfers (execution + memory)
Install/update/rollback/promote spawn the real command. stdout and stderr stream live into the active row via Tauri events, replacing the fake bar; the real exit code drives an honest success/failure result. Every operation logs to the history store with timestamp and from/to. History rows offer Roll back, gated for brew (disabled with a surfaced reason unless a cached bottle or pin exists).

### Search (the swarm)
Federated across npm + brew by default, results merged and sorted by weekly downloads, with source-filter chips to scope to a single registry. pip is exact-name lookup only via `https://pypi.org/pypi/<name>/json`, labeled honestly so the gap is visible. Sources:
- npm: `https://registry.npmjs.org/-/v1/search?text=<query>` plus `https://api.npmjs.org/downloads/point/last-week/<pkg>`.
- brew: full catalog from `https://formulae.brew.sh/api/formula.json`, fetched once, cached, searched in process; analytics from the same site for the downloads column.
- pip: exact-name resolution only.

Installing from a result runs the same install path and adds the package to the library. The flame marker on heavily-shared packages is preserved.

### What's New (the decision feed)
One card per available update.
- `security`: a real advisory exists (npm audit, GitHub Security Advisories, or OSV). Always recommend taking it.
- `safe`: release age past 7 days, no advisories.
- Fresh releases with no signal are labeled "new, little signal yet" rather than a fake verdict.
- `hold` (issue-velocity scoring) is explicitly deferred to v1.5.
- Changelog text pulled from the source's GitHub Releases where available.

## Persistence

**SQLite** via `rusqlite` (bundled), one file at `~/Library/Application Support/napm/napm.sqlite` (already covered by `.gitignore` as `napm.sqlite`). Stores pins, history, and the registry/advisory caches. History queryability is what makes "claude-code started misbehaving, what changed and when" answerable.

## Network and caching

Registry catalogs (notably brew's full `formula.json`) and GitHub release/advisory lookups are cached aggressively in the store with TTLs, for speed and to stay under rate limits. The GitHub token is read at runtime from the `gh` CLI (`gh auth token`) when the GitHub CLI is authenticated; otherwise unauthenticated with caching, with an optional environment-variable override.

## Build order

1. Tauri scaffold and frontend wired to a real **npm** Library scan, replacing seeded data end to end.
2. Add **brew**, **pip**, and **npx** scans.
3. **Transfers**: real streamed install/update, history store, rollback for npm and pip, brew gated, npx promote.
4. **Search**: npm, then brew cached index, then pip exact lookup, with source chips.
5. **What's New**: changelogs plus safe and security verdicts. Defer hold.
6. Polish and a real macOS `.app` bundle.

## Out of scope for v1

Cross-platform support, code signing / notarization / distribution, the `hold` issue-velocity verdict, npx usage-frequency intelligence, and any redesign of the interface.

## Repo notes

The real project currently lives inside `napm.zip` (contains `CLAUDE.md`, `prototype/napm-prototype.html`, `reference/scanner.js`, `README.md`, `package.json`, `.gitignore`). The top-level prototype was rebranded and renamed from `napster-package-manager.html` to `napm-prototype.html`. Unpacking the project into place and reconciling the canonical prototype is the first scaffolding step of implementation.
