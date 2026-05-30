# napm Milestone 2: brew, pip, and npx scans

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the Shared Library scan from npm-only to all four sources, so the library shows your real Homebrew formulae, pip packages, and npx-cached tools alongside npm globals.

**Architecture:** One Rust scan module per source (`brew.rs`, `pip.rs`, `npx.rs`), each exposing a pure, unit-tested parse function plus a thin shell/fs wrapper, mirroring the existing `npm.rs`. `scan_all()` aggregates all four and skips any source whose tool is absent. The frontend already renders the `InstalledTool` shape, so it needs only an `npx` source pill and an honest neutral treatment for npx rows.

**Tech Stack:** Rust, `serde_json`, `std::process::Command`, `std::fs`. No new crates, no network.

**Scope note:** This is Milestone 2 of 6 (`docs/superpowers/specs/2026-05-30-napm-real-app-design.md`). It adds offline detection for brew, pip, and npx. Deliberately deferred:
- **npx "latest" / freshness.** npx has no native outdated command; a real latest version needs an npm-registry HTTP call, which belongs with the cached registry layer in M4. In M2, npx rows show the cached installed version and a neutral "tracked, freshness unknown" state. We do NOT show a green up-to-date check for npx, because that would claim a freshness check we did not run (the project's honesty rule).
- **npx Promote to global** and all install/rollback actions belong to M3 (Transfers).
- **Pins, real size** remain their later milestones; npx/brew/pip rows use `pinned: false` and empty `size`, same as npm in M1.

**Environment facts (verified 2026-05-30):**
- `brew` at `/opt/homebrew/bin/brew`. `brew list --versions` and `brew outdated --json=v2` work (71 outdated).
- pip binary is `pip3` at `/usr/bin/pip3` (there is no `pip`). `pip3 list --format=json` and `pip3 list --outdated --format=json` work (28 outdated).
- `~/.npm/_npx/` has 22 hash dirs; each `<hash>/package.json` carries `_npx.packages`.

---

## File structure after this milestone

```
src-tauri/src/scan/
├─ mod.rs     (MODIFIED: shared run() helper; scan_all aggregates 4 sources)
├─ npm.rs     (MODIFIED: use shared run() helper)
├─ brew.rs    (NEW: parse_brew + scan_brew, unit tested)
├─ pip.rs     (NEW: parse_pip + scan_pip with binary detection, unit tested)
└─ npx.rs     (NEW: npx_pkg_name + dedup_npx pure fns + scan_npx fs-walk, unit tested)
frontend/index.html  (MODIFIED: npx source pill + honest npx row rendering)
prototype/napm-prototype.html (MODIFIED: mirror of frontend)
```

---

## Task 1: Shared `run()` helper

Extract the npm shell-runner into a crate-internal helper the new scanners reuse, so each source does not reimplement `Command` plumbing.

**Files:**
- Modify: `src-tauri/src/scan/mod.rs`
- Modify: `src-tauri/src/scan/npm.rs`

- [ ] **Step 1: Add the shared helper to mod.rs**

In `src-tauri/src/scan/mod.rs`, add module declarations for the new sources and a shared `run()` above `scan_all`. The full file becomes:

```rust
use serde::Serialize;
use std::process::Command;

pub mod npm;
pub mod brew;
pub mod pip;
pub mod npx;

/// One row in the Shared Library. Mirrors the prototype's tool shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledTool {
    pub name: String,
    pub eco: String,
    pub pkg: String,
    pub installed: Option<String>,
    pub latest: String,
    pub size: String,
    pub pinned: bool,
}

/// Run a command and return its stdout, ignoring exit status (some tools, like
/// `npm outdated`, exit non-zero when they have results). Returns "" on spawn
/// failure, so a missing tool degrades to an empty scan rather than an error.
pub(crate) fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Aggregate across all sources. A source whose tool is absent contributes
/// nothing (its scanner returns an empty Vec).
pub fn scan_all() -> Vec<InstalledTool> {
    let mut all = Vec::new();
    all.extend(npm::scan_npm());
    all.extend(brew::scan_brew());
    all.extend(pip::scan_pip());
    all.extend(npx::scan_npx());
    all
}
```

- [ ] **Step 2: Point npm.rs at the shared helper**

In `src-tauri/src/scan/npm.rs`, delete the local `run_npm` function and its `use std::process::Command;` import, and change `scan_npm` to call `super::run`:

```rust
pub fn scan_npm() -> Vec<InstalledTool> {
    let ls = super::run("npm", &["ls", "-g", "--depth=0", "--json"]);
    let outdated = super::run("npm", &["outdated", "-g", "--json"]);
    parse_npm(&ls, &outdated)
}
```

Leave `parse_npm` and the existing tests unchanged.

- [ ] **Step 3: Verify the crate still builds and tests pass**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib 2>&1 | tail -6
```
Expected: the 6 existing npm tests still pass. (The new empty modules `brew`/`pip`/`npx` do not exist yet, so this step will fail to compile until Task 2-4 create them. If you are doing tasks strictly in order, create empty `brew.rs`, `pip.rs`, `npx.rs` files with a single `use super::InstalledTool;` line now so the crate compiles, and fill them in the next tasks. Otherwise run this verification after Task 4.)

- [ ] **Step 4: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/scan/mod.rs src-tauri/src/scan/npm.rs
git commit -m "refactor: shared run() helper for source scanners"
```

---

## Task 2: brew scan (TDD)

**Files:**
- Create: `src-tauri/src/scan/brew.rs`

The testable unit is `parse_brew(list_versions, outdated_json)`, mirroring `reference/scanner.js` scanBrew(): start from `brew list --versions` (installed equals latest), then let `brew outdated --json=v2` override latest from `current_version`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/scan/brew.rs` with the tests first:

```rust
use super::InstalledTool;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use serde_json::Value;

// implementation added in Step 3

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_formula_has_equal_installed_and_latest() {
        let list = "aom 3.13.3\nripgrep 14.1.1\n";
        let rows = parse_brew(list, r#"{"formulae":[]}"#);
        let rg = rows.iter().find(|t| t.pkg == "ripgrep").unwrap();
        assert_eq!(rg.eco, "brew");
        assert_eq!(rg.installed.as_deref(), Some("14.1.1"));
        assert_eq!(rg.latest, "14.1.1");
    }

    #[test]
    fn outdated_formula_takes_latest_from_current_version() {
        let list = "aom 3.13.3\n";
        let outdated = r#"{"formulae":[{"name":"aom","installed_versions":["3.13.3"],"current_version":"3.14.1"}]}"#;
        let rows = parse_brew(list, outdated);
        let aom = rows.iter().find(|t| t.pkg == "aom").unwrap();
        assert_eq!(aom.installed.as_deref(), Some("3.13.3"));
        assert_eq!(aom.latest, "3.14.1");
    }

    #[test]
    fn multiple_installed_versions_use_the_last_token() {
        // brew list --versions can list several versions; the last is newest
        let rows = parse_brew("foo 1.0.0 1.2.0\n", r#"{"formulae":[]}"#);
        assert_eq!(rows[0].installed.as_deref(), Some("1.2.0"));
        assert_eq!(rows[0].latest, "1.2.0");
    }

    #[test]
    fn empty_or_garbage_yields_no_rows() {
        assert!(parse_brew("", "").is_empty());
        assert!(parse_brew("", "not json").is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib scan::brew 2>&1 | tail -10
```
Expected: compile error, `cannot find function 'parse_brew'`.

- [ ] **Step 3: Implement parse_brew and scan_brew**

Add above the `#[cfg(test)]` block in `src-tauri/src/scan/brew.rs`:

```rust
/// Merge `brew list --versions` (installed) with `brew outdated --json=v2`
/// (latest). Mirrors reference/scanner.js scanBrew().
pub fn parse_brew(list_versions: &str, outdated_json: &str) -> Vec<InstalledTool> {
    let mut map: BTreeMap<String, (Option<String>, String)> = BTreeMap::new();

    for line in list_versions.lines() {
        let mut parts = line.split_whitespace();
        if let Some(name) = parts.next() {
            // remaining tokens are versions; the last is the newest installed
            if let Some(ver) = parts.last() {
                map.insert(name.to_string(), (Some(ver.to_string()), ver.to_string()));
            }
        }
    }

    if let Ok(od) = serde_json::from_str::<Value>(outdated_json) {
        if let Some(formulae) = od.get("formulae").and_then(|f| f.as_array()) {
            for f in formulae {
                let name = match f.get("name").and_then(|v| v.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };
                let latest = f.get("current_version").and_then(|v| v.as_str()).unwrap_or("");
                let installed = f
                    .get("installed_versions")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.last())
                    .and_then(|v| v.as_str());
                match map.entry(name.to_string()) {
                    Entry::Occupied(mut o) => {
                        let e = o.get_mut();
                        if let Some(i) = installed {
                            e.0 = Some(i.to_string());
                        }
                        if !latest.is_empty() {
                            e.1 = latest.to_string();
                        }
                    }
                    Entry::Vacant(v) => {
                        if !latest.is_empty() {
                            v.insert((installed.map(|s| s.to_string()), latest.to_string()));
                        }
                    }
                }
            }
        }
    }

    map.into_iter()
        .map(|(name, (installed, latest))| InstalledTool {
            name: name.clone(),
            eco: "brew".to_string(),
            pkg: name,
            installed,
            latest,
            size: String::new(),
            pinned: false,
        })
        .collect()
}

/// Run the real brew commands and merge.
pub fn scan_brew() -> Vec<InstalledTool> {
    let list = super::run("brew", &["list", "--versions"]);
    let outdated = super::run("brew", &["outdated", "--json=v2"]);
    parse_brew(&list, &outdated)
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib scan::brew 2>&1 | tail -8
```
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/scan/brew.rs
git commit -m "feat: brew scan + merge logic with unit tests"
```

---

## Task 3: pip scan (TDD)

**Files:**
- Create: `src-tauri/src/scan/pip.rs`

The testable unit is `parse_pip(list_json, outdated_json)`, mirroring scanner.js scanPip(): merge `pip list --format=json` (installed) with `pip list --outdated --format=json` (latest), keyed by lowercased name (pip names are case-insensitive). `scan_pip` additionally probes for the right binary, since this machine has `pip3` but no `pip`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/scan/pip.rs` with the tests first:

```rust
use super::InstalledTool;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::process::Command;
use serde_json::Value;

// implementation added in Step 3

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_package_has_equal_installed_and_latest() {
        let list = r#"[{"name":"absl-py","version":"2.3.1"}]"#;
        let rows = parse_pip(list, "[]");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].eco, "pip");
        assert_eq!(rows[0].pkg, "absl-py");
        assert_eq!(rows[0].installed.as_deref(), Some("2.3.1"));
        assert_eq!(rows[0].latest, "2.3.1");
    }

    #[test]
    fn outdated_package_takes_latest_version() {
        let list = r#"[{"name":"altgraph","version":"0.17.2"}]"#;
        let outdated = r#"[{"name":"altgraph","version":"0.17.2","latest_version":"0.17.5"}]"#;
        let rows = parse_pip(list, outdated);
        assert_eq!(rows[0].installed.as_deref(), Some("0.17.2"));
        assert_eq!(rows[0].latest, "0.17.5");
    }

    #[test]
    fn merge_is_case_insensitive() {
        // pip reports the same dist under different casings; they must not double up
        let list = r#"[{"name":"Flask","version":"3.0.0"}]"#;
        let outdated = r#"[{"name":"flask","version":"3.0.0","latest_version":"3.1.0"}]"#;
        let rows = parse_pip(list, outdated);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].latest, "3.1.0");
        assert_eq!(rows[0].name, "Flask"); // display name from the list entry
    }

    #[test]
    fn empty_or_garbage_yields_no_rows() {
        assert!(parse_pip("", "").is_empty());
        assert!(parse_pip("not json", "[]").is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib scan::pip 2>&1 | tail -10
```
Expected: compile error, `cannot find function 'parse_pip'`.

- [ ] **Step 3: Implement parse_pip and scan_pip**

Add above the `#[cfg(test)]` block:

```rust
/// Merge `pip list --format=json` (installed) with
/// `pip list --outdated --format=json` (latest), keyed by lowercased name.
pub fn parse_pip(list_json: &str, outdated_json: &str) -> Vec<InstalledTool> {
    // key = lowercased name -> (display name, installed, latest)
    let mut map: BTreeMap<String, (String, Option<String>, String)> = BTreeMap::new();

    if let Ok(list) = serde_json::from_str::<Value>(list_json) {
        if let Some(arr) = list.as_array() {
            for p in arr {
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let ver = p.get("version").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() || ver.is_empty() {
                    continue;
                }
                map.insert(
                    name.to_lowercase(),
                    (name.to_string(), Some(ver.to_string()), ver.to_string()),
                );
            }
        }
    }

    if let Ok(od) = serde_json::from_str::<Value>(outdated_json) {
        if let Some(arr) = od.as_array() {
            for p in arr {
                let name = match p.get("name").and_then(|v| v.as_str()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };
                let cur = p.get("version").and_then(|v| v.as_str());
                let latest = p.get("latest_version").and_then(|v| v.as_str()).unwrap_or("");
                match map.entry(name.to_lowercase()) {
                    Entry::Occupied(mut o) => {
                        let e = o.get_mut();
                        if let Some(c) = cur {
                            e.1 = Some(c.to_string());
                        }
                        if !latest.is_empty() {
                            e.2 = latest.to_string();
                        }
                    }
                    Entry::Vacant(v) => {
                        if !latest.is_empty() {
                            v.insert((name.to_string(), cur.map(|s| s.to_string()), latest.to_string()));
                        }
                    }
                }
            }
        }
    }

    map.into_iter()
        .map(|(_, (name, installed, latest))| InstalledTool {
            name: name.clone(),
            eco: "pip".to_string(),
            pkg: name,
            installed,
            latest,
            size: String::new(),
            pinned: false,
        })
        .collect()
}

/// Find a working pip binary. This machine has `pip3` but no `pip`.
fn pip_bin() -> Option<&'static str> {
    for c in ["pip3", "pip"] {
        let ok = Command::new(c)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(c);
        }
    }
    None
}

/// Run the real pip commands and merge. Returns empty if no pip is installed.
pub fn scan_pip() -> Vec<InstalledTool> {
    let bin = match pip_bin() {
        Some(b) => b,
        None => return Vec::new(),
    };
    let list = super::run(bin, &["list", "--format=json"]);
    let outdated = super::run(bin, &["list", "--outdated", "--format=json"]);
    parse_pip(&list, &outdated)
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib scan::pip 2>&1 | tail -8
```
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/scan/pip.rs
git commit -m "feat: pip scan + merge logic with binary detection and unit tests"
```

---

## Task 4: npx scan (TDD)

**Files:**
- Create: `src-tauri/src/scan/npx.rs`

npx has no outdated command, so this task only detects which tools you have run via npx and their cached versions. The two pure, testable units are `npx_pkg_name` (strip the `@version` spec, handling scoped names) and `dedup_npx` (collapse the same tool cached in multiple hash dirs). The `scan_npx` fs-walk is a thin wrapper verified manually. Per the scope note, npx `latest` is set equal to `installed` (a neutral sentinel meaning "freshness unknown"); the honest UI treatment lands in Task 6.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/scan/npx.rs` with the tests first:

```rust
use super::InstalledTool;
use std::collections::BTreeMap;
use std::fs;
use serde_json::Value;

// implementation added in Step 3

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_version_from_plain_spec() {
        assert_eq!(npx_pkg_name("get-shit-done-cc@latest"), "get-shit-done-cc");
        assert_eq!(npx_pkg_name("typescript@5.6.2"), "typescript");
    }

    #[test]
    fn preserves_scoped_names() {
        assert_eq!(npx_pkg_name("@anthropic-ai/claude-code@1.0.0"), "@anthropic-ai/claude-code");
        assert_eq!(npx_pkg_name("@scope/pkg"), "@scope/pkg");
    }

    #[test]
    fn plain_name_without_version_is_unchanged() {
        assert_eq!(npx_pkg_name(" created"), " created"); // no '@', returned as-is
        assert_eq!(npx_pkg_name("eslint"), "eslint");
    }

    #[test]
    fn dedup_keeps_greatest_version_and_tags_npx() {
        let rows = dedup_npx(vec![
            ("tool".to_string(), "1.0.0".to_string()),
            ("tool".to_string(), "1.2.0".to_string()),
            ("other".to_string(), "0.1.0".to_string()),
        ]);
        assert_eq!(rows.len(), 2);
        let tool = rows.iter().find(|t| t.pkg == "tool").unwrap();
        assert_eq!(tool.eco, "npx");
        assert_eq!(tool.installed.as_deref(), Some("1.2.0"));
        // latest == installed: neutral "freshness unknown" sentinel for M2
        assert_eq!(tool.latest, "1.2.0");
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib scan::npx 2>&1 | tail -10
```
Expected: compile error, `cannot find function 'npx_pkg_name'`.

- [ ] **Step 3: Implement npx_pkg_name, dedup_npx, and scan_npx**

Add above the `#[cfg(test)]` block:

```rust
/// Strip the trailing `@version` from an npx package spec, preserving scoped
/// names. "pkg@1.2.3" -> "pkg"; "@scope/pkg@1.2.3" -> "@scope/pkg".
pub fn npx_pkg_name(spec: &str) -> &str {
    match spec.rfind('@') {
        Some(i) if i > 0 => &spec[..i],
        _ => spec,
    }
}

/// Collapse (name, version) pairs into library rows, deduping by name and
/// keeping the greatest version string. latest is set equal to installed: in
/// M2 napm does not know the registry latest for npx tools, so this is a
/// neutral sentinel meaning "freshness unknown" (rendered as such in the UI).
pub fn dedup_npx(items: Vec<(String, String)>) -> Vec<InstalledTool> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (name, ver) in items {
        map.entry(name)
            .and_modify(|v| {
                if ver > *v {
                    *v = ver.clone();
                }
            })
            .or_insert(ver);
    }
    map.into_iter()
        .map(|(name, ver)| InstalledTool {
            name: name.clone(),
            eco: "npx".to_string(),
            pkg: name,
            installed: Some(ver.clone()),
            latest: ver,
            size: String::new(),
            pinned: false,
        })
        .collect()
}

/// Walk ~/.npm/_npx/<hash>/, read each shim's `_npx.packages` to learn which
/// tool was run, and resolve its cached version from node_modules. Returns
/// empty if the cache does not exist.
pub fn scan_npx() -> Vec<InstalledTool> {
    let home = match std::env::var_os("HOME") {
        Some(h) => h,
        None => return Vec::new(),
    };
    let root = std::path::Path::new(&home).join(".npm").join("_npx");
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut items: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let shim = match fs::read_to_string(dir.join("package.json")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let shim: Value = match serde_json::from_str(&shim) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let specs = match shim.get("_npx").and_then(|n| n.get("packages")).and_then(|p| p.as_array()) {
            Some(s) => s,
            None => continue,
        };
        for spec in specs {
            let spec = match spec.as_str() {
                Some(s) => s,
                None => continue,
            };
            let name = npx_pkg_name(spec);
            let pkg_json = dir.join("node_modules").join(name).join("package.json");
            if let Ok(s) = fs::read_to_string(&pkg_json) {
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
                        items.push((name.to_string(), ver.to_string()));
                    }
                }
            }
        }
    }
    dedup_npx(items)
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib 2>&1 | tail -8
```
Expected: all tests pass (6 npm + 4 brew + 4 pip + 4 npx = 18).

- [ ] **Step 5: Manual integration check of the real scan**

Confirm the fs-walk returns real npx tools. Add a temporary throwaway test and run it (do not commit it):

```bash
source "$HOME/.cargo/env" && cd src-tauri
cat >> src/scan/npx.rs <<'EOF'

#[cfg(test)]
mod manual {
    #[test]
    #[ignore]
    fn dump_real_npx() {
        for t in super::scan_npx() {
            println!("{} = {:?}", t.pkg, t.installed);
        }
    }
}
EOF
cargo test --lib scan::npx::manual::dump_real_npx -- --ignored --nocapture 2>&1 | tail -20
git checkout src/scan/npx.rs   # discard the throwaway test
```
Expected: a list of real npx tools with versions (e.g. `get-shit-done-cc = Some("1.42.3")`). If empty, check `ls ~/.npm/_npx`.

- [ ] **Step 6: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/scan/npx.rs
git commit -m "feat: npx cache scan with unit-tested name parsing and dedup"
```

---

## Task 5: Confirm the aggregated command compiles and runs

**Files:**
- (verification only)

- [ ] **Step 1: Build the backend**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo build 2>&1 | tail -6
```
Expected: `Finished`, no errors. `scan_all()` now calls all four scanners.

- [ ] **Step 2: Full test run**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib 2>&1 | tail -6
```
Expected: 18 passed.

No commit (no source change in this task).

---

## Task 6: Frontend: npx source pill and honest npx rows

**Files:**
- Modify: `frontend/index.html`
- Modify: `prototype/napm-prototype.html` (mirror)

The render pipeline already handles npm/brew/pip rows. This task adds the `npx` source pill color and renders npx rows honestly: because their `latest` equals `installed` (the "freshness unknown" sentinel), the default `statusOf` would paint a green up-to-date check, which would falsely claim we checked the registry. Instead npx rows get a neutral glyph and a muted Latest cell.

- [ ] **Step 1: Add the npx pill color**

In `frontend/index.html`, find the `:root` block with `--npm`, `--brew`, `--pip` and add an `--npx` color:

Find:
```css
    --npm:#cb3837; --brew:#d07000; --pip:#2f6690;
```
Replace with:
```css
    --npm:#cb3837; --brew:#d07000; --pip:#2f6690; --npx:#6f42c1;
```

Then find the source-pill rules:
```css
  .src.npm{background:var(--npm);} .src.brew{background:var(--brew);} .src.pip{background:var(--pip);}
```
Replace with:
```css
  .src.npm{background:var(--npm);} .src.brew{background:var(--brew);} .src.pip{background:var(--pip);} .src.npx{background:var(--npx);}
```

- [ ] **Step 2: Render npx rows with a neutral status**

In `frontend/index.html`, find the `renderRows` function's per-row block. It currently starts:
```javascript
    TOOLS.forEach(function(t,i){
      var st=statusOf(t), g=GLYPH[st], off=st==="offline";
```
Replace those two lines with a version that gives npx its own neutral treatment:
```javascript
    TOOLS.forEach(function(t,i){
      var npx = t.eco==="npx";
      var st=statusOf(t), g=npx?["♪","g-off"]:GLYPH[st], off=st==="offline";
```

Then, in the same row template, find the Latest cell and the action cell:
```javascript
        '<td class="'+(st==="update"?'vernew':'muted')+'">'+t.latest+'</td>'+
```
Replace with (npx shows a muted dash instead of a version, since we have not freshness-checked it):
```javascript
        '<td class="'+(st==="update"&&!npx?'vernew':'muted')+'">'+(npx?"—":t.latest)+'</td>'+
```

Find the action cell:
```javascript
      var action = st==="update" ? '<button class="btn rowbtn" data-get="'+i+'">Get</button>'
                 : off ? '<button class="btn rowbtn" data-get="'+i+'">Install</button>' : '<span class="muted">—</span>';
```
Replace with (npx has no action in M2; Promote to global is M3):
```javascript
      var action = npx ? '<span class="muted">—</span>'
                 : st==="update" ? '<button class="btn rowbtn" data-get="'+i+'">Get</button>'
                 : off ? '<button class="btn rowbtn" data-get="'+i+'">Install</button>' : '<span class="muted">—</span>';
```

- [ ] **Step 3: Mirror to the reference prototype**

```bash
cd /Users/zach/Documents/GitHub/napm
cp frontend/index.html prototype/napm-prototype.html
diff -q frontend/index.html prototype/napm-prototype.html && echo identical
```

- [ ] **Step 4: Manual verification (human, GUI)**

Restart the app and look at the Shared Library:
```bash
source "$HOME/.cargo/env" && npm run tauri dev
```
Expected: the library now lists npm globals plus your real Homebrew formulae (orange `brew` pill), pip packages (blue `pip` pill), and npx-cached tools (purple `npx` pill). brew and pip show real outdated arrows; npx rows show the cached version, a neutral note glyph, a muted Latest dash, and no action button. The status bar count climbs to include all sources.

- [ ] **Step 5: Commit**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: render brew/pip/npx sources, with honest neutral npx rows"
```

---

## Self-review (completed during planning)

- **Spec coverage (M2 slice):** brew scan (one batch command, json=v2) Task 2; pip scan (with pip3 detection) Task 3; npx scan of `~/.npm/_npx` Task 4; aggregation in `scan_all` Task 1/5; npx as a distinct source pill Task 6. Deferred-by-design and stated: npx latest/freshness (M4 registry+cache), npx Promote and all actions (M3), pins and real size (later).
- **Placeholder scan:** No TBD/TODO; every code and command step is concrete, with fixtures taken from real machine output.
- **Type consistency:** All scanners produce the same `InstalledTool` (`name`,`eco`,`pkg`,`installed: Option<String>`,`latest`,`size`,`pinned`) defined in `mod.rs`. Function names `parse_brew`/`scan_brew`, `parse_pip`/`scan_pip`/`pip_bin`, `npx_pkg_name`/`dedup_npx`/`scan_npx`, and the shared `run` are used consistently. `scan_all` calls `npm::scan_npm`, `brew::scan_brew`, `pip::scan_pip`, `npx::scan_npx`, all of which exist.
- **Honesty rule:** npx rows never claim up-to-date; they render a neutral glyph and muted Latest because freshness is genuinely unknown until M4. This is the project's "do not fake what is not possible" rule applied.
- **Known approximation:** npx dedup keeps the lexicographically greatest version string, not a true semver max. Documented in code. Acceptable for M2 (display only); revisit if it ever misorders in practice.
