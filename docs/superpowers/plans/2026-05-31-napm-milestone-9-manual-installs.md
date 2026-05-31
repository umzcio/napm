# napm M9 - Manual / standalone installs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fifth "manual / unmanaged" library source that surfaces CLI tools installed outside any package manager (curl|bash installs, direct downloads), with best-effort version, real on-disk size/age, an honest "unmanaged" status, and filesystem-only actions. Never imply an update path.

**Architecture:** A new pure-heavy Rust scanner `scan/manual.rs` does a broad `$PATH` sweep, resolves symlinks, excludes anything owned by a package manager / app bundle / toolchain / system, dedups by resolved target, and versions each survivor (filename first, then a timeout-bounded `<tool> --version` only for binaries under `$HOME`). `scan_all` gates it on a new `Sources.manual` flag and feeds it the names the other four scanners returned. The frontend adds an "unmanaged" status, a manual source toggle (View + Preferences), and a manual-only right-click menu, all reusing existing machinery.

**Tech Stack:** Rust (Tauri backend, std only - no new crates), serde, vanilla JS frontend (single file `frontend/index.html`).

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-9-manual-installs.md`

---

## CRITICAL conventions (read before any task)

- After EVERY edit to `frontend/index.html`, run `cp frontend/index.html prototype/napm-prototype.html` so the two stay byte-identical. This is a required step in each frontend task.
- No em dashes in any copy you write. (The existing `—` placeholder glyph in table cells is pre-existing UI furniture and is reused as-is, not new copy.)
- Run `source "$HOME/.cargo/env"` before any cargo command.
- Backend tests: `cd src-tauri && cargo test --lib`.
- Commit author for every commit: `git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit ...`
- Do NOT push. Local commits only.

## File structure

- Create: `src-tauri/src/scan/manual.rs` - the new scanner and its pure helpers.
- Modify: `src-tauri/src/scan/mod.rs` - declare the module, gate + wire `scan_manual` into `scan_all`.
- Modify: `src-tauri/src/store.rs` - add `manual` to `Sources`.
- Modify: `src-tauri/src/lib.rs` - add the `reveal_in_finder` command and register it.
- Modify: `frontend/index.html` (+ mirror to `prototype/napm-prototype.html`) - status, rendering, toggles, menu, preferences.

---

## Task 1: Add `manual` to the Sources struct

**Files:**
- Modify: `src-tauri/src/store.rs:18-28` (the `Sources` struct + `Default`)
- Modify: `src-tauri/src/store.rs` test module (extend existing tests)

- [ ] **Step 1: Update the existing `partial_settings_keeps_other_sources_on` test to also assert `manual`**

Replace the body of that test (currently at `src-tauri/src/store.rs:179-190`) so it expects the new field:

```rust
    #[test]
    fn partial_settings_keeps_other_sources_on() {
        // A settings.json that only disables npm must keep brew/pip/npx/manual on,
        // never drop the unspecified sources to false.
        let s = temp_store();
        std::fs::create_dir_all(&s.dir_for_test()).unwrap();
        std::fs::write(s.dir_for_test().join("settings.json"), br#"{"sources":{"npm":false}}"#).unwrap();
        let got = s.settings();
        assert!(!got.sources.npm);
        assert!(got.sources.brew && got.sources.pip && got.sources.npx && got.sources.manual);
        assert_eq!(got.github_token, "");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib partial_settings_keeps_other_sources_on`
Expected: FAIL to compile (no field `manual` on `Sources`).

- [ ] **Step 3: Add the `manual` field to `Sources` and its `Default`**

In `src-tauri/src/store.rs`, change the struct and impl (lines 18-28):

```rust
/// Which ecosystems the scan and search cover. Defaults to all enabled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sources {
    pub npm: bool,
    pub brew: bool,
    pub pip: bool,
    pub npx: bool,
    pub manual: bool,
}
impl Default for Sources {
    fn default() -> Self { Sources { npm: true, brew: true, pip: true, npx: true, manual: true } }
}
```

- [ ] **Step 4: Update `settings_round_trip` to construct the new field**

In `src-tauri/src/store.rs:161-162`, the test builds a `Sources { ... }` literal. Add `manual: true`:

```rust
        s.set_settings(&Settings { github_token: "abc".into(),
            sources: Sources { npm: true, brew: false, pip: true, npx: true, manual: true } });
```

- [ ] **Step 5: Run the store tests to verify they pass**

Run: `cd src-tauri && cargo test --lib store`
Expected: PASS (all store tests green, including the corrupt/partial cases reading `manual` as true).

- [ ] **Step 6: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/store.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): add manual source flag to Sources"
```

---

## Task 2: Version parser `first_version`

**Files:**
- Create: `src-tauri/src/scan/manual.rs`

A single pure function extracts the first semver-ish token (`MAJOR.MINOR` optionally `.PATCH`) from any string. Used for both the resolved filename and `--version` output.

- [ ] **Step 1: Create the file with the failing tests**

Create `src-tauri/src/scan/manual.rs`:

```rust
use super::InstalledTool;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The first version-like token in `s`: a run of digits and dots that contains
/// at least one dot and starts with a digit (e.g. "0.2.14", "1.4"). Trailing
/// dots are trimmed. Returns None when there is no such token.
/// "grok-0.2.14-macos-aarch64" -> "0.2.14"; "grok 0.2.14 (e0d895d)" -> "0.2.14".
pub fn first_version(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let tok = s[start..i].trim_end_matches('.');
            let parts: Vec<&str> = tok.split('.').collect();
            if parts.len() >= 2
                && !parts[0].is_empty()
                && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
            {
                return Some(tok.to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_from_filename() {
        assert_eq!(first_version("grok-0.2.14-macos-aarch64").as_deref(), Some("0.2.14"));
        assert_eq!(first_version("tool-v1.2").as_deref(), Some("1.2"));
        assert_eq!(first_version("agy").as_deref(), None);
        assert_eq!(first_version("aarch64").as_deref(), None); // digits, no dot
    }

    #[test]
    fn version_from_output() {
        assert_eq!(first_version("grok 0.2.14 (e0d895d)").as_deref(), Some("0.2.14"));
        assert_eq!(first_version("v1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(first_version("some build, no version here").as_deref(), None);
    }
}
```

- [ ] **Step 2: Register the module so it compiles**

In `src-tauri/src/scan/mod.rs`, add to the module list (after `pub mod npx;` at line 8):

```rust
pub mod manual;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib manual::tests::version`
Expected: PASS for both `version_from_filename` and `version_from_output`.

(Note: `InstalledTool`, `BTreeMap`, `BTreeSet`, `Path`, `PathBuf` imports are unused until later tasks; that is fine - `cargo test` warns but does not fail. They are used by Tasks 3-5.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/scan/manual.rs src-tauri/src/scan/mod.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): version token parser for manual installs"
```

---

## Task 3: Exclusion predicate `is_managed`

**Files:**
- Modify: `src-tauri/src/scan/manual.rs`

A pure function that decides whether a resolved binary path belongs to something napm should NOT claim as manual.

- [ ] **Step 1: Add the failing test**

Append to the `tests` module in `src-tauri/src/scan/manual.rs`:

```rust
    #[test]
    fn excludes_managed_paths_and_known_names() {
        let mut roots: Vec<PathBuf> = vec![
            PathBuf::from("/opt/homebrew"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/Users/x/.cargo"),
        ];
        roots.sort();
        let mut names = BTreeSet::new();
        names.insert("eslint".to_string());

        // Homebrew cellar (under /opt/homebrew)
        assert!(is_managed(Path::new("/opt/homebrew/Cellar/foo/1.0/bin/foo"), "foo", &roots, &names));
        // app bundle CLI
        assert!(is_managed(Path::new("/Applications/Docker.app/Contents/Resources/bin/docker"), "docker", &roots, &names));
        // cargo toolchain
        assert!(is_managed(Path::new("/Users/x/.cargo/bin/cargo"), "cargo", &roots, &names));
        // system dir
        assert!(is_managed(Path::new("/usr/bin/ls"), "ls", &roots, &names));
        // name already owned by npm/pip/npx/brew scan, regardless of path
        assert!(is_managed(Path::new("/Users/x/.local/bin/eslint"), "eslint", &roots, &names));
        // a genuinely-manual tool: not excluded
        assert!(!is_managed(Path::new("/Users/x/.local/bin/agy"), "agy", &roots, &names));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd src-tauri && cargo test --lib manual::tests::excludes`
Expected: FAIL to compile (`is_managed` not defined).

- [ ] **Step 3: Implement `is_managed`**

Add to `src-tauri/src/scan/manual.rs` (above the `tests` module):

```rust
/// True when `real` (a fully-resolved path) belongs to something napm must not
/// claim as a manual install: an app bundle, any managed root prefix, or a
/// basename already returned by the npm/brew/pip/npx scans.
pub fn is_managed(
    real: &Path,
    basename: &str,
    managed_roots: &[PathBuf],
    other_names: &BTreeSet<String>,
) -> bool {
    if other_names.contains(basename) {
        return true;
    }
    if real.to_string_lossy().contains(".app/") {
        return true;
    }
    managed_roots.iter().any(|root| real.starts_with(root))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib manual::tests::excludes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/scan/manual.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): exclusion predicate for managed binaries"
```

---

## Task 4: Dedup by resolved target `dedup_by_target`

**Files:**
- Modify: `src-tauri/src/scan/manual.rs`

Collapse rows that resolve to the same real binary (grok reachable four ways) into one, keyed on the resolved path stored in `description`.

- [ ] **Step 1: Add the failing test**

Append to the `tests` module:

```rust
    fn manual_row(name: &str, target: &str, ver: &str) -> InstalledTool {
        InstalledTool {
            name: name.to_string(),
            eco: "manual".to_string(),
            pkg: name.to_string(),
            installed: Some(ver.to_string()),
            latest: ver.to_string(),
            size: String::new(),
            pinned: false,
            publisher: "local".to_string(),
            description: target.to_string(),
            updated: 0,
            requested: true,
        }
    }

    #[test]
    fn dedup_collapses_same_target() {
        let rows = dedup_by_target(vec![
            manual_row("grok", "/Users/x/.grok/downloads/grok-0.2.14", "0.2.14"),
            manual_row("grok", "/Users/x/.grok/downloads/grok-0.2.14", "0.2.14"),
            manual_row("agent", "/Users/x/.grok/downloads/agent-bin", "0.2.14"),
        ]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.name == "grok"));
        assert!(rows.iter().any(|r| r.name == "agent"));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd src-tauri && cargo test --lib manual::tests::dedup`
Expected: FAIL to compile (`dedup_by_target` not defined).

- [ ] **Step 3: Implement `dedup_by_target`**

Add to `src-tauri/src/scan/manual.rs` (above `tests`):

```rust
/// Collapse rows sharing a resolved target path (stored in `description`),
/// keeping the first seen. Sorted by display name for stable output.
pub fn dedup_by_target(rows: Vec<InstalledTool>) -> Vec<InstalledTool> {
    let mut map: BTreeMap<String, InstalledTool> = BTreeMap::new();
    for row in rows {
        map.entry(row.description.clone()).or_insert(row);
    }
    let mut out: Vec<InstalledTool> = map.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib manual::tests::dedup`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/scan/manual.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): dedup manual rows by resolved target"
```

---

## Task 5: The `scan_manual` orchestrator (sweep, resolve, version, build)

**Files:**
- Modify: `src-tauri/src/scan/manual.rs`

This is the filesystem glue: enumerate PATH, resolve, exclude, version, build rows. Not unit-tested (touches the live filesystem and runs processes); verified live in Task 8 and the milestone-end review.

- [ ] **Step 1: Add the managed-roots builder, the version resolver, and `scan_manual`**

Add to `src-tauri/src/scan/manual.rs` (above the `tests` module). Adjust imports at the top of the file to:

```rust
use super::InstalledTool;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
```

Then add:

```rust
/// Directory prefixes whose contents are owned by a package manager, toolchain,
/// or the OS, and must never be surfaced as manual installs.
fn managed_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/usr/libexec"),
        PathBuf::from("/System"),
        PathBuf::from("/Library/Apple"),
        PathBuf::from("/opt/homebrew"),
        PathBuf::from("/usr/local/Cellar"),
        PathBuf::from("/usr/local/Homebrew"),
    ];
    // Resolved Homebrew prefix, if brew is installed (covers non-standard prefixes).
    if let Ok(out) = Command::new("brew").arg("--prefix").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            roots.push(PathBuf::from(p));
        }
    }
    // Home-relative toolchain / version-manager dirs and the npx cache.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for sub in [".cargo", ".rustup", ".nvm", ".pyenv", ".volta", ".asdf", "go/bin"] {
            roots.push(home.join(sub));
        }
        roots.push(home.join(".npm").join("_npx"));
    }
    roots
}

/// Resolve a tool's version: a token in the resolved filename first (free, no
/// execution), then `<tool> --version`/`-v`/`version` but ONLY when the binary
/// resolves under $HOME (never run system-wide binaries). Empty when unknown.
fn resolve_version(real: &Path, home: Option<&Path>) -> String {
    if let Some(name) = real.file_name().and_then(|n| n.to_str()) {
        if let Some(v) = first_version(name) {
            return v;
        }
    }
    let under_home = match home {
        Some(h) => real.starts_with(h),
        None => false,
    };
    if under_home {
        for arg in ["--version", "-v", "version"] {
            if let Some(out) = run_with_timeout(real, arg, Duration::from_millis(2000)) {
                if let Some(v) = first_version(&out) {
                    return v;
                }
            }
        }
    }
    String::new()
}

/// Run `bin arg`, capturing stdout+stderr, killing it if it exceeds `dur`.
/// Returns the combined output, or None on spawn failure or timeout.
fn run_with_timeout(bin: &Path, arg: &str, dur: Duration) -> Option<String> {
    let mut child = Command::new(bin)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= dur {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s)
}

/// Scan every $PATH directory for executables that no package manager owns,
/// resolving symlinks, excluding managed/app/toolchain/system paths and names
/// already returned by the other scanners, deduped by resolved target.
/// `other_names` is the set of tool names from the npm/brew/pip/npx scans.
pub fn scan_manual(other_names: &BTreeSet<String>) -> Vec<InstalledTool> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let roots = managed_roots();
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut rows: Vec<InstalledTool> = Vec::new();

    for dir in std::env::split_paths(&path) {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let candidate = entry.path();
            // Must be executable (regular file or symlink to one).
            let meta = match std::fs::metadata(&candidate) {
                Ok(m) => m,
                Err(_) => continue, // broken symlink, etc.
            };
            if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
                continue;
            }
            let real = match std::fs::canonicalize(&candidate) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let basename = match candidate.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if is_managed(&real, &basename, &roots, other_names) {
                continue;
            }
            if !seen.insert(real.clone()) {
                continue; // already processed this target
            }
            let version = resolve_version(&real, home.as_deref());
            let size = super::size::human_size(super::size::dir_size(&real));
            rows.push(InstalledTool {
                name: basename.clone(),
                eco: "manual".to_string(),
                pkg: basename,
                installed: Some(version.clone()),
                latest: version,
                size,
                pinned: false,
                publisher: "local".to_string(),
                description: real.to_string_lossy().into_owned(),
                updated: super::path_mtime(&real),
                requested: true,
            });
        }
    }
    dedup_by_target(rows)
}
```

Note: `super::size::dir_size` on a single file path returns that file's size via its existing walk (a file is its own leaf); this is the on-disk footprint of the binary. That is the intended best-effort size.

- [ ] **Step 2: Build to verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles clean (warnings about previously-unused imports now resolved).

- [ ] **Step 3: Run the full manual test module**

Run: `cd src-tauri && cargo test --lib manual`
Expected: PASS (the four pure-function tests still green).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/scan/manual.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): scan_manual PATH sweep with resolve/exclude/version"
```

---

## Task 6: Wire `scan_manual` into `scan_all`

**Files:**
- Modify: `src-tauri/src/scan/mod.rs:61-71`

- [ ] **Step 1: Gate the source and feed it the other scanners' names**

Replace the body of `scan_all` (lines 61-71) with:

```rust
pub fn scan_all(pins: &std::collections::BTreeSet<String>, sources: Sources) -> Vec<InstalledTool> {
    let mut all = Vec::new();
    if sources.npm { all.extend(npm::scan_npm()); }
    if sources.brew { all.extend(brew::scan_brew()); }
    if sources.pip { all.extend(pip::scan_pip()); }
    if sources.npx { all.extend(npx::scan_npx()); }
    if sources.manual {
        let other_names: std::collections::BTreeSet<String> =
            all.iter().map(|t| t.name.clone()).collect();
        all.extend(manual::scan_manual(&other_names));
    }
    for row in all.iter_mut() {
        row.pinned = pins.contains(&row.pkg);
    }
    all
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles clean. (No change needed in `lib.rs`: `scan_all`'s signature is unchanged; `other_names` is built internally.)

- [ ] **Step 3: Run the whole library test suite**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS (all existing tests plus the new manual tests; count goes up from 62).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/scan/mod.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): gate and wire manual source into scan_all"
```

---

## Task 7: Backend `reveal_in_finder` command

**Files:**
- Modify: `src-tauri/src/lib.rs` (add command near the other thin commands; register in `generate_handler!` at line 160)

- [ ] **Step 1: Add the command**

Add this function in `src-tauri/src/lib.rs` near `open_external` / `open_data_dir`:

```rust
/// Reveal and select a path in Finder (`open -R`). Validates the path exists so
/// a stale entry never shells an arbitrary string. No-op on a missing path.
#[tauri::command]
fn reveal_in_finder(path: String) {
    let p = std::path::Path::new(&path);
    if p.exists() {
        let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
    }
}
```

- [ ] **Step 2: Register it in the handler list**

In `src-tauri/src/lib.rs:160`, add `reveal_in_finder` to the `tauri::generate_handler![...]` list (e.g. after `export_library`):

```rust
    .invoke_handler(tauri::generate_handler![scan_installed, set_pin, get_history, run_op, search_registry, get_whats_new, get_changelog, get_advisory, open_data_dir, open_external, clear_caches, get_settings, set_settings, export_library, reveal_in_finder])
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): reveal_in_finder command"
```

---

## No change needed: What's New / OSV exclusion

The spec requires manual rows to be excluded from the OSV security scan and the
verdict feed, like brew. This is already true with NO code change, verified
against the current code:

- `osv::osv_ecosystem` maps only `npm`/`npx` -> `npm` and `pip` -> `PyPI`;
  every other eco (including `manual` and `brew`) returns `None`. In
  `osv::scan_security` the eligible-tools filter does `let eco =
  osv_ecosystem(&t.eco)?;`, so manual tools are dropped from the OSV query.
- The frontend `verdictScope()` filters `statusOf(t)==="update"`. Manual rows are
  `"unmanaged"`, so they are never in the verdict scope sent to `get_whats_new`.
- Defensively, `release::release_age`'s catch-all arm returns
  `("unknown", "")` for any unrecognized eco.

Do not add manual handling to the intel layer. Task 12 verifies no manual cards
appear in What's New.

## Task 8: Frontend - honest "unmanaged" status and row rendering

**Files:**
- Modify: `frontend/index.html` (then mirror to `prototype/napm-prototype.html`)

All edits are to `frontend/index.html`. There is no test runner for the frontend; verification is live (`npm run tauri dev`) plus the milestone-end review.

- [ ] **Step 1: `statusOf` checks manual FIRST**

At `frontend/index.html:314`, replace:

```js
  function statusOf(t){ if(!t.installed) return "offline"; if(t.installed===t.latest) return "current"; return verCmp(t.latest,t.installed)<0?"current":"update"; }
```

with:

```js
  function statusOf(t){ if(t.eco==="manual") return "unmanaged"; if(!t.installed) return "offline"; if(t.installed===t.latest) return "current"; return verCmp(t.latest,t.installed)<0?"current":"update"; }
```

- [ ] **Step 2: Add the `unmanaged` glyph and status rank**

At `frontend/index.html:326`, replace the `GLYPH` map:

```js
  var GLYPH={update:["↑","g-up"],current:["✓","g-ok"],offline:["✗","g-off"],unmanaged:["?","g-man"]};
```

At `frontend/index.html:336`, replace `statusRank`:

```js
  function statusRank(t){ var s=statusOf(t); return s==="update"?0:s==="offline"?1:s==="unmanaged"?3:2; }
```

- [ ] **Step 3: Add the manual color var and CSS**

At `frontend/index.html:16`, add a `--manual` gray to the eco color vars:

```css
    --npm:#cb3837; --brew:#d07000; --pip:#2f6690; --npx:#6f42c1; --manual:#7a7a7a;
```

At `frontend/index.html:110`, extend the `.src` eco-badge rule:

```css
  .src.npm{background:var(--npm);} .src.brew{background:var(--brew);} .src.pip{background:var(--pip);} .src.npx{background:var(--npx);} .src.manual{background:var(--manual);}
```

At `frontend/index.html:95`, add the `g-man` glyph color next to the others:

```css
  .g-up{color:var(--amber);} .g-ok{color:var(--green);} .g-off{color:var(--dgray);} .g-man{color:var(--dgray);}
```

At `frontend/index.html:97`, add `g-man` to the selected-row white override:

```css
  tr.sel .g-up,tr.sel .g-ok,tr.sel .g-off,tr.sel .g-safe,tr.sel .g-hold,tr.sel .g-man{color:#fff;}
```

- [ ] **Step 4: Render manual rows in `renderRows`**

In the `display.forEach` block (`frontend/index.html:365-389`), make these edits:

After line 367 (`var npx = t.eco==="npx";`) add:

```js
      var manual = t.eco==="manual";
```

Replace the glyph line (372):

```js
      var g = manual?["?","g-man"] : npx?["♪","g-off"] : st==="update" ? (safe?["↑","g-safe"]:["↑","g-hold"]) : GLYPH[st];
```

Replace the gTitle line (373):

```js
      var gTitle = manual?"unmanaged (no package manager owns this)" : (st==="update"&&!npx) ? kind+" update"+(held?" - above your appetite":"") : "";
```

Replace the action line (376-378):

```js
      var action = (manual||npx) ? '<span class="muted">—</span>'
                 : st==="update" ? '<button class="btn rowbtn" data-get="'+i+'">Get</button>'
                 : off ? '<button class="btn rowbtn" data-get="'+i+'">Install</button>' : '<span class="muted">—</span>';
```

Replace the latest-version cell (384) so manual shows the same neutral placeholder npx uses:

```js
        '<td class="'+((npx||manual)?'muted':safe?'vernew':held?'verhold':'muted')+'">'+((npx||manual)?"—":esc(t.latest))+'</td>'+
```

(The installed-version cell at 383 already uses `t.installed||"—"`, which renders the best-effort version or a placeholder for manual rows. No change there. `safe`/`held`/`kind` stay false/"none" for manual because `st` is "unmanaged", so manual rows are automatically out of Update All, `safeCount`, and the outdated count, all of which gate on `statusOf(t)==="update"`.)

- [ ] **Step 5: Mirror to the prototype**

Run: `cp frontend/index.html prototype/napm-prototype.html`

- [ ] **Step 6: Commit**

```bash
git add frontend/index.html prototype/napm-prototype.html
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): unmanaged status + manual row rendering"
```

---

## Task 9: Frontend - manual source toggle (View menu + VIEW state)

**Files:**
- Modify: `frontend/index.html` (then mirror)

- [ ] **Step 1: Add `manual` to the VIEW.sources default and the localStorage merge**

At `frontend/index.html:329`, add `manual:true`:

```js
  var VIEW = { requested:false, outdated:false, sources:{npm:true,brew:true,pip:true,npx:true,manual:true}, sort:"name", desc:true };
```

At `frontend/index.html:330`, add `manual:true` to the merge default:

```js
  try{ var sv=JSON.parse(localStorage.getItem("napm.view")); if(sv){ VIEW=Object.assign(VIEW, sv); VIEW.sources=Object.assign({npm:true,brew:true,pip:true,npx:true,manual:true}, sv.sources||{}); } }catch(e){}
```

- [ ] **Step 2: Add the View menu toggle**

In the View menu source-toggle block (`frontend/index.html:915`, the `Source: npx` line), add a manual toggle right after it:

```js
      {label:"Source: npx", checked:function(){return VIEW.sources.npx;}, run:function(){toggleSource("npx");}},
      {label:"Source: manual", checked:function(){return VIEW.sources.manual;}, run:function(){toggleSource("manual");}},
```

(The library filter at line 356 already reads `if(!VIEW.sources[t.eco]) return false;`, so `manual` is honored automatically once it exists in `VIEW.sources`.)

- [ ] **Step 3: Mirror and commit**

```bash
cp frontend/index.html prototype/napm-prototype.html
git add frontend/index.html prototype/napm-prototype.html
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): manual source toggle in View menu"
```

---

## Task 10: Frontend - manual-only right-click menu

**Files:**
- Modify: `frontend/index.html` (then mirror)

- [ ] **Step 1: Add a `revealPath` helper**

After the `openExt` helper (`frontend/index.html:607`), add:

```js
  function revealPath(p){ var i=inv(); if(i && p) i("reveal_in_finder",{path:p}); }
```

- [ ] **Step 2: Short-circuit `libMenu` for manual rows**

In `libMenu` (`frontend/index.html:761`), immediately after `var t=TOOLS[i]; if(!t) return;` (line 762), insert the manual branch before the package-manager logic:

```js
    if(t.eco==="manual"){
      openPopup([
        {label:"Reveal in Finder", disabled:function(){ return !t.description; }, run:function(){ revealPath(t.description); }},
        {label:"Copy path", disabled:function(){ return !t.description; }, run:function(){ copyText(t.description); }},
        {label:"Copy name", run:function(){ copyText(t.name); }},
        {sep:true},
        {label:"Copy version", disabled:function(){ return !t.installed; }, run:function(){ copyText(t.installed); }}
      ], x, y);
      return;
    }
```

- [ ] **Step 3: Mirror and commit**

```bash
cp frontend/index.html prototype/napm-prototype.html
git add frontend/index.html prototype/napm-prototype.html
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): manual-only right-click menu (filesystem actions)"
```

---

## Task 11: Frontend - manual source in Preferences

**Files:**
- Modify: `frontend/index.html` (then mirror)

- [ ] **Step 1: Add manual to the Preferences source list and defaults**

In `renderPrefs` (`frontend/index.html:987-989`), include `manual` in the default and the rendered rows:

```js
  function renderPrefs(s){
    s=s||{}; var src=s.sources||{npm:true,brew:true,pip:true,npx:true,manual:true};
    var rows=["npm","brew","pip","npx","manual"].map(function(k){
      return '<label style="display:block;margin-top:2px"><input type="checkbox" id="prefSrc_'+k+'"'+(src[k]!==false?' checked':'')+'> '+k+'</label>';
    }).join("");
```

- [ ] **Step 2: Include manual when saving**

In the `data-prefs-save` handler (`frontend/index.html:1024-1027`), add the manual checkbox to the `sources` object:

```js
        sources:{
          npm:document.getElementById("prefSrc_npm").checked,
          brew:document.getElementById("prefSrc_brew").checked,
          pip:document.getElementById("prefSrc_pip").checked,
          npx:document.getElementById("prefSrc_npx").checked,
          manual:document.getElementById("prefSrc_manual").checked
        }
```

- [ ] **Step 3: Update the About copy to mention manual installs**

In `showAbout` (`frontend/index.html:1013`), update the description so it is accurate (no em dashes):

```js
      '<div style="margin-top:8px">npstr AI Package Manager. Tracks the CLI tools you have across npm, brew, pip, npx, and manual installs; tells you what is out of date and whether it is safe to take.</div>'+
```

- [ ] **Step 4: Mirror and commit**

```bash
cp frontend/index.html prototype/napm-prototype.html
git add frontend/index.html prototype/napm-prototype.html
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m9): manual source in Preferences + About copy"
```

---

## Task 12: Live verification

**Files:** none (manual run)

- [ ] **Step 1: Launch the app**

Run: `source "$HOME/.cargo/env" && npm run tauri dev`

- [ ] **Step 2: Verify the library**

Confirm:
- `grok` appears as a single row with the `manual` gray badge, version `0.2.14`, an "unmanaged" gray "?" status glyph, and a "-" placeholder in the latest column.
- `agy` appears as a manual row (version best-effort, possibly blank).
- Homebrew tools, Docker (`.app`), and cargo/rustup binaries do NOT appear as manual rows.
- No npm/pip/npx tool is duplicated into the manual source.

- [ ] **Step 3: Verify actions and toggles**

- Right-click a manual row: Reveal in Finder opens the containing folder with the binary selected; Copy path / Copy name / Copy version put the right text on the clipboard. There is no Get/Update/Roll back.
- View menu -> Source: manual hides/shows the manual rows.
- Edit -> Preferences: the manual checkbox is present, defaults checked; unchecking it and saving rescans with no manual rows; rechecking restores them.
- What's New shows no cards for manual tools; Search has no manual chip.

- [ ] **Step 4: Update the roadmap**

Mark M9 done in `docs/ROADMAP.md`: move the "## Next: M9 ..." heading to a "## Done: M9 ..." entry summarizing what shipped (broad PATH sweep + exclusion, hybrid versioning, unmanaged status, filesystem-only actions), matching the style of the other Done entries. Then commit:

```bash
git add docs/ROADMAP.md
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "docs: mark M9 (manual installs) done"
```

---

## Milestone-end review (after Task 12)

Per the standing convention ([milestone-end-review]), before declaring M9 complete, dispatch adversarial bug-hunt subagents over the assembled feature. Focus areas:

- **Exclusion false positives/negatives:** does any genuinely-managed binary slip through as "manual" (e.g. a tool installed under a non-standard prefix, a pyenv shim, a Go binary), and does any genuinely-manual tool get wrongly excluded by a too-broad root or a basename collision with a package-manager tool of the same name?
- **Version fabrication:** can `resolve_version` ever return a misleading version (e.g. a path component like `python3.12` bleeding into the version, a `--version` run on a system binary that slipped the home check)?
- **Process safety:** does `run_with_timeout` ever leak a child or hang the scan; is the home-dir guard airtight so no system-wide binary is executed?
- **Frontend honesty:** does any manual row ever render green "current", appear in Update All / safeCount / the outdated count, or expose a package-manager action?
- **Dedup correctness:** distinct binaries sharing a directory stay separate; the same binary reached multiple ways collapses to one.

Fix findings, then mark M9 done.
