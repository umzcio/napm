# napm M4 - Search the swarm (registry discovery)

**Date:** 2026-05-30
**Status:** Approved, ready for implementation plan
**Milestone:** M4 (see `docs/ROADMAP.md`)

## Goal

Replace the Search tab's seeded fake `SWARM` array with real, live registry
queries across npm, brew, and pip, so the user can discover and install tools
they do not yet have. Preserve the prototype's look, grid, and install flow
exactly. Stay honest about each registry's real limits.

Guiding principle (project-wide): keep the late-90s file-sharing look and feel,
but every element must carry real data or function. No decorative jokes, no
faked capability.

## Product decisions (settled)

- **Federated search.** One query searches the whole swarm at once (npm + brew +
  pip), results merged into one flat list. This is the Napster-faithful model:
  Napster searched the entire network, never a pre-selected sub-network. A
  "source selector" that forces you to pick a registry first was rejected as the
  least Napster-like option.
- **Source chips as optional filters.** Chips `( all ) ( npm ) ( brew ) ( pip )`
  sit above the results, default to `all`, and narrow the view. They map to
  Napster's optional filter dropdowns (line speed, bitrate, ping). They are a
  pure client-side filter over the already-fetched federated result set:
  clicking a chip never re-queries the network.
- **"Find It!" button.** The search button reads "Find It!" (the real Napster
  wording), replacing "Search". It is still a real search trigger.
- **Downloads per week is the sort key.** Results sort by weekly downloads
  descending, the heavily-shared package floats to the top. This is the trust
  signal, analogous to Napster's line speed / ping quality columns. Fire marker
  stays on heavily-shared packages.
- **pip is exact-name only, labeled.** PyPI has no search API, so the query is
  treated as an exact package name. pip results carry a small "exact match" tag
  so the gap is visible, never papered over.
- **No API keys.** Every endpoint used in M4 is keyless.

## UX (what changes on screen)

The Search tab keeps its exact current layout and result grid. Only the data and
controls underneath change.

- The seeded `SWARM` array is deleted. **Find It!** calls a new backend command
  that queries the live registries and returns real results.
- Source chips appear above the results, default `all`, opt-in narrowing, pure
  client-side view filter.
- Results stay a flat grid sorted by downloads/wk descending, fire marker on
  heavily-shared packages, same columns as today (glyph, Package, Version,
  Downloads/wk, Source, Size, action).
- Install / Update / "in library" actions are unchanged. They already hand off
  to the real M3 Transfers path. The wiring reads from the live result and
  dedupes against the real installed library.
- pip results only appear on an exact PyPI name match, tagged "exact match".
- While the first brew search builds its catalog index, the grid shows a
  one-time "building swarm index..." state, not a fake spinner.

Nothing about the chrome, the grid, or the install flow is redesigned.

## Backend architecture

Mirrors the existing `scan/` module structure so it reads like the surrounding
code.

New module `src-tauri/src/search/`:

- `mod.rs` - defines the `SearchResult` struct
  (`name, eco, pkg, version, weekly_downloads, size, description`), the
  `search_all(query)` orchestrator that fans out to the three sources, merges,
  dedupes, and sorts by `weekly_downloads` descending.
- `npm.rs` - npm registry search + download counts. Pure
  `parse_npm_search(json)` (unit-tested), thin network wrapper.
- `brew.rs` - cached catalog search + analytics join. Pure
  `search_catalog(catalog, query)` and `parse_analytics(json)` (unit-tested),
  thin fetch/cache wrapper.
- `pip.rs` - exact-name PyPI lookup. Pure `parse_pypi(json)` (unit-tested), thin
  wrapper.
- `http.rs` - one small shared GET helper (timeout, user-agent, returns
  `Result<String>`); the single place anything touches the network.

**HTTP client:** add `ureq` (blocking, rustls TLS, no async runtime). Matches the
existing all-blocking `std::process` style. The `search_registry` command runs
synchronously on Tauri's command thread, same as `scan_installed`.

**Caching** (in the existing app-data dir, alongside `pins.json` /
`history.json`):

