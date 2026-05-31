# napm M5 - What's New Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate the What's New tab with real release and security intelligence: a per-installed OSV security scan, a supply-chain wire, and age-based update verdicts, behind the existing card UI.

**Architecture:** A shared top-level `http` module (lifted from `search/`), plus a new `src-tauri/src/intel/` module with pure parsers (unit-tested against captured JSON) and thin network wrappers. Three layers fan out concurrently behind one `get_whats_new` command; changelogs load lazily via `get_changelog`. Reuses the M4.1 keep-alive agent and `thread::scope` concurrency.

**Tech Stack:** Rust, Tauri v2, `ureq`, `serde_json`, `std::thread::scope`, `std::sync::OnceLock`. No new crates (ISO timestamps parsed with a small pure helper). Vanilla-JS frontend.

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-5-whats-new.md`

## Conventions for every task

- Run `source "$HOME/.cargo/env"` before any cargo command.
- Tests: `cd /Users/zach/Documents/GitHub/napm/src-tauri && cargo test --lib`. Build: `cargo build`.
- Follow the existing `scan/` and `search/` style: pure `parse_*`/logic functions with `#[cfg(test)] mod tests`, thin network wrappers on top (wrappers are not unit-tested; verified live).
- NO em dashes in any code, comment, or UI string. Never the word "Napster" (brand is "npstr").
- The current test count is 49. Each task notes the expected new total.
- Commit after each task with the given message. Dead-code warnings on wrappers unused until a later task are acceptable; do not silence with broad `#[allow]`.
- After the frontend task: `cp frontend/index.html prototype/napm-prototype.html`.

## File structure

- Move `src-tauri/src/search/http.rs` -> `src-tauri/src/http.rs` (shared) (Task 1).
- Create `src-tauri/src/intel/mod.rs` - types + `whats_new` orchestrator (Tasks 2, 6).
- Create `src-tauri/src/intel/osv.rs` - security scan (Task 3).
- Create `src-tauri/src/intel/wire.rs` - supply-chain wire (Task 4).
- Create `src-tauri/src/intel/release.rs` - age verdict + changelog (Task 5).
- Modify `src-tauri/src/lib.rs` - module decls + two commands (Tasks 1, 2, 6).
- Modify `frontend/index.html` - rewire What's New (Task 7); mirror to prototype.
- Modify `docs/ROADMAP.md` (Task 8).

---

### Task 1: Lift the HTTP helper to a shared top-level module

`intel` needs the same keep-alive agent `search` uses. Move it to the crate root so both share one connection pool.

**Files:**
- Move: `src-tauri/src/search/http.rs` -> `src-tauri/src/http.rs`
- Modify: `src-tauri/src/search/mod.rs:4` (remove `pub mod http;`)
- Modify: `src-tauri/src/search/{npm,brew,pip}.rs` (rewrite `super::http::` -> `crate::http::`)
- Modify: `src-tauri/src/lib.rs:1-3` (add `mod http;`)

- [ ] **Step 1: Move the file.**

```bash
cd /Users/zach/Documents/GitHub/napm/src-tauri
git mv src/search/http.rs src/http.rs
```

- [ ] **Step 2: Make its functions crate-visible.** In `src/http.rs`, the two `pub(crate) fn get` and `pub(crate) fn encode` already use `pub(crate)`, which is correct at the crate root. No change needed to their signatures. Leave the `encode` test in place.

- [ ] **Step 3: Remove the old module declaration.** In `src/search/mod.rs`, delete the line `pub mod http;` (line 4).

- [ ] **Step 4: Repoint references.** In `src/search/npm.rs`, `src/search/brew.rs`, and `src/search/pip.rs`, replace every `super::http::` with `crate::http::` (there are calls to `crate::http::get` and `crate::http::encode`). Use a search-replace; confirm with `grep -rn "super::http" src/search/` returning nothing.

- [ ] **Step 5: Declare the module at the crate root.** In `src/lib.rs`, add `mod http;` as the first module line (above `mod scan;`).

- [ ] **Step 6: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, 49 tests pass (the `encode` test moved but still runs).

- [ ] **Step 7: Commit.**

```bash
git add -A
git commit -m "refactor: lift http helper to a shared crate-root module"
```

---

### Task 2: intel module scaffold and shared types

Create the type vocabulary the layers produce, plus the empty module wiring.

**Files:**
- Create: `src-tauri/src/intel/mod.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod intel;`)

- [ ] **Step 1: Create `src/intel/mod.rs` with the types and a passing test.**

