# napm M4.1 - Search performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a warm swarm search return in well under a second by removing avoidable latency, without changing search behavior, results, or honesty.

**Architecture:** Five focused, behavior-preserving optimizations to the existing `src-tauri/src/search/` module: a process-global keep-alive `ureq` agent, concurrent fan-out of the three sources, an in-memory parsed brew catalog (so the multi-MB JSON is parsed once per process, not per search), background warming of that catalog at app launch, and parallel npm per-scoped download lookups. All std-only, no new dependencies.

**Tech Stack:** Rust, Tauri v2, `ureq`, `serde_json`, `std::thread::scope`, `std::sync::{OnceLock, Mutex}`. Rust edition 2021, rust-version 1.77 (so `OnceLock` and `thread::scope` are available).

**Why:** M4 was built blocking and sequential for correctness. `search_all` runs npm, then brew, then pip in series; every HTTP call opens a fresh TLS connection; and brew re-reads and re-parses its multi-MB catalog on every search. See `docs/ROADMAP.md` "M4.1".

---

## Conventions for every task

- Run `source "$HOME/.cargo/env"` before any cargo command.
- Tests: `cd /Users/zach/Documents/GitHub/napm/src-tauri && cargo test --lib`. Build: `cargo build`.
- Behavior must NOT change: same results, same sort, each source still fails independently to an empty list, blank query still returns empty.
- Follow TDD where a pure function is involved. Concurrency/agent/IO changes are verified by the existing tests staying green plus `cargo build` clean; do not invent flaky timing tests.
- NO em dashes in any code, comment, or string. Never the word "Napster" (brand is "npstr").
- Commit after each task with the given message. Keep the existing 48 tests green throughout.

## File structure

- Modify `src-tauri/src/search/http.rs` - process-global agent (Task 1).
- Modify `src-tauri/src/search/mod.rs` - concurrent `search_all` (Task 2).
- Modify `src-tauri/src/search/brew.rs` - in-memory parsed catalog + `warm_brew` (Task 3).
- Modify `src-tauri/src/search/npm.rs` - parallel scoped download lookups (Task 4).
- Modify `src-tauri/src/lib.rs` - warm brew at launch in `setup` (Task 5).

---

### Task 1: Process-global keep-alive HTTP agent

Right now `http::get` builds a brand-new `ureq::Agent` on every call, so every request pays a fresh TLS handshake. A `ureq::Agent` is internally reference-counted, cheap to clone, and `Send + Sync`. Build it once and reuse it for connection keep-alive across all calls and all searches.

**Files:**
- Modify: `src-tauri/src/search/http.rs:1-16`

- [ ] **Step 1: Replace the body of `http.rs` above the `encode` function.** Keep `encode` and its test exactly as they are. Replace lines 1-16 (the `use` and `get`) with:

```rust
use std::sync::OnceLock;
use std::time::Duration;
use ureq::Agent;

/// One process-global agent so connections keep-alive across every search and
/// every source, instead of a fresh TLS handshake per request. ureq Agents are
/// internally reference-counted and Send + Sync, so a single shared instance is
/// safe to use from the concurrent source threads.
fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(4))
            .timeout_read(Duration::from_secs(6))
            .user_agent("napm")
            .build()
    })
}

/// The single place anything in the app touches the network for search.
/// Short timeouts so a dead source never hangs the grid. Returns the body
/// string on 2xx, or an Err message on any failure (caller degrades to no rows).
pub(crate) fn get(url: &str) -> Result<String, String> {
    match agent().get(url).call() {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}
```

- [ ] **Step 2: Build and test.** `cd /Users/zach/Documents/GitHub/napm/src-tauri && cargo build && cargo test --lib`
Expected: clean build, 48 tests still pass (the `encode` test is unchanged; `get` is network code, verified by build + later live use).

- [ ] **Step 3: Commit.**

```bash
git add src-tauri/src/search/http.rs
git commit -m "perf: reuse one process-global ureq agent for keep-alive connections"
```

