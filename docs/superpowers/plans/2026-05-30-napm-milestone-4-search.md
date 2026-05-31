# napm M4 - Search the swarm Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Search tab's seeded `SWARM` array with live federated registry search (npm + brew + pip), behind the prototype's existing grid and install flow.

**Architecture:** A new `src-tauri/src/search/` module mirrors the existing `scan/` module: pure parse functions (unit-tested against captured JSON) plus thin network wrappers behind a single `http.rs` choke point. Blocking HTTP via `ureq`. One new Tauri command, `search_registry(query) -> Vec<SearchResult>`. The frontend deletes `SWARM`, renames the button to "Find It!", adds source-filter chips (client-side view filter), and renders real results.

**Tech Stack:** Rust, Tauri v2, `ureq` (blocking, rustls), `serde`/`serde_json`. Vanilla-JS frontend via `window.__TAURI__.core.invoke`.

**Spec:** `docs/superpowers/specs/2026-05-30-napm-milestone-4-search.md`

## Conventions for every task

- Run `source "$HOME/.cargo/env"` before any cargo command.
- Tests: `cd src-tauri && cargo test --lib`. Pure functions are unit-tested; network wrappers are thin and verified live (not unit-tested).
- Follow the existing `scan/` module style exactly (see `scan/pip.rs`, `scan/npm.rs`): pure `parse_*`/`search_*` functions with `#[cfg(test)]` tests, thin shell/network wrapper on top.
- No em dashes in any code comment or UI string. Never the word "Napster". Brand is npstr.
- Commit after each task with a `feat:`/`chore:` message. Dead-code warnings are acceptable until Task 5 wires the sources; do not silence them with broad `#[allow]` unless a single function is genuinely unused at that step.
- After the frontend task, mirror the file: `cp frontend/index.html prototype/napm-prototype.html`.

## File structure

- Create `src-tauri/src/search/mod.rs` - `SearchResult` struct, `merge()`, `search_all()`.
- Create `src-tauri/src/search/http.rs` - shared GET helper + percent-encode.
- Create `src-tauri/src/search/npm.rs` - `parse_npm_search()`, `search_npm()`.
- Create `src-tauri/src/search/brew.rs` - `search_catalog()`, `parse_analytics()`, `search_brew()`.
- Create `src-tauri/src/search/pip.rs` - `parse_pypi()`, `search_pip()`.
- Modify `src-tauri/Cargo.toml` - add `ureq`.
- Modify `src-tauri/src/lib.rs` - `mod search;`, `search_registry` command, register it.
- Modify `frontend/index.html` - rewire the Search tab. Mirror to `prototype/`.

---

### Task 1: Scaffold - dependency, HTTP helper, SearchResult, merge

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/search/mod.rs`
- Create: `src-tauri/src/search/http.rs`
- Modify: `src-tauri/src/lib.rs:1-3` (add `mod search;`)

- [ ] **Step 1: Add the dependency.** In `Cargo.toml` `[dependencies]` add:

```toml
ureq = "2"
```

(ureq 2.x defaults to rustls TLS, no OpenSSL. Run `cargo build` once to confirm it resolves.)

- [ ] **Step 2: Write `search/mod.rs` with the struct and a failing merge test.**

```rust
use serde::Serialize;
use std::path::Path;

pub mod http;

/// One discovered package in the swarm. Canonical SearchResult shape.
/// Serialized camelCase so the frontend reads `weeklyDownloads`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub name: String,
    pub eco: String,
    pub pkg: String,
    pub version: String,
    pub weekly_downloads: u64,
    pub size: String,        // "" when the registry does not expose install size
    pub description: String,
}