```rust
use serde::{Deserialize, Serialize};

pub mod osv;
pub mod wire;
pub mod release;

/// The minimal tool identity the frontend sends for each installed tool.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolRef {
    pub pkg: String,
    pub eco: String,
    pub installed: Option<String>,
    pub latest: String,
}

/// A security finding for an installed tool (Layer 1). `severity` is
/// "malicious" (compromise/hijack) or "vulnerable" (CVE/GHSA).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAlert {
    pub pkg: String,
    pub eco: String,
    pub severity: String,            // "malicious" | "vulnerable"
    pub id: String,                  // e.g. "MAL-2024-1" or "GHSA-..."
    pub summary: String,
    pub installed: String,           // the version the user is holding
    pub fixed_version: Option<String>, // patched version if OSV reports one
    pub link: String,
}

/// One recent ecosystem malware advisory (Layer 2, the wire).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireItem {
    pub id: String,
    pub eco: String,
    pub summary: String,
    pub packages: Vec<String>,
    pub published: String,
    pub link: String,
}

/// An age-based update verdict for an in-scope update (Layer 3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub pkg: String,
    pub eco: String,
    pub version: String,
    pub age_label: String,           // "released 6 days ago", or "" when unknown
    pub recommendation: String,      // "safe" | "new" | "unknown"
}

/// Whether the OSV security check actually ran. The frontend must never imply
/// "safe" when the check could not run, so this is explicit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub alerts: Vec<SecurityAlert>,
    pub security_ok: bool,           // false => OSV check failed, do not imply clean
    pub wire: Vec<WireItem>,
    pub wire_ok: bool,
    pub verdicts: Vec<ReleaseInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_ref_deserializes_from_frontend_shape() {
        let t: ToolRef = serde_json::from_str(
            r#"{"pkg":"eslint","eco":"npm","installed":"9.0.0","latest":"9.10.0"}"#
        ).unwrap();
        assert_eq!(t.pkg, "eslint");
        assert_eq!(t.eco, "npm");
        assert_eq!(t.installed.as_deref(), Some("9.0.0"));
    }
}
```

- [ ] **Step 2: Create empty source files** so the `pub mod` lines compile. Create `src/intel/osv.rs`, `src/intel/wire.rs`, `src/intel/release.rs` each containing only a single line comment `// implemented in a later task` for now. (They get real content in Tasks 3-5. An empty `.rs` file is a valid empty module.)

- [ ] **Step 3: Declare the module.** In `src/lib.rs`, add `mod intel;` beneath `mod ops;`.

- [ ] **Step 4: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, 50 tests (one new `tool_ref_deserializes_from_frontend_shape`).

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/intel/ src-tauri/src/lib.rs
git commit -m "feat: intel module scaffold with SecurityAlert, WireItem, ReleaseInfo types"
```

---

### Task 3: OSV security scan (Layer 1)

OSV `querybatch` returns, per query (aligned to request order), the advisory IDs affecting that version (minimal: id + modified). Details (summary, fixed version) come from a per-id `GET /v1/vulns/<id>`. Only the few flagged packages need a detail fetch. Malicious entries have `MAL-` IDs; everything else is a vulnerability.

**Files:**
- Modify: `src-tauri/src/intel/osv.rs`

- [ ] **Step 1: Write `parse_osv_batch` with a failing test.** Pure: maps the batch response to, for each input index, the list of advisory IDs (empty when clean).

```rust
use super::{SecurityAlert, ToolRef};
use serde_json::Value;

/// OSV ecosystem name for our eco string, or None if OSV does not cover it
/// (brew has no Homebrew ecosystem in OSV).
pub fn osv_ecosystem(eco: &str) -> Option<&'static str> {
    match eco {
        "npm" | "npx" => Some("npm"),
        "pip" => Some("PyPI"),
        _ => None,
    }
}

/// Parse an OSV querybatch response into a vector aligned to the request order:
/// each element is the list of advisory IDs affecting that query (empty = clean).
pub fn parse_osv_batch(json: &str) -> Vec<Vec<String>> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let results = match v.get("results").and_then(|r| r.as_array()) {
        Some(a) => a, None => return Vec::new(),
    };
    results.iter().map(|r| {
        r.get("vulns").and_then(|x| x.as_array()).map(|arr| {
            arr.iter().filter_map(|vuln| vuln.get("id").and_then(|i| i.as_str()).map(String::from)).collect()
        }).unwrap_or_default()
    }).collect()
}
```

Test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_aligns_ids_to_query_order() {
        // 3 queries: clean, vuln, malicious
        let json = r#"{"results":[
            {},
            {"vulns":[{"id":"GHSA-aaaa-bbbb-cccc","modified":"2024-01-01T00:00:00Z"}]},
            {"vulns":[{"id":"MAL-2024-99","modified":"2024-05-01T00:00:00Z"}]}
        ]}"#;
        let ids = parse_osv_batch(json);
        assert_eq!(ids.len(), 3);
        assert!(ids[0].is_empty());
        assert_eq!(ids[1], vec!["GHSA-aaaa-bbbb-cccc".to_string()]);
        assert_eq!(ids[2], vec!["MAL-2024-99".to_string()]);
    }

    #[test]
    fn ecosystem_maps_and_excludes_brew() {
        assert_eq!(osv_ecosystem("npm"), Some("npm"));
        assert_eq!(osv_ecosystem("pip"), Some("PyPI"));
        assert_eq!(osv_ecosystem("brew"), None);
    }
}
```

- [ ] **Step 2: Run, confirm pass.** `cargo test --lib`.

- [ ] **Step 3: Add `parse_osv_vuln` with a failing test.** Pure: a single `/v1/vulns/<id>` detail document -> `(severity, summary, fixed_version)`. Malicious when the id starts with `MAL-`. `fixed_version` is the first `fixed` event found in `affected[].ranges[].events[]`.