---

### Task 2: Run the three sources concurrently

`search_all` calls npm, then brew, then pip in series, so total latency is their sum. Run them on three scoped threads so total latency is the slowest single source. `std::thread::scope` lets the threads borrow `query` and `cache_dir` without `'static` bounds. A panicking source thread degrades to an empty list (`join().unwrap_or_default()`), preserving the fail-independently guarantee.

**Files:**
- Modify: `src-tauri/src/search/mod.rs:43-51`

- [ ] **Step 1: Replace the `search_all` body.** Replace lines 43-51 with:

```rust
pub fn search_all(query: &str, cache_dir: &Path) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() { return Vec::new(); }
    // Fan out the three sources concurrently: total latency becomes the slowest
    // source, not the sum. A panicking source thread degrades to an empty list,
    // so one dead registry still never blanks the grid.
    std::thread::scope(|s| {
        let n = s.spawn(|| npm::search_npm(query));
        let b = s.spawn(|| brew::search_brew(query, cache_dir));
        let p = s.spawn(|| pip::search_pip(query));
        merge(vec![
            n.join().unwrap_or_default(),
            b.join().unwrap_or_default(),
            p.join().unwrap_or_default(),
        ])
    })
}
```

- [ ] **Step 2: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, 48 tests pass. `Vec<SearchResult>` is the per-source return type and `Default`s to empty, so `unwrap_or_default()` type-checks.

- [ ] **Step 3: Commit.**

```bash
git add src-tauri/src/search/mod.rs
git commit -m "perf: fan out npm, brew, and pip search concurrently"
```

---

### Task 3: In-memory parsed brew catalog

`search_brew` calls `cached_or_fetch` (disk, 24h) then re-parses the multi-MB catalog JSON on every search. Parse it once per process into a lightweight, pre-lowercased form held in a process-global, refreshed on the same 24h TTL. This also pre-lowercases name/desc so the per-search substring match does no repeated lowercasing. Add a `warm_brew` entry point (used by Task 5) that loads and caches the catalog without searching.

This task restructures the pure search to operate on the parsed form. The existing `parse_analytics` is unchanged. `search_catalog(json, ...)` is replaced by two pure, testable functions: `parse_catalog(json) -> Vec<Formula>` and `search_parsed(&[Formula], query, &analytics) -> Vec<SearchResult>`.

**Files:**
- Modify: `src-tauri/src/search/brew.rs` (replace `search_catalog`, `cached_or_fetch` usage, and `search_brew`; keep `parse_analytics`)

- [ ] **Step 1: Add the `Formula` type and a failing `parse_catalog` test.** At the top of `brew.rs`, after the `use` lines, add:

```rust
/// A catalog formula reduced to the fields search needs, with name and
/// description pre-lowercased so the per-query substring match does no repeated
/// case folding.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    pub name: String,
    pub name_lc: String,
    pub desc: String,
    pub desc_lc: String,
    pub version: String,
}

/// Parse the brew `formula.json` catalog into the lightweight Formula form.
pub fn parse_catalog(json: &str) -> Vec<Formula> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let arr = match v.as_array() { Some(a) => a, None => return Vec::new() };
    let mut out = Vec::with_capacity(arr.len());
    for f in arr {
        let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if name.is_empty() { continue; }
        let desc = f.get("desc").and_then(|x| x.as_str()).unwrap_or("").trim();
        let version = f.get("versions").and_then(|x| x.get("stable"))
            .and_then(|x| x.as_str()).unwrap_or("").to_string();
        out.push(Formula {
            name: name.to_string(),
            name_lc: name.to_lowercase(),
            desc: desc.to_string(),
            desc_lc: desc.to_lowercase(),
            version,
        });
    }
    out
}
```

