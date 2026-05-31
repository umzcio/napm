# napm M5 - What's New (the decision feed + security intelligence)

**Date:** 2026-05-31
**Status:** Approved, ready for implementation plan
**Milestone:** M5 (see `docs/ROADMAP.md`)

## Goal

Populate the What's New tab with real release and security intelligence per tool,
so it answers "should I take this update, and why" and, just as importantly,
"am I holding something I should remove right now." Replace the empty `FEED` with
three layers of real signal. Preserve the existing card UI exactly.

Guiding principle (project-wide): keep the late-90s file-sharing look and feel,
but every element must carry real data or function. Never imply safety that was
not actually verified.

## The three layers (priority order)

M5 produces three kinds of intelligence. Visual priority is
**malicious -> vulnerable -> safe -> new**, with the wire above everything.

### Layer 1 - Protect what you have (the core)

A batched OSV query over EVERY installed tool at its CURRENT version (not just
outdated ones - a compromise often hits a version you already hold and have no
reason to update). OSV returns two severities:

- **malicious** - the package/version appears in the OpenSSF malicious-packages
  data or GitHub malware advisories (a hijack/compromise). Renders as a top red
  SECURITY alert: update to the fixed version, or if none exists, remove it now.
- **vulnerable** - a CVE/GHSA affects the installed version. Renders as a
  security recommendation, always shown.

Runs independent of the appetite dial and independent of outdated status. One
batched call covers the whole library.

### Layer 2 - The supply-chain wire (awareness)

A compact bulletin strip at the top of What's New, fed by GitHub's recent npm +
pip malware advisories (newest first, cached). Surfaces big ecosystem events even
for packages the user does not have. On-theme: a server-bulletin ticker.

### Layer 3 - Update verdicts (the safe-to-take set)

For each update in the appetite-scoped candidate set, a recommendation from
release age:

- **safe** - settled (older than 7 days), no advisory.
- **new** - fresh (under 7 days), no advisory. Labeled "new, little signal yet,"
  never a faked confident verdict.

The genuinely hard **hold** verdict (real issue-velocity scoring) stays deferred
to v1.5, per CLAUDE.md.

Security (Layers 1 and 2) is never hidden by the appetite dial; the dial only
scopes which non-security updates appear as safe/new.

## Data sources (all keyless by default)

- **Security / compromise:** OSV.dev. `POST https://api.osv.dev/v1/querybatch`
  with one query per installed tool (`{package:{ecosystem, name}, version}`).
  OSV aggregates GitHub Security Advisories (including the `malware` type), the
  OpenSSF malicious-packages dataset (`MAL-` IDs), and CVEs. Ecosystems: npm ->
  `npm`, pip -> `PyPI`.
- **Supply-chain wire:** GitHub global advisory database.
  `GET https://api.github.com/advisories?type=malware&ecosystem=npm&sort=published`
  (and `ecosystem=pip`). Public, keyless, rate-limited.
- **Release age:** npm `https://registry.npmjs.org/<pkg>` (`time[version]`); pip
  `https://pypi.org/pypi/<pkg>/json` (`releases[version]` upload time).
- **Changelog:** GitHub releases of the upstream repo
  (`https://api.github.com/repos/<owner>/<repo>/releases`), mapped from the
  `repository`/`homepage` metadata already mined during the scan.

## Per-ecosystem coverage (honest about gaps)

**npm and pip - full coverage:** OSV ecosystem mapping exists, so the Layer 1
scan covers them; real per-version publish timestamps drive the age verdict;
changelogs from the upstream GitHub repo.

**brew - honestly limited, labeled:**

- OSV has no Homebrew ecosystem, so brew formulae cannot be reliably scanned.
  brew is excluded from Layer 1 and the card says so, rather than implying it was
  checked and is clean.
- formulae.brew.sh exposes no clean per-version release date, so the safe/new
  verdict cannot be computed. brew updates are labeled "limited signal."
- Changelog DOES work where the formula maps to an upstream GitHub repo, so brew
  cards can still show "what changed."
- Mapping system tools (openssl, curl) to CVE feeds (Debian/Alpine) is a real but
  messy v1.5 stretch, deferred.

**GitHub token - optional, zero-friction default:** changelog and wire calls hit
the GitHub API (60 req/hr unauthenticated). v1 default is keyless with aggressive
caching (the wire is one cached call; changelogs are lazy and cached permanently
per version), which is plenty for normal use. If a `GITHUB_TOKEN` is present
(env var, or a Preferences field in M6), use it to raise the limit to 5000/hr. No
setup required to get value.

## Architecture