```rust
/// Classify and summarize a single OSV vuln detail document.
/// Returns (severity, summary, fixed_version). severity is "malicious" for
/// MAL- ids, else "vulnerable".
pub fn parse_osv_vuln(json: &str) -> Option<(String, String, Option<String>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let id = v.get("id").and_then(|x| x.as_str())?;
    let severity = if id.starts_with("MAL-") { "malicious" } else { "vulnerable" };
    let summary = v.get("summary").and_then(|x| x.as_str())
        .or_else(|| v.get("details").and_then(|x| x.as_str()))
        .unwrap_or("")
        .lines().next().unwrap_or("").trim().to_string();
    // First "fixed" event across all affected ranges.
    let fixed_version = v.get("affected").and_then(|a| a.as_array()).and_then(|affected| {
        affected.iter().find_map(|aff| {
            aff.get("ranges").and_then(|r| r.as_array()).and_then(|ranges| {
                ranges.iter().find_map(|range| {
                    range.get("events").and_then(|e| e.as_array()).and_then(|events| {
                        events.iter().find_map(|ev| ev.get("fixed").and_then(|f| f.as_str()).map(String::from))
                    })
                })
            })
        })
    });
    Some((severity.to_string(), summary, fixed_version))
}
```

Test:

```rust
#[test]
fn vuln_detail_extracts_severity_summary_fixed() {
    let mal = r#"{"id":"MAL-2024-99","summary":"Malicious code in foo","affected":[]}"#;
    let (sev, sum, fixed) = parse_osv_vuln(mal).unwrap();
    assert_eq!(sev, "malicious");
    assert_eq!(sum, "Malicious code in foo");
    assert_eq!(fixed, None);

    let vuln = r#"{"id":"GHSA-x","details":"Prototype pollution\nmore text",
        "affected":[{"ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"1.2.3"}]}]}]}"#;
    let (sev2, sum2, fixed2) = parse_osv_vuln(vuln).unwrap();
    assert_eq!(sev2, "vulnerable");
    assert_eq!(sum2, "Prototype pollution");
    assert_eq!(fixed2, Some("1.2.3".to_string()));
}
```

- [ ] **Step 4: Run, confirm pass.**

- [ ] **Step 5: Write the network wrapper `scan_security`.** Not unit-tested. Signature: `pub fn scan_security(installed: &[ToolRef]) -> Option<Vec<SecurityAlert>>`. Returns `None` when the batch call itself fails (so the caller can set `security_ok=false`); `Some(alerts)` otherwise (possibly empty). Logic:
  1. Build queries only for tools where `osv_ecosystem(eco).is_some()` and `installed.is_some()`. Keep a parallel `Vec<(pkg, eco, installed_version)>` so results map back. If no eligible tools, return `Some(vec![])`.
  2. POST the batch: build the JSON body `{"queries":[{"package":{"ecosystem":E,"name":N},"version":V}, ...]}` and call OSV. (Add a `crate::http::post_json(url, body) -> Result<String,String>` helper in `src/http.rs` mirroring `get`: `agent().post(url).send_string(body)` then `.into_string()`. Include it in this task.)
  3. `parse_osv_batch` the response. For each query index with non-empty ids, take the first id, fetch `https://api.osv.dev/v1/vulns/<id>` (parallelize the detail fetches with `std::thread::scope`), `parse_osv_vuln`, and build a `SecurityAlert` (link = `https://osv.dev/vulnerability/<id>`). Skip ids whose detail fetch fails.
  4. Sort alerts so `severity == "malicious"` comes before `"vulnerable"`.

  Add to `src/http.rs`:

```rust
/// POST a JSON body and return the response string, sharing the same agent.
pub(crate) fn post_json(url: &str, body: &str) -> Result<String, String> {
    match agent().post(url).set("Content-Type", "application/json").send_string(body) {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}
```

- [ ] **Step 6: Build and test.** `cargo test --lib && cargo build`
Expected: 52 tests (two new osv tests added across steps), clean build (a dead-code warning on `scan_security`/`post_json` until Task 6 is acceptable).

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/intel/osv.rs src-tauri/src/http.rs
git commit -m "feat: OSV security scan over installed tools with malicious vs vuln classification"
```

---

### Task 4: Supply-chain wire (Layer 2)

GitHub's global advisory database lists recent malware advisories per ecosystem. Fetch npm and pip, merge, cache for an hour.

**Files:**
- Modify: `src-tauri/src/intel/wire.rs`

**Endpoint:** `https://api.github.com/advisories?type=malware&ecosystem=<npm|pip>&sort=published&per_page=15`
Response: array of `{ "ghsa_id", "summary", "published_at", "html_url", "vulnerabilities":[{"package":{"ecosystem","name"}}] }`. GitHub requires a `User-Agent` (the shared agent already sends "napm") and an `Accept: application/vnd.github+json` header is recommended.

- [ ] **Step 1: Write `parse_advisories` with a failing test.** Pure: `(json, eco) -> Vec<WireItem>`.

```rust
use super::WireItem;
use serde_json::Value;

/// Parse a GitHub global-advisories array into wire items, tagging each with eco.
pub fn parse_advisories(json: &str, eco: &str) -> Vec<WireItem> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let arr = match v.as_array() { Some(a) => a, None => return Vec::new() };
    arr.iter().filter_map(|a| {
        let id = a.get("ghsa_id").and_then(|x| x.as_str())?;
        let summary = a.get("summary").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let published = a.get("published_at").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let link = a.get("html_url").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let packages = a.get("vulnerabilities").and_then(|x| x.as_array()).map(|vs| {
            vs.iter().filter_map(|vuln| {
                vuln.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(String::from)
            }).collect()
        }).unwrap_or_default();
        Some(WireItem { id: id.to_string(), eco: eco.to_string(), summary, packages, published, link })
    }).collect()
}
```