Add this test inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn parse_catalog_reduces_and_lowercases() {
    let catalog = r#"[
        {"name":"RipGrep","desc":"Recursive Search","versions":{"stable":"14.1.1"}},
        {"name":"","desc":"skip me","versions":{"stable":"1.0"}}
    ]"#;
    let f = parse_catalog(catalog);
    assert_eq!(f.len(), 1); // empty-name entry skipped
    assert_eq!(f[0].name, "RipGrep");
    assert_eq!(f[0].name_lc, "ripgrep");
    assert_eq!(f[0].desc_lc, "recursive search");
    assert_eq!(f[0].version, "14.1.1");
}
```

- [ ] **Step 2: Run the test, confirm it passes.** `cargo test --lib`.

- [ ] **Step 3: Replace `search_catalog` with `search_parsed` and update the existing test.** Delete the old `search_catalog` function (the one taking `catalog_json: &str`). Add:

```rust
/// Search the parsed catalog in-process. Case-insensitive substring on name or
/// description (both already lowercased). weekly_downloads is the 30-day
/// analytics count divided to a rough weekly figure.
pub fn search_parsed(formulae: &[Formula], query: &str, analytics: &BTreeMap<String, u64>) -> Vec<SearchResult> {
    let q = query.to_lowercase();
    let mut out = Vec::new();
    for f in formulae {
        if !f.name_lc.contains(&q) && !f.desc_lc.contains(&q) { continue; }
        let weekly = analytics.get(&f.name).copied().unwrap_or(0) / 4;
        out.push(SearchResult {
            name: f.name.clone(), eco: "brew".into(), pkg: f.name.clone(),
            version: f.version.clone(), weekly_downloads: weekly,
            size: String::new(), description: f.desc.clone(),
        });
    }
    out
}
```

Replace the existing `substring_match_on_name_or_desc_with_weekly_from_analytics` test with one that drives `search_parsed`:

```rust
#[test]
fn substring_match_on_name_or_desc_with_weekly_from_analytics() {
    let formulae = parse_catalog(r#"[
        {"name":"ripgrep","desc":"Recursive search faster than grep","versions":{"stable":"14.1.1"}},
        {"name":"jq","desc":"JSON processor","versions":{"stable":"1.7.1"}}
    ]"#);
    let mut a = BTreeMap::new();
    a.insert("ripgrep".to_string(), 4_000_000u64);
    let hits = search_parsed(&formulae, "search", &a);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].pkg, "ripgrep");
    assert_eq!(hits[0].eco, "brew");
    assert_eq!(hits[0].version, "14.1.1");
    assert_eq!(hits[0].weekly_downloads, 1_000_000); // 4M / 4
    // matches description too:
    assert_eq!(search_parsed(&formulae, "json", &a).len(), 1);
}
```

- [ ] **Step 4: Run the tests, confirm they pass.** `cargo test --lib`.

- [ ] **Step 5: Add the in-memory cache and rewrite `search_brew` + add `warm_brew`.** Add these `use` lines at the top if not present: `use std::sync::{Mutex, OnceLock};` and `use std::sync::Arc;`. Keep `cached_or_fetch` exactly as it is (the disk cache is the cross-restart backstop). Add the in-memory layer and rewrite `search_brew`:

```rust
/// Parsed catalog + analytics held in memory so the multi-MB JSON is parsed
/// once per process, not once per search. `loaded` drives the same 24h refresh
/// as the disk cache. The inner data is Arc'd so a search clones cheaply and
/// runs its substring scan outside the lock.
struct CatalogCache {
    loaded: SystemTime,
    formulae: Arc<Vec<Formula>>,
    analytics: Arc<BTreeMap<String, u64>>,
}

fn catalog_cell() -> &'static Mutex<Option<CatalogCache>> {
    static CELL: OnceLock<Mutex<Option<CatalogCache>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// Load the parsed catalog and analytics, using the in-memory copy when it is