/// Pool results from all sources, dedupe by (eco, pkg) keeping the first seen,
/// and sort by weekly_downloads descending (the trust signal / sort key).
pub fn merge(sources: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
    use std::collections::BTreeSet;
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out: Vec<SearchResult> = Vec::new();
    for source in sources {
        for r in source {
            if seen.insert((r.eco.clone(), r.pkg.clone())) {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| b.weekly_downloads.cmp(&a.weekly_downloads));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(eco: &str, pkg: &str, dl: u64) -> SearchResult {
        SearchResult { name: pkg.into(), eco: eco.into(), pkg: pkg.into(),
            version: "1.0.0".into(), weekly_downloads: dl, size: String::new(),
            description: String::new() }
    }

    #[test]
    fn merge_sorts_by_downloads_desc_and_dedupes() {
        let merged = merge(vec![
            vec![r("npm", "a", 100), r("brew", "b", 500)],
            vec![r("npm", "a", 999), r("pip", "c", 300)], // dup (npm,a) dropped
        ]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].pkg, "b");   // 500
        assert_eq!(merged[1].pkg, "c");   // 300
        assert_eq!(merged[2].pkg, "a");   // 100 (first-seen kept)
    }
}
```

- [ ] **Step 3: Write `search/http.rs`.**

```rust
use std::time::Duration;

/// The single place anything in the app touches the network for search.
/// Short timeouts so a dead source never hangs the grid. Returns the body
/// string on 2xx, or an Err message on any failure (caller degrades to no rows).
pub(crate) fn get(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .user_agent("napm")
        .build();
    match agent.get(url).call() {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Percent-encode a query value (encode everything except RFC 3986 unreserved).
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encode_escapes_spaces_and_slashes() {
        assert_eq!(encode("fuzzy finder"), "fuzzy%20finder");
        assert_eq!(encode("@scope/pkg"), "%40scope%2Fpkg");
        assert_eq!(encode("ripgrep"), "ripgrep");
    }
}
```

- [ ] **Step 4: Wire the module.** In `src-tauri/src/lib.rs`, add `mod search;` beneath the existing `mod ops;` (line 3).

- [ ] **Step 5: Run tests.** `cd src-tauri && cargo test --lib`
Expected: PASS (merge + encode tests green). `cargo build` succeeds with ureq resolved. A `search_all` does not exist yet; that is fine.

- [ ] **Step 6: Commit.**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/search/ src-tauri/src/lib.rs
git commit -m "feat: search module scaffold - ureq, http helper, SearchResult, merge"
```

---

### Task 2: npm source

**Files:**
- Create: `src-tauri/src/search/npm.rs`
- Modify: `src-tauri/src/search/mod.rs` (add `pub mod npm;`)

**Endpoints:**
- Search: `https://registry.npmjs.org/-/v1/search?text=<encoded>&size=25`
  Response: `{ "objects": [ { "package": { "name", "version", "description" } } ] }`
- Downloads (bulk, unscoped only): `https://api.npmjs.org/downloads/point/last-week/<comma-list>`
  Bulk response: `{ "<pkg>": { "downloads": N, "package": "<pkg>" } }`
- Downloads (single / scoped `@scope/pkg`): `https://api.npmjs.org/downloads/point/last-week/<pkg>`
  Single response: `{ "downloads": N, "package": "<pkg>" }`
  (Scoped names cannot be combined in the bulk call - query each individually.)

- [ ] **Step 1: Write `search/npm.rs` with a failing `parse_npm_search` test.** Pure function takes the search JSON, returns `Vec<SearchResult>` with `weekly_downloads: 0`, `size: ""` (downloads filled by the wrapper later).

```rust
use super::SearchResult;
use serde_json::Value;

/// Parse the npm registry search response into results (downloads filled in
/// later by the wrapper; size is not exposed by npm search so it stays "").
pub fn parse_npm_search(json: &str) -> Vec<SearchResult> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let objects = match v.get("objects").and_then(|o| o.as_array()) {
        Some(a) => a, None => return Vec::new(),
    };
    let mut out = Vec::new();
    for o in objects {
        let p = match o.get("package") { Some(p) => p, None => continue };
        let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if name.is_empty() { continue; }
        let version = p.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let description = p.get("description").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        out.push(SearchResult {
            name: name.to_string(), eco: "npm".into(), pkg: name.to_string(),
            version, weekly_downloads: 0, size: String::new(), description,
        });
    }
    out
}
```

Test (fixture inline, real shape):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_name_version_description() {
        let json = r#"{"objects":[
            {"package":{"name":"eslint","version":"9.10.0","description":"Pluggable JS linter"}},
            {"package":{"name":"@scope/x","version":"1.2.3","description":""}}
        ]}"#;
        let r = parse_npm_search(json);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].pkg, "eslint");
        assert_eq!(r[0].eco, "npm");
        assert_eq!(r[0].version, "9.10.0");
        assert_eq!(r[1].pkg, "@scope/x");
    }
    #[test]
    fn garbage_yields_no_rows() {
        assert!(parse_npm_search("nope").is_empty());
        assert!(parse_npm_search(r#"{"objects":[]}"#).is_empty());
    }
}
```

- [ ] **Step 2: Run the test, confirm it passes.** Add `pub mod npm;` to `search/mod.rs`. `cargo test --lib`.

- [ ] **Step 3: Add a failing test for the downloads parser.** A pure helper `parse_downloads(json) -> BTreeMap<String, u64>` that handles BOTH the bulk shape and the single shape:

```rust
use std::collections::BTreeMap;

/// Parse a npm downloads-point response. Handles the bulk shape
/// (`{"pkg":{"downloads":N,...}}`) and the single shape
/// (`{"downloads":N,"package":"pkg"}`). Returns pkg -> weekly downloads.
pub fn parse_downloads(json: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return map };
    // Single shape: has a top-level "package" string + "downloads" number.
    if let (Some(pkg), Some(dl)) = (
        v.get("package").and_then(|x| x.as_str()),
        v.get("downloads").and_then(|x| x.as_u64()),
    ) {
        map.insert(pkg.to_string(), dl);
        return map;
    }
    // Bulk shape: object of pkg -> {downloads, package}. Null entries = unknown pkg.
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let Some(dl) = val.get("downloads").and_then(|x| x.as_u64()) {
                map.insert(k.clone(), dl);
            }
        }
    }
    map
}
```

Test:

```rust
#[test]
fn parses_bulk_and_single_downloads() {
    let bulk = r#"{"eslint":{"downloads":32000000,"package":"eslint"},
                   "prettier":{"downloads":28000000,"package":"prettier"},
                   "bogus":null}"#;
    let m = parse_downloads(bulk);
    assert_eq!(m.get("eslint"), Some(&32000000));
    assert_eq!(m.get("prettier"), Some(&28000000));
    assert_eq!(m.get("bogus"), None);

    let single = r#"{"downloads":1400000,"package":"@anthropic-ai/claude-code"}"#;
    let m2 = parse_downloads(single);
    assert_eq!(m2.get("@anthropic-ai/claude-code"), Some(&1400000));
}
```

- [ ] **Step 4: Run the test, confirm it passes.** `cargo test --lib`.

- [ ] **Step 5: Write the thin network wrapper `search_npm`.** Not unit-tested (network). Logic:
  1. `let body = http::get(&format!("https://registry.npmjs.org/-/v1/search?text={}&size=25", http::encode(query)))?;` (on Err, return `Vec::new()`).
  2. `let mut rows = parse_npm_search(&body);`
  3. Split row pkg names into unscoped (no leading `@`) and scoped. Bulk-fetch unscoped: `https://api.npmjs.org/downloads/point/last-week/<comma-joined>` -> `parse_downloads`. Fetch each scoped name individually and merge into the same map. Any fetch failure just leaves that pkg at 0.
  4. Set each row's `weekly_downloads` from the map (default 0).
  5. Return rows.

  Signature: `pub fn search_npm(query: &str) -> Vec<SearchResult>`.