Test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_advisory_array_with_packages() {
        let json = r#"[
          {"ghsa_id":"GHSA-1","summary":"Malware in foo","published_at":"2026-05-19T00:00:00Z",
           "html_url":"https://github.com/advisories/GHSA-1",
           "vulnerabilities":[{"package":{"ecosystem":"npm","name":"foo"}},{"package":{"ecosystem":"npm","name":"bar"}}]}
        ]"#;
        let w = parse_advisories(json, "npm");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].id, "GHSA-1");
        assert_eq!(w[0].eco, "npm");
        assert_eq!(w[0].packages, vec!["foo".to_string(), "bar".to_string()]);
    }
    #[test]
    fn garbage_is_empty() { assert!(parse_advisories("nope", "npm").is_empty()); }
}
```

- [ ] **Step 2: Run, confirm pass.**

- [ ] **Step 3: Write the cached wrapper `fetch_wire`.** Not unit-tested. Signature: `pub fn fetch_wire(cache_dir: &Path) -> Option<Vec<WireItem>>`. Returns `None` only if it cannot produce any list at all (no cache and both fetches fail). Logic:
  1. Reuse the brew cache pattern: a `wire.json` file in `cache_dir`, fresh under 1h. If fresh, read and deserialize it (the cached file stores the merged `Vec<WireItem>` as JSON).
  2. If stale/missing, fetch both ecosystems via `crate::http::get` (the GitHub advisories URLs), `parse_advisories` each, merge (npm first, then pip), sort by `published` descending (string compare on ISO timestamps works), cap to 15 total. Write the merged vec to `wire.json`. Return it.
  3. If both fetches fail and a stale `wire.json` exists, return the stale copy; if not, return `None`.

  Use `serde_json::to_string`/`from_str` for the cache file, and the same `metadata().modified()` freshness check as `brew::cached_or_fetch` (mirror that code; do not call it, since this caches a typed vec not a raw body).

- [ ] **Step 4: Build and test.** `cargo test --lib && cargo build`
Expected: 54 tests (two new wire tests), clean build (dead-code on `fetch_wire` acceptable until Task 6).

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/intel/wire.rs
git commit -m "feat: supply-chain wire from GitHub recent malware advisories, cached hourly"
```

---

### Task 5: Release age verdict and changelog (Layer 3)

Age comes from registry publish timestamps; the verdict is a 7-day boundary. Changelog comes from the upstream GitHub repo's releases, with the repo derived from registry/formula metadata. ISO timestamps are parsed with a small pure helper (no date crate).

**Files:**
- Modify: `src-tauri/src/intel/release.rs`

- [ ] **Step 1: Write the ISO-to-unix helper with a failing test.** Pure: parse a leading `YYYY-MM-DDThh:mm:ss` into Unix seconds using the days-from-civil algorithm.

```rust
/// Convert a leading ISO 8601 datetime ("2024-05-01T12:00:00Z" or
/// "2024-05-01...") to Unix seconds (UTC). Returns None if the date part does
/// not parse. Seconds precision; ignores any fractional/zone suffix.
pub fn iso_to_unix(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 10 { return None; }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let (mut hh, mut mm, mut ss) = (0i64, 0i64, 0i64);
    if s.len() >= 19 && b[10] == b'T' {
        hh = num(11, 13).unwrap_or(0);
        mm = num(14, 16).unwrap_or(0);
        ss = num(17, 19).unwrap_or(0);
    }
    // days_from_civil (Howard Hinnant): days since 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}
```

Test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iso_parses_known_epochs() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("2000-01-01T00:00:00Z"), Some(946684800));
        assert_eq!(iso_to_unix("2024-05-01T12:00:00Z"), Some(1714564800));
        assert_eq!(iso_to_unix("nope"), None);
    }
}
```

- [ ] **Step 2: Run, confirm pass.** (Verify the expected epochs with `date -u -j -f "%Y-%m-%dT%H:%M:%S" "2024-05-01T12:00:00" +%s` if unsure; `1714564800` is correct for that UTC instant.)

- [ ] **Step 3: Add the registry time parsers + verdict with a failing test.**

```rust
use serde_json::Value;

/// npm registry document -> publish unix time for `version` (from `time[version]`).
pub fn parse_npm_time(json: &str, version: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(json).ok()?;
    let t = v.get("time")?.get(version)?.as_str()?;
    iso_to_unix(t)
}

/// PyPI project document -> upload unix time for `version`
/// (`releases[version][0].upload_time_iso_8601`).
pub fn parse_pypi_time(json: &str, version: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(json).ok()?;
    let files = v.get("releases")?.get(version)?.as_array()?;
    let t = files.first()?.get("upload_time_iso_8601").or_else(|| files.first()?.get("upload_time"))?.as_str()?;
    iso_to_unix(t)
}