/// under 24h old, otherwise rebuilding it from `cached_or_fetch` (which itself
/// keeps a 24h disk cache). Returns cheap Arc clones. Returns None only when the
/// catalog cannot be obtained at all (no memory copy, no disk copy, no network).
fn load_catalog(cache_dir: &Path) -> Option<(Arc<Vec<Formula>>, Arc<BTreeMap<String, u64>>)> {
    {
        let guard = catalog_cell().lock().unwrap();
        if let Some(c) = guard.as_ref() {
            let fresh = SystemTime::now()
                .duration_since(c.loaded)
                .unwrap_or(Duration::MAX)
                < Duration::from_secs(24 * 60 * 60);
            if fresh && !c.formulae.is_empty() {
                return Some((c.formulae.clone(), c.analytics.clone()));
            }
        }
    }

    let catalog_json = cached_or_fetch(
        &cache_dir.join("brew_catalog.json"),
        "https://formulae.brew.sh/api/formula.json",
    )?;
    let analytics_map = cached_or_fetch(
        &cache_dir.join("brew_analytics.json"),
        "https://formulae.brew.sh/api/analytics/install/30d.json",
    )
    .map(|s| parse_analytics(&s))
    .unwrap_or_default();

    let formulae = Arc::new(parse_catalog(&catalog_json));
    let analytics = Arc::new(analytics_map);

    let mut guard = catalog_cell().lock().unwrap();
    *guard = Some(CatalogCache {
        loaded: SystemTime::now(),
        formulae: formulae.clone(),
        analytics: analytics.clone(),
    });
    Some((formulae, analytics))
}

/// Load and cache the brew catalog without searching. Called in a background
/// thread at launch so the first user search is warm. Best-effort: errors are
/// swallowed by the caller.
pub fn warm_brew(cache_dir: &Path) {
    let _ = load_catalog(cache_dir);
}

/// Search brew formulae using the in-memory parsed catalog (backed by a 24h
/// disk cache and analytics). If the catalog cannot be obtained at all, returns
/// an empty list so brew is simply absent rather than an error.
pub fn search_brew(query: &str, cache_dir: &Path) -> Vec<SearchResult> {
    let (formulae, analytics) = match load_catalog(cache_dir) {
        Some(pair) => pair,
        None => return Vec::new(),
    };
    search_parsed(&formulae, query, &analytics)
}
```

NOTE: remove the old `search_brew` body that called `cached_or_fetch` inline and `search_catalog`. The `SearchResult` import and `cached_or_fetch` stay. If `Arc` ends up imported twice, keep a single `use std::sync::Arc;`.

- [ ] **Step 6: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, all tests pass (now including `parse_catalog_reduces_and_lowercases`, so 49 total). No dead-code warnings (every new function is reachable: `warm_brew` will be wired in Task 5, but it is `pub` and called by `search_brew`'s sibling path; if the build warns that `warm_brew` is unused until Task 5, that single warning is acceptable and resolved in Task 5).

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/search/brew.rs
git commit -m "perf: parse brew catalog once into an in-memory index, add warm_brew"
```

---

### Task 4: Parallelize npm per-scoped download lookups

`search_npm` fetches each scoped package's download count in a sequential loop. With the shared keep-alive agent from Task 1 these can run concurrently. Keep the single bulk call for unscoped names (that is already one request). Run the scoped lookups on scoped threads and merge their maps.

**Files:**
- Modify: `src-tauri/src/search/npm.rs:95-106` (the scoped loop)

- [ ] **Step 1: Replace the sequential scoped loop.** Replace the block that starts with `// Fetch each scoped package individually.` (lines 95-106) with:

```rust
    // Fetch each scoped package concurrently (the bulk endpoint cannot combine
    // scoped names). The shared agent keeps connections warm across them.
    if !scoped.is_empty() {
        let maps: Vec<BTreeMap<String, u64>> = std::thread::scope(|s| {
            let handles: Vec<_> = scoped.iter().map(|pkg| {
                s.spawn(move || {
                    let dl_url = format!(
                        "https://api.npmjs.org/downloads/point/last-week/{}",
                        super::http::encode(pkg)
                    );
                    match super::http::get(&dl_url) {
                        Ok(dl_body) => parse_downloads(&dl_body),
                        Err(_) => BTreeMap::new(),
                    }
                })
            }).collect();
            handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
        });
        for m in maps {
            for (k, v) in m { dl_map.insert(k, v); }
        }
    }
```

