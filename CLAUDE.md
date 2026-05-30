# napm

`napm` is a desktop package manager for command-line dev tools, wearing a late-90s peer-to-peer file-sharing client as its interface. It tracks the CLIs you have installed across npm, Homebrew, and pip, tells you what is out of date and whether an update is safe to take, lets you search the registries and install straight from the results, and keeps a history you can roll back.

This file is the brief for turning the prototype into a real, working application. Read it fully before writing code.

## Your job

Make the prototype real. Do not redesign it. The prototype in `prototype/napm-prototype.html` is the source of truth for UX, layout, and visual identity. Open it in a browser to see every intended behavior. Your work is to replace its seeded fake data and fake actions with a real backend that scans the system and runs real package operations, while preserving the look and the interaction model exactly.

## Naming and legal

- The command and package name is `napm`. Use it everywhere.
- Do not use the word "Napster" in any user-facing string, filename, or metadata. The 90s-client styling is homage and parody, which is fine, but the brand name is not ours to use.
- Do not reproduce the Napster cat-with-headphones logo or any other element of its trade dress. The placeholder cat emoji is fine. Any custom artwork must be original.
- Keep the era flavor (dial-up splash, gray beveled chrome, peer handles, the decorative throttle slider). That tone is intentional product identity, not filler.

## Recommended architecture

`napm` needs privileged local shell access, because it runs `npm`, `brew`, and `pip`. That rules out a pure browser app. The recommended shape is two layers:

1. A local backend agent (Node) that owns all shell interaction and exposes a small, typed API. Nothing else in the app shells out. The agent is the single source of truth.
2. A frontend that is the prototype's UI, calling the agent.

Package it as a desktop app. Tauri is preferred for a small footprint (Rust shell wrapping the web UI). Electron is the faster path from this mock if you want to stay all-JavaScript. Pick one and note the choice in the README. A third option, suitable for a self-hoster, is to run the agent as a localhost daemon and serve the UI as a local page; this works but desktop packaging is friendlier for most users.

The agent API should cover roughly: `scanInstalled()`, `searchRegistry(query, source)`, `getReleaseInfo(pkg)`, `install(pkg, version)`, `rollback(pkg, version)`, `pin(pkg)`, `unpin(pkg)`. Keep all version-comparison and shell logic inside the agent.

## Data model

These shapes come from the prototype and are canonical. Mirror them.

```ts
type Ecosystem = "npm" | "brew" | "pip";

interface InstalledTool {
  name: string;          // display name
  eco: Ecosystem;
  pkg: string;           // registry identifier, e.g. "@anthropic-ai/claude-code"
  installed: string | null;  // null means not installed
  latest: string;
  size: string;
  pinned: boolean;
}

interface SearchResult {
  name: string;
  eco: Ecosystem;
  pkg: string;
  version: string;
  weeklyDownloads: number;
  size: string;
  description: string;
}

interface ReleaseInfo {       // drives the What's New feed
  pkg: string;
  version: string;
  ageLabel: string;           // "released 6 days ago"
  changelog: string[];
  recommendation: "safe" | "hold" | "security";
  signals: { level: "good" | "warn" | "danger"; label: string; text: string }[];
}

interface HistoryEntry {
  ts: number;
  pkg: string;
  action: "install" | "update" | "rollback";
  from: string | null;
  to: string;
}
```

## Features, and how to build each

### Shared Library (the installed view)

Populate from `scanInstalled()`, which merges three ecosystems. Run one batch command per ecosystem rather than one call per tool.

- npm: `npm ls -g --depth=0 --json` for installed versions, plus `npm outdated -g --json` for latest. Merge by package key. Note that `npm outdated` exits with a nonzero code when results exist, so capture stdout from the error path rather than treating it as a failure.
- brew: `brew list --versions` for installed, plus `brew outdated --json=v2` for latest.
- pip: `pip list --format=json` for installed, plus `pip list --outdated --format=json` for latest.
- Status is derived: installed equals latest is current, they differ is update, no installed version is not installed.
- Pins: persist a set of pinned packages. A pinned tool is excluded from Update All but still displayed as outdated so the user does not lose track of it.

`reference/scanner.js` already implements this detection across all three ecosystems as a working CLI. Lift its batch logic for the agent.