/// (recommendation, age_label) from a publish time and "now". Settled (>= 7 days)
/// is "safe"; fresher is "new". A None publish time is "unknown".
pub fn age_verdict(published: Option<i64>, now: i64) -> (String, String) {
    let p = match published { Some(p) => p, None => return ("unknown".into(), "".into()) };
    let days = ((now - p).max(0)) / 86400;
    let label = if days < 1 { "released today".to_string() }
        else if days == 1 { "released 1 day ago".to_string() }
        else { format!("released {} days ago", days) };
    let rec = if days >= 7 { "safe" } else { "new" };
    (rec.to_string(), label)
}
```

Test:

```rust
#[test]
fn npm_and_pypi_times_and_verdict() {
    let npm = r#"{"time":{"1.2.3":"2024-05-01T12:00:00Z","modified":"x"}}"#;
    assert_eq!(parse_npm_time(npm, "1.2.3"), Some(1714564800));
    let pypi = r#"{"releases":{"1.0.0":[{"upload_time_iso_8601":"2024-05-01T12:00:00Z"}]}}"#;
    assert_eq!(parse_pypi_time(pypi, "1.0.0"), Some(1714564800));

    let now = 1714564800 + 10 * 86400; // 10 days later
    let (rec, label) = age_verdict(Some(1714564800), now);
    assert_eq!(rec, "safe");
    assert_eq!(label, "released 10 days ago");
    let (rec2, _) = age_verdict(Some(now - 3 * 86400), now);
    assert_eq!(rec2, "new");
    assert_eq!(age_verdict(None, now).0, "unknown");
}
```

- [ ] **Step 4: Run, confirm pass.**

- [ ] **Step 5: Add the GitHub repo extractor + releases parser with a failing test.**

```rust
/// Extract (owner, repo) from a GitHub URL in any common form
/// (git+https://github.com/owner/repo.git, https://github.com/owner/repo, etc).
pub fn github_repo_from_url(url: &str) -> Option<(String, String)> {
    let i = url.find("github.com/")? + "github.com/".len();
    let rest = &url[i..];
    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim().trim_end_matches(".git");
    // strip any trailing query/fragment/path on the repo segment
    let repo = repo.split(['#', '?']).next().unwrap_or(repo);
    if owner.is_empty() || repo.is_empty() { return None; }
    Some((owner.to_string(), repo.to_string()))
}

/// Pull changelog bullet lines from a GitHub releases array for the release whose
/// tag matches `version` (with or without a leading "v"). Returns up to 12
/// non-empty, de-marked lines from that release body.
pub fn parse_github_releases(json: &str, version: &str) -> Vec<String> {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return Vec::new() };
    let arr = match v.as_array() { Some(a) => a, None => return Vec::new() };
    let want = version.trim_start_matches('v');
    let body = arr.iter().find_map(|rel| {
        let tag = rel.get("tag_name").and_then(|t| t.as_str()).unwrap_or("");
        if tag.trim_start_matches('v') == want {
            rel.get("body").and_then(|b| b.as_str())
        } else { None }
    });
    let body = match body { Some(b) => b, None => return Vec::new() };
    body.lines()
        .map(|l| l.trim().trim_start_matches(['#', '-', '*', ' ']).trim())
        .filter(|l| !l.is_empty())
        .take(12)
        .map(String::from)
        .collect()
}
```

Test:

```rust
#[test]
fn repo_url_and_release_notes() {
    assert_eq!(github_repo_from_url("git+https://github.com/eslint/eslint.git"),
               Some(("eslint".to_string(), "eslint".to_string())));
    assert_eq!(github_repo_from_url("https://github.com/cli/cli/tree/trunk"),
               Some(("cli".to_string(), "cli".to_string())));
    assert_eq!(github_repo_from_url("https://example.com/x"), None);

    let rel = r#"[
      {"tag_name":"v1.2.3","body":"# Notes\n- Fixed a bug\n- Added a flag\n"},
      {"tag_name":"v1.2.2","body":"old"}
    ]"#;
    let log = parse_github_releases(rel, "1.2.3");
    assert_eq!(log, vec!["Notes".to_string(), "Fixed a bug".to_string(), "Added a flag".to_string()]);
}
```

- [ ] **Step 6: Run, confirm pass.**

- [ ] **Step 7: Write the thin wrappers.** Not unit-tested.
  - `pub fn release_age(eco: &str, pkg: &str, version: &str, now: i64) -> (String, String)`: for npm/npx GET `https://registry.npmjs.org/<encoded pkg>` -> `parse_npm_time`; for pip GET `https://pypi.org/pypi/<encoded pkg>/json` -> `parse_pypi_time`; brew returns `("unknown", "")` (no clean per-version date). Then `age_verdict`.
  - `pub fn changelog(eco: &str, pkg: &str, version: &str, cache_dir: &Path) -> Vec<String>`: cache permanently per `(eco,pkg,version)` in `cache_dir` as `changelog_<eco>_<sanitized pkg>_<version>.json` (sanitize by replacing `/` and `@` with `_`). On miss: derive the GitHub repo (npm: GET registry doc, read `repository.url`; pip: GET pypi doc, scan `info.project_urls` values and `info.home_page` for a github.com URL; brew: GET `https://formulae.brew.sh/api/formula/<pkg>.json` and read `homepage`), `github_repo_from_url`, then GET `https://api.github.com/repos/<owner>/<repo>/releases?per_page=20` (send header `Accept: application/vnd.github+json`; if `GITHUB_TOKEN` env var is set, also send `Authorization: Bearer <token>`), `parse_github_releases`. Cache and return (cache even an empty result to avoid re-hitting a rate limit). Any failure returns `Vec::new()`.

  For the token + Accept header, extend `crate::http` with a small `get_with_headers(url, headers: &[(&str,&str)]) -> Result<String,String>` mirroring `get`, and use it here and (optionally) in Task 4's wire fetch.

- [ ] **Step 8: Build and test.** `cargo test --lib && cargo build`
Expected: 57 tests (three new release tests), clean build (dead-code on `release_age`/`changelog` acceptable until Task 6).

- [ ] **Step 9: Commit.**

```bash
git add src-tauri/src/intel/release.rs src-tauri/src/http.rs
git commit -m "feat: release age verdict and GitHub changelog with offline-derived repo"
```