- [ ] **Step 2: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, all tests pass. The pure `parse_downloads`/`parse_npm_search` tests are unaffected. `scoped` is a `Vec<String>`; the closures borrow each `pkg: &String` which lives in `scoped` for the scope, so `thread::scope` type-checks.

- [ ] **Step 3: Commit.**

```bash
git add src-tauri/src/search/npm.rs
git commit -m "perf: fetch npm scoped download counts concurrently"
```

---

### Task 5: Warm the brew catalog at launch

The first brew search of a session still pays the cold catalog download and parse. Kick that off in a background thread during Tauri `setup` so it is ready (or nearly) by the time the user searches. Best-effort and non-blocking: startup must not wait on the network.

**Files:**
- Modify: `src-tauri/src/lib.rs:56-65` (the `.setup(...)` closure)

- [ ] **Step 1: Add the warming thread to `setup`.** The existing `setup` closure installs the log plugin. Add a background warm before `Ok(())`. The closure already has `app`; get the app-data dir the same way `open_store` does (note `use tauri::Manager;` is already imported at the top of the file). Replace the `.setup(|app| { ... })` block with:

```rust
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      // Warm the brew catalog in the background so the first search is not cold.
      // Best-effort: never block startup on the network.
      let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
      std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        search::brew::warm_brew(&dir);
      });
      Ok(())
    })
```

- [ ] **Step 2: Confirm `brew` is reachable as `search::brew`.** In `src-tauri/src/search/mod.rs` the module is declared `pub mod brew;`, so `search::brew::warm_brew` resolves from `lib.rs`. If it is declared `mod brew;` (private), change it to `pub mod brew;`. (It is already `pub mod brew;` from M4.)

- [ ] **Step 3: Build and test.** `cargo build && cargo test --lib`
Expected: clean build with NO unused-code warning for `warm_brew` (now called from `setup`), all tests pass.

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/lib.rs
git commit -m "perf: warm the brew catalog in a background thread at launch"
```

---

### Task 6: Live verification (human)

No automated test can confirm the latency win; the human verifies live.

- [ ] **Step 1:** `source "$HOME/.cargo/env" && npm run tauri dev`.
- [ ] **Step 2:** The human confirms: searches now return noticeably faster (warm search well under a second), results are identical to before (same packages, same sort, brew/npm/pip all present), and the first search after launch is not noticeably slower than later ones (catalog warmed). Same honesty states (empty query, unreachable, pip exact-match tag) still behave.

There is no commit for this task.

---

## Self-review notes

- Coverage vs the M4.1 roadmap entry: concurrent sources (T2), one global agent (T1), in-memory parsed catalog (T3), warm at launch (T5), parallel npm scoped lookups (T4). All five covered. Live check (T6).
- Behavior preserved: blank query returns empty (T2 guard kept); each source fails independently (T2 `unwrap_or_default`, T4 per-thread `Err -> empty map`, T3 `None -> empty list`); same results and sort (pure `search_parsed` mirrors the old `search_catalog`; `merge` unchanged).
- Type consistency: `Formula` defined in T3 and used by `parse_catalog`/`search_parsed`/`CatalogCache`. `warm_brew(&Path)` defined T3, called T5. `load_catalog -> Option<(Arc<Vec<Formula>>, Arc<BTreeMap<String,u64>>)>` consistent between definition and `search_brew`/`warm_brew` callers. `http::get(&str) -> Result<String,String>` signature unchanged (T1 only swaps the agent source).
- No placeholders: every code step shows complete code; pure-function steps show real test assertions.
- No new dependencies: `OnceLock`, `Mutex`, `Arc`, `thread::scope` are all std.