Reuses the M4.1 performance patterns (shared keep-alive agent, concurrent
fan-out, tiered caching).

**Shared network layer:** lift `search/http.rs` up to a shared
`src-tauri/src/http.rs` (the keep-alive agent + `get` + `encode`), used by both
`search` and the new `intel` module. One connection pool for the app.

**New module `src-tauri/src/intel/`:**

- `osv.rs` - `parse_osv_batch(json)` (pure, tested) + `scan_security(installed)`
  (one batched querybatch over the installed set).
- `wire.rs` - `parse_advisories(json)` (pure, tested) + `fetch_wire()` (GitHub
  global advisories, cached).
- `release.rs` - `parse_npm_time`, `parse_pypi_time`, `age_verdict`,
  `parse_github_releases` (all pure, tested) + thin fetch wrappers.
- `mod.rs` - the `SecurityAlert`, `WireItem`, `ReleaseInfo` types and the
  orchestrators.

**Two commands (registered in the existing `generate_handler!`):**

- `get_whats_new(installed, verdict_scope)` - the frontend passes its full
  installed list plus the appetite-scoped candidate subset. Backend runs the
  three layers CONCURRENTLY (`thread::scope`): OSV batch over all installed, age
  verdicts only for the small `verdict_scope` set (parallelized), and the cached
  wire. Returns `{ alerts, wire, verdicts }`.
- `get_changelog(eco, pkg, version)` - lazy, called only when a card is expanded.

**Why this is cheap despite ~104 outdated tools:** security is ONE batched OSV
call for the whole library, not one-per-tool. Age verdicts run only for the ~6
in-scope updates. The wire is one cached call. Opening What's New is roughly 8
concurrent requests; changelogs are deferred until a card is expanded.

**Caching (app-data dir, same pattern as the brew cache):**

- OSV security - live each time What's New opens (one batched call, always fresh
  for current compromise data), with a short in-session memory cache.
- Wire - `wire.json`, about 1h TTL.
- Changelog and release age - cached permanently per `(eco, pkg, version)`, since
  published release notes and timestamps never change.

## Data flow (opening What's New)

1. JS gathers the installed list and computes the appetite-scoped candidate set,
   calls `get_whats_new(installed, verdict_scope)`.
2. Backend runs the three layers concurrently, returns `{ alerts, wire, verdicts }`.
3. JS renders top to bottom: the wire strip, then alert cards (malicious then
   vulnerable), then verdict cards (safe then new).
4. Expanding a card lazily calls `get_changelog(eco, pkg, version)` to fill its
   "what changed" list.
5. The card action routes through the existing M3 Transfers path: update to the
   latest, or for a malicious card to the FIXED version OSV reports. If the only
   remedy is removal and no fixed version exists, the card shows a blunt copyable
   "remove this" instruction rather than a fake one-click (napm has no uninstall
   op yet - honest gap).

## Card mapping

The backend `ReleaseInfo`/`SecurityAlert` map onto the current `FEED` item shape
(`ti, rec, age, blurb, changelog, signals`). The `REC` table grows from
safe/hold/security to **malicious / security / safe / new**, each with its own
glyph and color. Alert cards can reference a tool's CURRENT installed version (a
compromise does not require the tool to be outdated), so the feed can include a
card for a tool that is otherwise up to date.

## Error handling - never imply "safe" when a check failed

- OSV unreachable -> the security layer returns an explicit "security check
  unavailable" state, visually distinct from "no advisories found." This
  distinction is the whole point of a security feature.
- Wire unreachable -> the strip shows "wire unavailable," not a blank that reads
  as "all quiet."
- An age lookup failing -> that update is labeled "signal unknown," not dropped.
- Changelog rate-limited/unavailable -> the card says so honestly.

## Testing

- Pure parsers against captured real JSON fixtures: `parse_osv_batch` (a
  malicious `MAL-` hit, a `GHSA` vuln, and a clean package in one response),
  `parse_advisories` (the wire), `parse_npm_time`/`parse_pypi_time` (age from
  real registry shapes), `age_verdict` (the 7-day safe-vs-new boundary),
  `parse_github_releases` (changelog bullets).
- Verdict logic: severity sort order, and explicitly the "couldn't check" vs
  "clean" distinction.
- Network wrappers stay thin and are verified live, as with scan and search.

## Out of scope (deferred)

- Real issue-velocity `hold` verdict (needs GitHub issue-rate data + judgment) -
  v1.5.
- brew / system-tool CVE mapping via Debian/Alpine feeds - v1.5.
- One-click uninstall op for the "remove this" remedy.
- The appetite dial's "security-only" far-left notch (a small follow-on now that
  security always surfaces).