---

### Task 6: Orchestrate whats_new + the two commands

**Files:**
- Modify: `src-tauri/src/intel/mod.rs` (add `whats_new`)
- Modify: `src-tauri/src/lib.rs` (two commands + registration)

- [ ] **Step 1: Add `whats_new` to `intel/mod.rs`.** Runs the three layers concurrently. `verdict_scope` is the list of pkg names (matching ToolRef.pkg) the frontend wants age verdicts for. Skips verdict packages that already appear as a security alert (security supersedes a plain verdict).

```rust
use std::path::Path;

/// Run all three layers concurrently and assemble the feed payload.
pub fn whats_new(installed: &[ToolRef], verdict_scope: &[String], cache_dir: &Path, now: i64) -> WhatsNew {
    std::thread::scope(|s| {
        let sec = s.spawn(|| osv::scan_security(installed));
        let wir = s.spawn(|| wire::fetch_wire(cache_dir));
        let ver = s.spawn(|| {
            verdict_scope.iter().filter_map(|pkg| {
                let t = installed.iter().find(|t| &t.pkg == pkg)?;
                let (rec, age_label) = release::release_age(&t.eco, &t.pkg, &t.latest, now);
                Some(ReleaseInfo {
                    pkg: t.pkg.clone(), eco: t.eco.clone(), version: t.latest.clone(),
                    age_label, recommendation: rec,
                })
            }).collect::<Vec<_>>()
        });

        let sec = sec.join().unwrap_or(None);
        let wir = wir.join().unwrap_or(None);
        let mut verdicts = ver.join().unwrap_or_default();

        let (alerts, security_ok) = match sec {
            Some(a) => (a, true),
            None => (Vec::new(), false),
        };
        // Drop verdicts that are already covered by a security alert.
        let flagged: std::collections::BTreeSet<&str> = alerts.iter().map(|a| a.pkg.as_str()).collect();
        verdicts.retain(|v| !flagged.contains(v.pkg.as_str()));

        let (wire, wire_ok) = match wir {
            Some(w) => (w, true),
            None => (Vec::new(), false),
        };
        WhatsNew { alerts, security_ok, wire, wire_ok, verdicts }
    })
}
```

- [ ] **Step 2: Add the commands to `lib.rs`.** Reuse the `app_data_dir` pattern. `now` is computed from the system clock here (not in the pure logic).

```rust
#[tauri::command]
fn get_whats_new(app: tauri::AppHandle, installed: Vec<intel::ToolRef>, verdict_scope: Vec<String>) -> intel::WhatsNew {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    intel::whats_new(&installed, &verdict_scope, &dir, now)
}

#[tauri::command]
fn get_changelog(app: tauri::AppHandle, eco: String, pkg: String, version: String) -> Vec<String> {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    intel::release::changelog(&eco, &pkg, &version, &dir)
}
```

Add `get_whats_new, get_changelog` to the `generate_handler!` list.