### Transfers (execution, history, rollback)

This pane is where versions actually change and where changes are remembered.

- Execution: run the real install command and stream stdout and stderr into the active row. Replace the prototype's fake progress bar with real output and the exit code. Show success or failure honestly.
- History: log every install, update, and rollback to a persistent store with a timestamp and a from/to. This is what makes "claude-code started misbehaving, what changed and when" answerable.
- Rollback: reinstall a prior version.
  - npm: `npm i -g <pkg>@<oldVersion>` works for any published version.
  - pip: `pip install <pkg>==<oldVersion>` works.
  - brew: Homebrew does not keep old bottles and cannot reliably downgrade. Detect `eco === "brew"` and disable the Roll back control unless a cached bottle or a pin exists. Do not offer an action that will fail. Surface the limitation in the UI.

### What's New (the decision feed)

One card per available update, telling the user whether to take it. Pull changelog text from the source's GitHub releases where available.

- The recommendation:
  - `security`: an advisory exists. Source from `npm audit`, GitHub Security Advisories, or the OSV database. Always recommend taking it.
  - `safe`: release age is past a threshold (for example more than 7 days), no advisories, issue rate flat.
  - `hold`: very fresh and showing elevated issue velocity. This is the hard one. It needs issue-open-rate data from GitHub, which is rate-limited and requires judgment.
- Scope guidance: for v1, compute `safe` and `security` reliably from age plus advisories. Do not fake a confident `hold`. For a fresh release with no data yet, label it "new, little signal yet" rather than asserting a verdict. Treat real issue-velocity scoring as v1.5.
- Cache release and advisory lookups aggressively and use a GitHub token to stay within rate limits.

### Search the swarm (registry discovery)

The search bar searches the registries, not the local library. Results are sorted by popularity, which is the trust signal (the heavily-downloaded package is the safe grab). Installing from a result adds the package to the library and runs through the same install path as Transfers.

The three registries are not equal here, and the UI should be honest about it:

- npm: real and rich. Use `https://registry.npmjs.org/-/v1/search?text=<query>` for results and `https://api.npmjs.org/downloads/point/last-week/<pkg>` for the weekly download count.
- brew: no per-query search service. Fetch the full formula catalog once from `https://formulae.brew.sh/api/formula.json`, cache it, and search it in process. Install analytics are available from the same site for the downloads column.
- pip: PyPI removed its search API and has no first-party replacement. Either integrate a third-party index such as libraries.io (needs an API key) or restrict pip to exact-name resolution via `https://pypi.org/pypi/<name>/json`. Do not present fuzzy pip search as if it works. Label the pip source so the difference is visible.

Open product decision, left for you and the owner: search federated across all three sources at once (as the prototype does, better for discovery) versus a source selector that scopes one registry (faster, and hides the pip gap unless chosen).

## Persistence

Pins, history, and the registry caches need a local store. SQLite is recommended; flat JSON files under the platform app-data directory are an acceptable simpler start.

## Aesthetic and UX constraints

Keep all of the following. They are deliberate identity, not decoration to clean up:

- The Windows-98 beveled chrome and the VT323 logo wordmark.
- The dial-up connect splash on launch.
- The throttle slider that intentionally does nothing.
- The era-flavored "shared by" peer handles.
- Downloads-per-week shown as the trust signal, with the flame marker on heavily-shared packages.

Every pane must stay genuinely useful underneath the joke. The bit is the skin; the substance is a real, fast package manager.

## Suggested build order

1. Agent plus Shared Library scan. Get npm working end to end first, then add brew, then pip.
2. Transfers: real install with streamed output, plus the history store.
3. Rollback for npm and pip, with brew gated.
4. Search: npm first, then the brew cached index, then pip exact-lookup.
5. What's New: changelog plus advisories for safe and security. Defer issue-velocity hold.
6. Package as a desktop app.

## What not to do

- Do not redesign the interface or swap the aesthetic for something generic.
- Do not use Napster trademarks or trade dress.
- Do not silently fake what is not technically possible. Pip fuzzy search and arbitrary brew downgrades both have real limits. Surface them in the UI rather than papering over them.
- Do not put shell logic in the frontend. It lives in the agent.

## Note on writing style

In any documentation or UI copy you generate for this project, do not use em dashes.