- [ ] **Step 6: Run `cargo test --lib`** (parsers green; wrapper compiles). Commit:

```bash
git add src-tauri/src/search/
git commit -m "feat: npm registry search with weekly download counts"
```

---

### Task 3: brew source (cached catalog + analytics)

**Files:**
- Create: `src-tauri/src/search/brew.rs`
- Modify: `src-tauri/src/search/mod.rs` (add `pub mod brew;`)

**Endpoints:**
- Catalog: `https://formulae.brew.sh/api/formula.json` -> array of
  `{ "name", "desc", "versions": { "stable": "x.y.z" } }`. Several MB.
- Analytics: `https://formulae.brew.sh/api/analytics/install/30d.json` ->
  `{ "formulae": { "wget": [ { "formula": "wget", "count": "1,234,567" } ], ... } }`.
  Note: `count` is a STRING with thousands separators ("1,234,567").

- [ ] **Step 1: Failing test for `parse_analytics`.** Pure: JSON -> `BTreeMap<String, u64>` (formula name -> 30-day install count, commas stripped).

```rust
use std::collections::BTreeMap;
use serde_json::Value;

/// formula name -> 30-day install count. The API gives `count` as a
/// comma-grouped string ("1,234,567"), so strip commas before parsing.
pub fn parse_analytics(json: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return map };
    let formulae = match v.get("formulae").and_then(|f| f.as_object()) {
        Some(o) => o, None => return map,
    };
    for (name, arr) in formulae {
        let count = arr.as_array()
            .and_then(|a| a.first())
            .and_then(|e| e.get("count"))
            .and_then(|c| c.as_str())
            .map(|s| s.replace(',', ""))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        map.insert(name.clone(), count);
    }
    map
}
```