- [ ] **Step 3: Build and test.** `cargo test --lib && cargo build`
Expected: 57 tests still pass, clean build with NO remaining dead-code warnings in `intel` (every wrapper is now reachable).

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/intel/mod.rs src-tauri/src/lib.rs
git commit -m "feat: whats_new orchestrator and get_whats_new/get_changelog commands"
```

---

### Task 7: Frontend - rewire the What's New tab

**Files:**
- Modify: `frontend/index.html` (the What's New section ~406-429, the `REC`/`FEED` vars ~270-271, the feed badge ~338, and the tab-open path)
- Then: `cp frontend/index.html prototype/napm-prototype.html`

Touch ONLY the What's New portion plus the shared helpers it needs. Do NOT alter the library, search, transfers, appetite dial internals, or window chrome. Match the existing vanilla style (`var`, string-concatenated HTML, `esc()` for text, the `window.__TAURI__.core.invoke` access pattern used elsewhere).

- [ ] **Step 1: Grow the `REC` table.** Replace line ~271:

```js
var REC={
  malicious:["☠ Compromised — act now","malicious"],
  security:["🛡 Update (security)","security"],
  safe:["✓ Safe to take","safe"],
  "new":["○ New, little signal yet","new"]
};
```

Add CSS for the `.rec.malicious` class near the existing `.rec.security`/`.rec.safe`/`.rec.hold` rules (find them in the `<style>` block): a strong red, e.g. `.rec.malicious{background:#a00;color:#fff;}`. Also add a `.wire`/`.wire-item` style block (see Step 3). Do not invent variables; reuse existing ones where present.

- [ ] **Step 2: Replace `FEED` population and `renderFeed`.** `FEED` stays the array the card renderer reads, but is now built from the backend payload. Add module-level `var WIRE=[]; var SECURITY_OK=true; var WIRE_OK=true;`. Rewrite the loader and renderer:

```js
function verdictScope(){
  // outdated, non-npx, unpinned, within the current appetite dial.
  return TOOLS.filter(function(t){
    return statusOf(t)==="update" && t.eco!=="npx" && !t.pinned && isSafe(bumpKind(t.installed,t.latest));
  }).map(function(t){ return t.pkg; });
}
function loadWhatsNew(){
  var inv=window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke;
  if(!inv){ FEED=[]; WIRE=[]; renderFeed(); return; }
  var installed=TOOLS.filter(function(t){return t.installed;}).map(function(t){
    return {pkg:t.pkg, eco:t.eco, installed:t.installed, latest:t.latest};
  });
  inv("get_whats_new",{installed:installed, verdictScope:verdictScope()}).then(function(r){
    r=r||{};
    SECURITY_OK = r.securityOk!==false;
    WIRE_OK = r.wireOk!==false;
    WIRE = r.wire||[];
    // Build FEED items (cards) from alerts first, then verdicts.
    var feed=[];
    (r.alerts||[]).forEach(function(a){
      var ti=findToolIdx(a.pkg, a.eco);
      feed.push({ti:ti, pkg:a.pkg, eco:a.eco, rec:a.severity, age:"security advisory",
        blurb:a.summary||"A security advisory affects your installed version.",
        changelog:[], loaded:false,
        signals:[{c:(a.severity==="malicious"?"danger":"warn"), lbl:a.id,
          txt:(a.fixedVersion?("fixed in "+a.fixedVersion):"no fixed version published — consider removing")}],
        fix:a.fixedVersion||null, link:a.link||""});
    });
    (r.verdicts||[]).forEach(function(v){
      var ti=findToolIdx(v.pkg, v.eco);
      feed.push({ti:ti, pkg:v.pkg, eco:v.eco, rec:v.recommendation==="unknown"?"new":v.recommendation,
        age:v.ageLabel||"signal unknown", blurb:reBlurb(v), changelog:[], loaded:false,
        signals:[], fix:null, link:""});
    });
    FEED=feed; renderFeed(); renderStatus();
  }).catch(function(){ FEED=[]; WIRE=[]; SECURITY_OK=false; renderFeed(); });
}
function reBlurb(v){
  if(v.recommendation==="safe") return "Settled release, no advisories. Safe to take.";
  if(v.recommendation==="new") return "Fresh release with little signal yet. Your call.";
  return "No release-age signal available for this source.";
}
function findToolIdx(pkg, eco){ for(var i=0;i<TOOLS.length;i++) if(TOOLS[i].pkg===pkg && TOOLS[i].eco===eco) return i; return -1; }
```

- [ ] **Step 3: Rewrite `renderFeed`** to render the wire strip, the "couldn't check" banners, and the cards (which now come straight from `FEED`, already ordered alerts-then-verdicts). Lazy-load changelog on expand.

```js
function renderFeed(){
  var f=document.getElementById("feed"); f.innerHTML="";
  // Wire strip.
  if(WIRE.length){
    var items=WIRE.slice(0,8).map(function(w){
      return '<div class="wire-item"><span class="src '+esc(w.eco)+'">'+esc(w.eco)+'</span> '+
        esc(w.summary||w.id)+(w.packages&&w.packages.length?' <span class="muted">('+esc(w.packages.slice(0,4).join(", "))+(w.packages.length>4?", …":"")+')</span>':'')+'</div>';
    }).join("");
    f.innerHTML+='<div class="wire"><div class="wire-h">📡 supply-chain wire</div>'+items+'</div>';
  } else if(!WIRE_OK){
    f.innerHTML+='<div class="wire"><div class="wire-h">📡 supply-chain wire</div><div class="wire-item muted">wire unavailable</div></div>';
  }
  if(!SECURITY_OK){
    f.innerHTML+='<div class="signal danger" style="margin:6px 0"><span class="lbl">security:</span> check unavailable — could not reach the advisory database, so this is not an all-clear.</div>';
  }
  if(!FEED.length){
    f.innerHTML+='<div class="empty">'+(SECURITY_OK?'You’re on the latest of everything, and nothing you have is flagged.':'No update verdicts to show.')+'</div>';
    return;
  }
  FEED.forEach(function(it,idx){
    var nm=it.ti>=0?TOOLS[it.ti].name:it.pkg;
    var verline=it.ti>=0&&TOOLS[it.ti].installed?(esc(TOOLS[it.ti].installed)+' → '+esc(it.fix||TOOLS[it.ti].latest)):esc(it.fix||"");
    var r=REC[it.rec]||REC["new"];
    var sig=it.signals.map(function(s){return '<div class="signal '+s.c+'"><span class="lbl">'+esc(s.lbl)+':</span> '+esc(s.txt)+'</div>';}).join("");
    var log=it.changelog.length?('<ul>'+it.changelog.map(function(c){return '<li>'+esc(c)+'</li>';}).join("")+'</ul>')
      :(it.loaded?'<div class="muted">No changelog available.</div>':'<div class="muted">Expand to load changelog…</div>');
    var actLabel = it.fix?("Get "+it.fix):(it.ti>=0?("Get "+TOOLS[it.ti].latest):"Get");
    var act = (it.rec==="malicious"&&!it.fix)
      ? '<div class="signal danger" style="margin-top:8px">No safe version published. Remove it: <code>'+esc(removeCmd(it))+'</code></div>'
      : (it.ti>=0?'<button class="btn rowbtn" style="margin-top:8px" data-getfeed="'+idx+'">'+esc(actLabel)+'</button>':'');
    var card=document.createElement("div"); card.className="card raised";
    card.innerHTML='<div class="card-h" data-card="'+idx+'"><span class="caret">+</span>'+
      '<span class="nm">'+esc(nm)+'</span><span class="ver">'+verline+'</span>'+
      '<span class="age">'+esc(it.age)+'</span><span class="rec '+r[1]+'">'+r[0]+'</span></div>'+
      '<div class="card-b"><div>'+esc(it.blurb)+'</div>'+
      '<div style="font-weight:bold;margin-top:6px">What changed</div>'+log+sig+act+'</div>';
    f.appendChild(card);
  });
}
function removeCmd(it){
  if(it.eco==="npm"||it.eco==="npx") return "npm rm -g "+it.pkg;
  if(it.eco==="pip") return "pip uninstall "+it.pkg;
  return "brew uninstall "+it.pkg;
}
```

- [ ] **Step 4: Update the feed click handler** to lazy-load changelog on expand and route the action (update to `fix` when present, else latest). Replace the existing handler (~424-429):

```js
document.getElementById("feed").addEventListener("click",function(e){
  var h=e.target.closest("[data-card]");
  if(h){
    var c=h.parentElement; c.classList.toggle("open");
    h.querySelector(".caret").textContent=c.classList.contains("open")?"–":"+";
    var idx=+h.dataset.card, it=FEED[idx];
    if(c.classList.contains("open") && it && !it.loaded){
      it.loaded=true;
      var inv=window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke;
      var ver=it.fix||(it.ti>=0?TOOLS[it.ti].latest:it.pkg);
      if(inv){ inv("get_changelog",{eco:it.eco,pkg:it.pkg,version:ver}).then(function(log){ it.changelog=log||[]; renderFeed(); }).catch(function(){ renderFeed(); }); }
    }
    return;
  }
  var g=e.target.closest("[data-getfeed]");
  if(g){ var it2=FEED[+g.dataset.getfeed]; if(it2 && it2.ti>=0){ queueTransfer(it2.ti, it2.fix||TOOLS[it2.ti].latest, "update"); switchTab("transfers"); } }
});
```

- [ ] **Step 5: Trigger the load.** The feed badge currently shows `FEED.length` (line ~338) - keep that, it now reflects real cards. Call `loadWhatsNew()` when the What's New tab is opened and after a library scan. In the tab-click handler (the `.tab` listener, ~line where it calls `switchTab`), add: when `t.dataset.view==="whatsnew"`, call `loadWhatsNew()` before/with `switchTab("whatsnew")`. Also call `loadWhatsNew()` at the end of the successful `scanLibrary()` callback (where it currently calls `renderRows(); renderFeed(); renderStatus();` near line 540, replace `renderFeed()` there with `loadWhatsNew()`), and remove the standalone `renderFeed()` from the initial boot line (~569) since `loadWhatsNew` will populate it. Keep the empty-state correct before data arrives.

- [ ] **Step 6: Add the wire + malicious CSS.** Near the card styles, add:

```css
.wire{border:1px solid var(--dgray); background:#1a1a1a; color:#cfc; font-family:var(--mono); font-size:11px; margin-bottom:8px;}
.wire-h{background:#000; color:#9f9; padding:2px 6px; font-weight:bold;}
.wire-item{padding:3px 6px; border-top:1px solid #333;}
.rec.malicious{background:#a00; color:#fff;}
```

(Match the existing palette; if `--dgray` is the variable used elsewhere, reuse it. Verify variable names in `:root`.)

- [ ] **Step 7: Mirror and manually verify.**

```bash
cp /Users/zach/Documents/GitHub/napm/frontend/index.html /Users/zach/Documents/GitHub/napm/prototype/napm-prototype.html
```

Do NOT run the app yourself. The human will run `npm run tauri dev` and verify: the wire strip appears, safe/new verdict cards render for the in-scope updates, expanding a card lazy-loads a changelog, any flagged installed package shows a red security card, and a forced OSV failure shows "check unavailable" rather than an all-clear.

- [ ] **Step 8: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: live What's New with security alerts, supply-chain wire, and age verdicts"
```

---

### Task 8: Update the roadmap

**Files:**
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1:** Move the M5 section from "Next" into "Done" with a summary (per-installed OSV security scan with malicious vs vulnerable, the GitHub supply-chain wire, age-based safe/new verdicts scoped to the appetite set, lazy changelogs, honest brew/coverage gaps, "never imply safe when the check failed"). Note deferrals carried forward (issue-velocity hold, brew CVE mapping, uninstall op, dial security-only notch). Set M6 (menu bar) as the next milestone.

- [ ] **Step 2: Commit.**

```bash
git add docs/ROADMAP.md
git commit -m "docs: mark M5 What's New done, M6 next"
```

---

## Self-review notes

- Spec coverage: Layer 1 OSV scan (T3), Layer 2 wire (T4), Layer 3 age verdicts (T5), orchestration + commands (T6), shared http (T1), types (T2), frontend with the four verdict states + wire + "couldn't check" honesty + lazy changelog + remove-instruction (T7), roadmap (T8). brew honesty gap handled in `osv_ecosystem` (T3) and `release_age` brew->unknown (T5). Optional GITHUB_TOKEN in T5/T4.
- Type consistency: `ToolRef`, `SecurityAlert`, `WireItem`, `ReleaseInfo`, `WhatsNew` defined once (T2), produced by T3/T4/T5, assembled by `whats_new` (T6). Serde camelCase means the frontend reads `securityOk`, `wireOk`, `fixedVersion`, `ageLabel` (T7 matches). `whats_new(installed, verdict_scope, cache_dir, now)` signature consistent between T6 definition and the `get_whats_new` command call.
- No placeholders: every code step has complete code; every pure-function step has real assertions. The `1714564800` epoch is the real Unix time for 2024-05-01T12:00:00Z.
- No new crates: ISO parsing is the pure `iso_to_unix` helper.