- `brew_catalog.json` - the full formula catalog (several MB). Fetched once,
  refreshed only when older than 24h. The one genuinely expensive fetch.
- `brew_analytics.json` - the 30-day install-count ranking, same 24h TTL, joined
  to the catalog for brew's downloads column.
- npm search, npm download counts, and pip lookups are live per query. No
  query-result caching in v1 (YAGNI).

**New command:** `search_registry(query: String) -> Vec<SearchResult>`,
registered in the existing `generate_handler!`. Frontend calls it via `invoke`,
exactly like `scan_installed`.

## Per-source specifics

**npm (real and rich):**

- Search: `https://registry.npmjs.org/-/v1/search?text=<query>&size=25`. Returns
  name, version, description per hit.
- Downloads/wk: `https://api.npmjs.org/downloads/point/last-week/<pkg>`. Bulk
  endpoint (`pkg1,pkg2,...`) for unscoped names in one call; scoped names
  (`@scope/pkg`) cannot be bulked and get individual calls. Bounded by the 25
  shown results.
- Size: npm search does not return install size. Column shows `-` for npm until
  install. Honest blank, never a fabricated number.

**brew (cached catalog, real analytics):**

- Search: the cached `formula.json` catalog filtered in-process by name and
  description substring. No per-query network call after the catalog is cached.
- Downloads/wk: joined from the cached 30-day analytics ranking, divided to a
  rough weekly figure, with a tooltip noting it is a 30-day install average.
  Real data, labeled as approximate.
- Size: catalog does not carry install size. `-` until install.

**pip (exact-name only, clearly labeled):**

- No fuzzy search. Query treated as an exact package name:
  `https://pypi.org/pypi/<query>/json`. A hit returns name, version, summary; a
  miss contributes nothing.
- Downloads/wk: `https://pypistats.org/api/packages/<name>/recent` (keyless),
  last-week field.
- Every pip result carries a small "exact match" tag in the grid.

**Merge and sort:** all hits pooled, deduped by `(eco, pkg)`, sorted by weekly
downloads descending. Any result already in the live installed library is marked
"in library" / "Update" instead of "Get".

## Data flow (one search)

1. User types a query, hits Find It! -> JS `invoke("search_registry", { query })`.
2. Backend `search_all` fans out: npm search + downloads, brew catalog filter +
   analytics join, pip exact lookup. Each source independent.
3. Results merged, deduped by `(eco, pkg)`, sorted by weekly downloads ->
   `Vec<SearchResult>`.
4. JS renders the grid, marks rows against the live installed library, applies
   the current source chip as a view filter.
5. Install / Update click -> existing M3 `run_op` Transfers path. No new install
   code.

## Error handling (honest, never fatal)

- Each source is wrapped independently. If npm is down, brew and pip still
  render; a failed source contributes zero rows and never blanks the whole
  search. The status line notes when a source failed (e.g. "npm unreachable").
- Network timeout is short (a few seconds) so a dead source does not hang the
  grid.
- First brew search with no cached catalog shows the "building swarm index..."
  state; if that fetch fails, brew is absent that session and says so.
- A pip miss is a normal empty result, not an error.
- Offline entirely -> empty grid with an honest "the swarm is unreachable -
  check your connection" message, not a fake spinner.

## Testing

- Pure parse functions unit-tested against captured real JSON fixtures:
  `parse_npm_search`, `parse_pypi`, `search_catalog` (brew substring match +
  ranking), `parse_analytics`. Same pattern as the `scan/` tests.
- Merge / sort / dedupe tested on synthetic `SearchResult` vectors (sort order,
  `(eco, pkg)` dedupe, library-match marking).
- Network wrappers stay thin and are verified manually against the live app, the
  way the scan commands were.

## Out of scope (deferred)

- Query-result caching for npm/pip (live is fast enough for v1).
- npx "latest" freshness via the new network layer (follow-on; M4 only brings
  the layer).
- libraries.io / third-party pip fuzzy search (keeps M4 keyless and honest).
- What's New decision feed (M5).