Test:

```rust
#[test]
fn parses_comma_grouped_counts() {
    let json = r#"{"formulae":{
        "wget":[{"formula":"wget","count":"1,234,567"}],
        "jq":[{"formula":"jq","count":"890"}]
    }}"#;
    let m = parse_analytics(json);
    assert_eq!(m.get("wget"), Some(&1234567));
    assert_eq!(m.get("jq"), Some(&890));
}
```

- [ ] **Step 2: Run, confirm pass.** Add `pub mod brew;` to `mod.rs`.

- [ ] **Step 3: Failing test for `search_catalog`.** Pure: `(catalog_json, query, &analytics_map) -> Vec<SearchResult>`. Substring match (case-insensitive) on name OR desc. `weekly_downloads` = analytics count / 4 (30-day to rough weekly). `version` from `versions.stable`. `size: ""`.

```rust
use super::SearchResult;

/// Search the cached brew catalog in-process. Case-insensitive substring on
/// name or description. weekly_downloads is the 30-day analytics count divided
/// to a rough weekly figure (labeled approximate in the UI).
pub fn search_catalog(catalog_json: &str, query: &str, analytics: &BTreeMap<String, u64>) -> Vec<SearchResult> {
    let v: Value = match serde_json::from_str(catalog_json) { Ok(v) => v, Err(_) => return Vec::new() };
    let arr = match v.as_array() { Some(a) => a, None => return Vec::new() };
    let q = query.to_lowercase();
    let mut out = Vec::new();
    for f in arr {
        let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if name.is_empty() { continue; }
        let desc = f.get("desc").and_then(|x| x.as_str()).unwrap_or("");
        if !name.to_lowercase().contains(&q) && !desc.to_lowercase().contains(&q) { continue; }
        let version = f.get("versions").and_then(|x| x.get("stable"))
            .and_then(|x| x.as_str()).unwrap_or("").to_string();
        let weekly = analytics.get(name).copied().unwrap_or(0) / 4;
        out.push(SearchResult {
            name: name.to_string(), eco: "brew".into(), pkg: name.to_string(),
            version, weekly_downloads: weekly, size: String::new(),
            description: desc.trim().to_string(),
        });
    }
    out
}
```

Test:

```rust
#[test]
fn substring_match_on_name_or_desc_with_weekly_from_analytics() {
    let catalog = r#"[
        {"name":"ripgrep","desc":"Recursive search faster than grep","versions":{"stable":"14.1.1"}},
        {"name":"jq","desc":"JSON processor","versions":{"stable":"1.7.1"}}
    ]"#;
    let mut a = BTreeMap::new();
    a.insert("ripgrep".to_string(), 4_000_000u64);
    let hits = search_catalog(catalog, "search", &a);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].pkg, "ripgrep");
    assert_eq!(hits[0].eco, "brew");
    assert_eq!(hits[0].version, "14.1.1");
    assert_eq!(hits[0].weekly_downloads, 1_000_000); // 4M / 4
    // matches description too:
    assert_eq!(search_catalog(catalog, "json", &a).len(), 1);
}
```

- [ ] **Step 4: Run, confirm pass.**

- [ ] **Step 5: Write the cache + wrapper `search_brew`.** Signature: `pub fn search_brew(query: &str, cache_dir: &Path) -> Vec<SearchResult>`. Logic:
  1. Ensure two cache files in `cache_dir`: `brew_catalog.json`, `brew_analytics.json`. A file is fresh if it exists and its mtime is under 24h old (reuse `std::fs::metadata().modified()`; compute age against `SystemTime::now()`).
  2. If stale/missing, fetch via `http::get` and write to the cache file. If the fetch fails AND no cached copy exists, return `Vec::new()` (brew simply absent this session).
  3. Read both cache files, `parse_analytics`, then `search_catalog`.

  Add a private helper `fn cached_or_fetch(path: &Path, url: &str) -> Option<String>` (returns cached string, refreshing when stale). Keep it small.

- [ ] **Step 6: `cargo test --lib`** (parsers green), commit:

```bash
git add src-tauri/src/search/
git commit -m "feat: brew catalog search with cached formula index and analytics"
```

---

### Task 4: pip source (exact-name lookup)

**Files:**
- Create: `src-tauri/src/search/pip.rs`
- Modify: `src-tauri/src/search/mod.rs` (add `pub mod pip;`)

**Endpoints:**
- Lookup: `https://pypi.org/pypi/<name>/json` -> `{ "info": { "name", "version", "summary" } }` (404 on miss).
- Downloads: `https://pypistats.org/api/packages/<name>/recent` -> `{ "data": { "last_week": N } }`.

- [ ] **Step 1: Failing test for `parse_pypi`.** Pure: `(json) -> Option<SearchResult>`. `weekly_downloads: 0` (filled by wrapper), `size: ""`.

```rust
use super::SearchResult;
use serde_json::Value;

/// Parse a PyPI project JSON into a single result. None on parse failure or
/// missing info (a 404 body will not parse to a usable info block).
pub fn parse_pypi(json: &str) -> Option<SearchResult> {
    let v: Value = serde_json::from_str(json).ok()?;
    let info = v.get("info")?;
    let name = info.get("name").and_then(|x| x.as_str())?;
    if name.is_empty() { return None; }
    let version = info.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let description = info.get("summary").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    Some(SearchResult {
        name: name.to_string(), eco: "pip".into(), pkg: name.to_string(),
        version, weekly_downloads: 0, size: String::new(), description,
    })
}

/// Last-week downloads from a pypistats `recent` response, or 0.
pub fn parse_pip_downloads(json: &str) -> u64 {
    serde_json::from_str::<Value>(json).ok()
        .and_then(|v| v.get("data").and_then(|d| d.get("last_week")).and_then(|x| x.as_u64()))
        .unwrap_or(0)
}
```

Tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_pypi_info() {
        let json = r#"{"info":{"name":"httpie","version":"3.2.2","summary":"Human-friendly HTTP client"}}"#;
        let r = parse_pypi(json).unwrap();
        assert_eq!(r.pkg, "httpie");
        assert_eq!(r.eco, "pip");
        assert_eq!(r.version, "3.2.2");
    }
    #[test]
    fn miss_or_garbage_is_none() {
        assert!(parse_pypi(r#"{"message":"Not Found"}"#).is_none());
        assert!(parse_pypi("nope").is_none());
    }
    #[test]
    fn parses_last_week_downloads() {
        assert_eq!(parse_pip_downloads(r#"{"data":{"last_day":1,"last_week":1200000,"last_month":9}}"#), 1200000);
        assert_eq!(parse_pip_downloads("nope"), 0);
    }
}
```

- [ ] **Step 2: Run, confirm pass.** Add `pub mod pip;` to `mod.rs`.

- [ ] **Step 3: Write the wrapper `search_pip`.** Signature: `pub fn search_pip(query: &str) -> Vec<SearchResult>`. Logic:
  1. `let body = http::get(&format!("https://pypi.org/pypi/{}/json", http::encode(query)))?` -> if Err, return empty.
  2. `let mut r = match parse_pypi(&body) { Some(r) => r, None => return Vec::new() };`
  3. Fetch downloads: `https://pypistats.org/api/packages/<encoded-lowercased-name>/recent`, `parse_pip_downloads`, set `r.weekly_downloads`. Failure leaves 0.
  4. Return `vec![r]`.

- [ ] **Step 4: `cargo test --lib`**, commit:

```bash
git add src-tauri/src/search/
git commit -m "feat: pip exact-name PyPI lookup with weekly downloads"
```

---

### Task 5: Orchestrate + the search_registry command

**Files:**
- Modify: `src-tauri/src/search/mod.rs` (add `search_all`)
- Modify: `src-tauri/src/lib.rs` (command + register)

- [ ] **Step 1: Add `search_all` to `mod.rs`.** Fans out to the three sources (each already degrades to empty on failure), merges. A blank query returns empty.

```rust
/// Federated swarm search: npm + brew + pip, merged and sorted. Each source
/// fails independently to an empty list, so one dead registry never blanks the
/// grid. `cache_dir` holds the brew catalog/analytics caches.
pub fn search_all(query: &str, cache_dir: &Path) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() { return Vec::new(); }
    merge(vec![
        npm::search_npm(query),
        brew::search_brew(query, cache_dir),
        pip::search_pip(query),
    ])
}
```

- [ ] **Step 2: Add the command to `lib.rs`.** Reuse the `app_data_dir` pattern from `open_store`:

```rust
#[tauri::command]
fn search_registry(app: tauri::AppHandle, query: String) -> Vec<search::SearchResult> {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    search::search_all(&query, &dir)
}
```

Add `search_registry` to the `generate_handler!` list (line 55).

- [ ] **Step 3: Build + test.** `cd src-tauri && cargo test --lib && cargo build`. Expected: all green, no unused-code warnings remain (every source fn is now reachable).

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/search/mod.rs src-tauri/src/lib.rs
git commit -m "feat: federated search_all orchestrator and search_registry command"
```

---

### Task 6: Frontend - rewire the Search tab to live results

**Files:**
- Modify: `frontend/index.html` (the SWARM/search section, lines ~252-397, 190-191; plus the chip styles/markup)
- Then: `cp frontend/index.html prototype/napm-prototype.html`

This task touches only the Search portion of the JS. Do NOT alter the library, transfers, appetite dial, or window chrome. Match the existing vanilla style (no frameworks, `var`, string-concatenated HTML, `esc()` for user text).

- [ ] **Step 1: Button text.** Change the search button label from `Search` to `Find It!` (line ~191, `<button class="btn primary" id="searchBtn">Search</button>`). The tab label stays "Search".

- [ ] **Step 2: Delete the seeded `SWARM` array** (lines ~252-277) entirely. Replace with a module-level `var SWARM=[];` that now holds the LAST live result set (so `installPackage` can still look up a clicked row by pkg). Add `var searchSource="all";` for the active chip.

- [ ] **Step 3: Add the source chips.** Just above `<div id="searchResults"></div>` (line ~215), insert:

```html
<div class="chips" id="srcChips">
  <span class="chip on" data-src="all">all</span>
  <span class="chip" data-src="npm">npm</span>
  <span class="chip" data-src="brew">brew</span>
  <span class="chip" data-src="pip">pip</span>
</div>
```

Add minimal CSS near the search-results styles (line ~101) in the beveled style already used by `.src`/`.tab`:

```css
.chips{display:flex; gap:4px; padding:4px 6px;}
.chip{font-family:var(--mono); font-size:12px; padding:1px 8px; border:1px solid var(--dgray);
  background:var(--btn); cursor:pointer;}
.chip.on{background:var(--navy); color:#fff;}
```

(Reuse existing CSS variables; check their names in `:root` and match. Do not invent new colors.)

- [ ] **Step 4: Rewire `runSearch` to call the backend.** Replace the `setTimeout` fake with a real invoke. Keep the "Searching the swarm..." interim text and the empty-state hint.

```js
function runSearch(q){
  lastQuery=q; switchTab("search");
  if(!q){ resEl.innerHTML='<div class="empty">Search the swarm for a package to download.<span class="hint">Try: <b>linter</b>, <b>http</b>, <b>fuzzy</b>, <b>claude</b>, <b>ripgrep</b></span></div>'; return; }
  resEl.innerHTML='<div class="searching">Searching the swarm for "'+esc(q)+'" ...</div>';
  invoke("search_registry",{query:q}).then(function(results){
    SWARM = results || [];
    renderSearchResults();
  }).catch(function(){
    resEl.innerHTML='<div class="empty">The swarm is unreachable. Check your connection and try again.</div>';
  });
}
```

- [ ] **Step 5: Rewire `renderSearchResults`** to render from `SWARM` (already the live set, already sorted by the backend - do not re-sort), apply the `searchSource` chip filter, and add the pip "exact match" tag. The download field is now `weeklyDownloads` (camelCase). brew/npm sizes are "" -> show as a dash. Keep the existing columns, glyph, fire marker, and library-match logic (`findTool`, `statusOf`).

```js
function renderSearchResults(){
  var q=lastQuery.toLowerCase();
  if(!q){ runSearch(""); return; }
  var hits=SWARM.filter(function(p){ return searchSource==="all" || p.eco===searchSource; });
  if(!hits.length){
    resEl.innerHTML='<div class="empty">No peers are sharing "'+esc(lastQuery)+'"'+(searchSource!=="all"?(' on '+searchSource):'')+'. Try another term.</div>';
    return;
  }
  var body=hits.map(function(p){
    var ti=findTool(p.pkg), inLib = ti>=0 && TOOLS[ti].installed;
    var outdated = ti>=0 && statusOf(TOOLS[ti])==="update";
    var act = inLib && !outdated ? '<span class="g-ok">✓ in library</span>'
            : outdated ? '<button class="btn rowbtn" data-install="'+esc(p.pkg)+'">Update</button>'
            : '<button class="btn rowbtn" data-install="'+esc(p.pkg)+'">Get</button>';
    var fire = p.weeklyDownloads>=5e6 ? ' <span class="fire" title="heavily shared">🔥</span>' : '';
    var pipTag = p.eco==="pip" ? ' <span class="exact" title="pip has no search API; matched by exact name">exact match</span>' : '';
    return '<tr>'+
      '<td class="glyph g-off">♪</td>'+
      '<td class="pkgcell"><b>'+esc(p.name)+'</b>'+fire+pipTag+' <span class="muted">'+esc(p.pkg)+'</span><span class="desc">'+esc(p.description)+'</span></td>'+
      '<td class="user">'+esc(p.version)+'</td>'+
      '<td class="dl">'+fmtDl(p.weeklyDownloads)+'</td>'+
      '<td><span class="src '+p.eco+'">'+p.eco+'</span></td>'+
      '<td class="muted">'+(p.size||"—")+'</td>'+
      '<td>'+act+'</td></tr>';
  }).join("");
  resEl.innerHTML='<table><thead><tr><th></th><th>Package</th><th>Version</th><th>Downloads/wk</th><th>Source</th><th>Size</th><th></th></tr></thead><tbody>'+body+'</tbody></table>';
}
```

Add a small CSS rule for `.exact` near `.fire` (muted, small, e.g. `font-size:10px; color:var(--dgray); border:1px solid var(--dgray); padding:0 3px;`).

- [ ] **Step 6: Wire the chips.** After the existing `resEl` click handler (line ~382), add:

```js
document.getElementById("srcChips").addEventListener("click",function(e){
  var c=e.target.closest("[data-src]"); if(!c) return;
  searchSource=c.dataset.src;
  document.querySelectorAll("#srcChips .chip").forEach(function(x){ x.classList.toggle("on", x===c); });
  renderSearchResults();
});
```

- [ ] **Step 7: Fix `installPackage`.** It currently reads `p.version` and builds a TOOLS row. Live results use the same field names except downloads. Update the new-row push to drop the fake `user:handleFor(...)` (publisher comes from a real scan after install) and use real fields:

```js
function installPackage(pkg){
  var p=null; for(var i=0;i<SWARM.length;i++) if(SWARM[i].pkg===pkg) p=SWARM[i];
  if(!p) return;
  var ti=findTool(pkg);
  if(ti>=0){
    if(statusOf(TOOLS[ti])==="update"){ queueTransfer(ti, TOOLS[ti].latest, "update"); switchTab("transfers"); }
    return;
  }
  TOOLS.push({name:p.name, eco:p.eco, pkg:p.pkg, installed:null, latest:p.version, size:p.size||"", publisher:"", description:p.description||"", updated:0, pinned:false});
  queueTransfer(TOOLS.length-1, p.version, "install");
  switchTab("transfers");
}
```

- [ ] **Step 8: Verify the invoke binding.** Confirm `invoke` is already aliased at the top of the script (same one `scanLibrary` uses, e.g. `var invoke=window.__TAURI__.core.invoke;`). If `runSearch("")` runs at startup (line ~564), it must not call the backend for an empty query - the guard in Step 4 already returns early. Good.

- [ ] **Step 9: Mirror and manually verify.**

```bash
cp frontend/index.html prototype/napm-prototype.html
```

Then `source "$HOME/.cargo/env" && npm run tauri dev`, and the USER verifies live: search "ripgrep" (brew hit), "eslint" (npm hit with real download count), "httpie" (pip exact match tag), a nonsense string (empty state), and the chips filtering. Confirm a real "Get" routes into Transfers.

- [ ] **Step 10: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: live federated swarm search UI with source chips and Find It"
```

---

### Task 7: Update the roadmap

**Files:**
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1:** Move the M4 section from "Next" into "Done" with a one-paragraph summary of what shipped (federated npm/brew/pip search, source chips, Find It! button, cached brew catalog + analytics, pip exact-name honesty, install via Transfers). Update the "Note" about leftover seeded data to say it is resolved. Set M5 (What's New) as the next milestone.

- [ ] **Step 2: Commit.**

```bash
git add docs/ROADMAP.md
git commit -m "docs: mark M4 search done, M5 next"
```

---

## Self-review notes

- Spec coverage: federated search (T5), chips as client filter (T6), Find It! (T6), npm real (T2), brew cached catalog + analytics (T3), pip exact-name labeled (T4), merge/sort/dedupe (T1), independent source failure (T5/wrappers), honest empty/unreachable states (T6). All covered.
- Type consistency: `SearchResult` defined once (T1), `weekly_downloads` -> serialized `weeklyDownloads`, read as `p.weeklyDownloads` in JS (T6). `search_all(query, cache_dir)` signature consistent between T5 definition and the command call.
- No placeholders: every code step shows real code; every test shows real assertions.
