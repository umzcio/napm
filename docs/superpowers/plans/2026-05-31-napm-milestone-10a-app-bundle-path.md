# napm M10a - Real app bundle + PATH Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a real macOS `.app` that runs from `/Applications`, finds the user's tools when launched from the Dock (PATH fix), shows the npstr icon and correct version, and preserves history across the bundle-identifier change.

**Architecture:** A new `pathenv` module captures the login-shell PATH at the top of `run()` and sets it process-wide before anything spawns. The bundle identifier moves to `com.napm.app`, with a one-time no-clobber migration of `pins/history/settings.json` from the old app-data dir. Metadata and identifier are config edits; the icon is already wired and is verified by a real build.

**Tech Stack:** Rust (Tauri v2 backend, std only), JSON/TOML config.

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-10a-app-bundle-path.md`

---

## CRITICAL conventions (read before any task)

- Run `source "$HOME/.cargo/env"` before any cargo command.
- Backend tests: `cd /Users/zach/Documents/GitHub/napm/src-tauri && cargo test --lib`
- No em dashes in any copy/strings you write.
- Commit author for every commit: `git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit ...`. Do NOT push.
- This crate is edition 2021, so `std::env::set_var` is a safe call (no `unsafe` block).
- No frontend changes in this milestone, so no prototype mirroring is needed.

## File structure

- Create: `src-tauri/src/pathenv.rs` - `extract_path` (pure) + `fix_path()` (shell capture + `set_var`) + a small bounded shell runner.
- Modify: `src-tauri/src/lib.rs` - `mod pathenv;`; call `pathenv::fix_path()` first in `run()`; call `store::migrate_legacy` in `.setup()`.
- Modify: `src-tauri/src/store.rs` - add `migrate_legacy(current, legacy)` + tests.
- Modify: `src-tauri/tauri.conf.json` - identifier `com.napm.app`.
- Modify: `src-tauri/Cargo.toml` - fill description/authors/license/repository.

---

## Task 1: `extract_path` pure parser

**Files:**
- Create: `src-tauri/src/pathenv.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod pathenv;`)

- [ ] **Step 1: Create the file with the failing tests and the parser**

Create `src-tauri/src/pathenv.rs`:

```rust
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const START: &str = "__NAPM_PATH_START__";
const END: &str = "__NAPM_PATH_END__";

/// Pull the PATH from sentinel-delimited shell output. Returns the trimmed
/// substring between the markers, or None if a marker is absent or the content
/// is empty. Robust to rc-file noise printed around the markers.
pub fn extract_path(output: &str) -> Option<String> {
    let start = output.find(START)? + START.len();
    let rest = &output[start..];
    let end = rest.find(END)? + start;
    let p = output[start..end].trim();
    if p.is_empty() {
        None
    } else {
        Some(p.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_clean_path() {
        let out = "__NAPM_PATH_START__/opt/homebrew/bin:/usr/bin__NAPM_PATH_END__";
        assert_eq!(extract_path(out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn extracts_through_rc_noise() {
        let out = "welcome to your shell\nsome banner\n__NAPM_PATH_START__/usr/local/bin:/usr/bin__NAPM_PATH_END__\n";
        assert_eq!(extract_path(out).as_deref(), Some("/usr/local/bin:/usr/bin"));
    }

    #[test]
    fn missing_markers_is_none() {
        assert_eq!(extract_path("no markers here /usr/bin"), None);
        assert_eq!(extract_path("__NAPM_PATH_START__/usr/bin no end"), None);
    }

    #[test]
    fn empty_between_markers_is_none() {
        assert_eq!(extract_path("__NAPM_PATH_START____NAPM_PATH_END__"), None);
        assert_eq!(extract_path("__NAPM_PATH_START__   __NAPM_PATH_END__"), None);
    }
}
```

(Note: `Command`, `Stdio`, `Duration`, `Instant` are imported now but used by Task 2; `cargo test` will warn about unused imports until then, which is fine, not an error.)

- [ ] **Step 2: Register the module**

In `src-tauri/src/lib.rs`, add `mod pathenv;` alongside the other module declarations near the top (after `mod search;`):

```rust
mod pathenv;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib pathenv`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/pathenv.rs src-tauri/src/lib.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10a): sentinel PATH extraction parser"
```

---

## Task 2: `fix_path()` shell capture

**Files:**
- Modify: `src-tauri/src/pathenv.rs`

- [ ] **Step 1: Add the bounded shell runner and `fix_path`**

Append to `src-tauri/src/pathenv.rs` (above the `tests` module):

```rust
/// Capture the user's real login-shell PATH and set it on this process so every
/// child spawned afterward inherits it. A no-op when the shell probe fails (the
/// inherited PATH is left untouched). Call once at the very start of `run()`,
/// before any process is spawned, so Finder/Dock launches can find npm/brew/pip
/// and the manual scanner can walk a real $PATH.
pub fn fix_path() {
    if let Some(p) = capture_login_path() {
        std::env::set_var("PATH", p);
    }
}

/// Run the user's login+interactive shell with a sentinel-delimited probe and
/// extract the PATH it reports. None on any failure. The result must contain a
/// '/' as a minimal sanity check before it is trusted.
fn capture_login_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let probe = "printf '__NAPM_PATH_START__%s__NAPM_PATH_END__' \"$PATH\"";
    let out = run_shell(&shell, &["-ilc", probe], Duration::from_millis(2000))?;
    let p = extract_path(&out)?;
    if p.contains('/') {
        Some(p)
    } else {
        None
    }
}

/// Spawn `shell args...`, capture stdout, and kill it if it exceeds `dur`.
/// stderr is discarded so rc-file warnings never pollute the result.
fn run_shell(shell: &str, args: &[&str], dur: Duration) -> Option<String> {
    let mut child = Command::new(shell)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles clean (unused-import warnings from Task 1 are now resolved).

- [ ] **Step 3: Run the pathenv tests**

Run: `cd src-tauri && cargo test --lib pathenv`
Expected: the 4 parser tests still pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/pathenv.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10a): capture login-shell PATH at startup"
```

---

## Task 3: `migrate_legacy` store migration

**Files:**
- Modify: `src-tauri/src/store.rs`

- [ ] **Step 1: Add the failing tests**

Append to the `tests` module in `src-tauri/src/store.rs`:

```rust
    #[test]
    fn migrate_copies_user_files_when_target_missing() {
        let legacy = temp_store();
        std::fs::write(legacy.dir_for_test().join("history.json"), b"[{\"old\":1}]").unwrap();
        std::fs::write(legacy.dir_for_test().join("pins.json"), b"[\"typescript\"]").unwrap();
        // a cache file that must NOT be migrated
        std::fs::write(legacy.dir_for_test().join("wire.json"), b"{}").unwrap();

        let mut current = std::env::temp_dir();
        current.push(format!("napm-test-cur-{:p}", &legacy));
        let _ = std::fs::remove_dir_all(&current);

        migrate_legacy(&current, &legacy.dir_for_test());

        assert_eq!(std::fs::read(current.join("history.json")).unwrap(), b"[{\"old\":1}]");
        assert!(current.join("pins.json").exists());
        assert!(!current.join("wire.json").exists()); // caches not migrated
    }

    #[test]
    fn migrate_never_clobbers_existing() {
        let legacy = temp_store();
        std::fs::write(legacy.dir_for_test().join("history.json"), b"LEGACY").unwrap();

        let mut current = std::env::temp_dir();
        current.push(format!("napm-test-cur2-{:p}", &legacy));
        let _ = std::fs::remove_dir_all(&current);
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("history.json"), b"CURRENT").unwrap();

        migrate_legacy(&current, &legacy.dir_for_test());

        assert_eq!(std::fs::read(current.join("history.json")).unwrap(), b"CURRENT");
    }

    #[test]
    fn migrate_absent_legacy_is_noop() {
        let mut legacy = std::env::temp_dir();
        legacy.push(format!("napm-test-missing-{:p}", &legacy));
        let _ = std::fs::remove_dir_all(&legacy);
        let mut current = std::env::temp_dir();
        current.push(format!("napm-test-cur3-{:p}", &current));
        let _ = std::fs::remove_dir_all(&current);

        migrate_legacy(&current, &legacy); // must not panic
        assert!(!current.join("history.json").exists());
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --lib store::tests::migrate`
Expected: FAIL to compile (`migrate_legacy` not defined).

- [ ] **Step 3: Implement `migrate_legacy`**

Add this free function to `src-tauri/src/store.rs` (outside the `Store` impl, e.g. just below it; ensure `use std::path::Path;` is available - the file already imports `std::path::{Path, PathBuf}`):

```rust
/// One-time, best-effort migration of user-data files from a legacy app-data
/// directory into the current one. Only `pins/history/settings.json` are copied,
/// and only when the target does not already exist (never clobber newer data).
/// Caches are intentionally skipped (they regenerate). No-op if the legacy dir
/// is absent. Any IO error is ignored so this never blocks startup.
pub fn migrate_legacy(current_dir: &Path, legacy_dir: &Path) {
    if !legacy_dir.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(current_dir);
    for f in ["pins.json", "history.json", "settings.json"] {
        let dst = current_dir.join(f);
        let src = legacy_dir.join(f);
        if !dst.exists() && src.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib store::tests::migrate`
Expected: 3 migrate tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/store.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10a): one-time no-clobber app-data migration"
```

---

## Task 4: Wire PATH fix and migration into the app lifecycle

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Call `fix_path()` first in `run()`**

In `src-tauri/src/lib.rs`, in `pub fn run()`, make `pathenv::fix_path();` the very first statement, before `tauri::Builder::default()`:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // Capture the real login-shell PATH before anything spawns, so a Dock/Finder
  // launch can find npm/brew/pip and the manual scanner can walk a real $PATH.
  pathenv::fix_path();
  tauri::Builder::default()
```

- [ ] **Step 2: Call the migration in `.setup()` before the brew warm-up**

In the `.setup()` closure, the existing code computes `dir` (the app-data dir) and moves it into the warm-up thread. Insert the migration after `dir` is computed and BEFORE the `std::thread::spawn` that moves it. The block currently reads:

```rust
      let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
      std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        search::brew::warm_brew(&dir);
      });
```

Change it to:

```rust
      let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
      // One-time migration from the pre-rename app-data dir (com.tauri.dev).
      if let Some(parent) = dir.parent() {
        store::migrate_legacy(&dir, &parent.join("com.tauri.dev"));
      }
      std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        search::brew::warm_brew(&dir);
      });
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles clean.

- [ ] **Step 4: Run the full library test suite**

Run: `cd src-tauri && cargo test --lib`
Expected: all tests pass (66 prior + 4 pathenv + 3 migrate = 73).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10a): apply PATH fix at startup and migrate legacy app-data"
```

---

## Task 5: Bundle identifier

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Change the identifier**

In `src-tauri/tauri.conf.json`, change:

```json
  "identifier": "com.tauri.dev",
```

to:

```json
  "identifier": "com.napm.app",
```

- [ ] **Step 2: Verify the config still parses and the app builds**

Run: `cd src-tauri && cargo build`
Expected: compiles clean (the build script reads `tauri.conf.json`; a malformed change would fail here).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10a): bundle identifier com.napm.app"
```

---

## Task 6: Crate metadata

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Fill the placeholder metadata**

In `src-tauri/Cargo.toml`, replace the `[package]` placeholder fields:

```toml
description = "A Tauri App"
authors = ["you"]
license = ""
repository = ""
```

with:

```toml
description = "napm: a desktop package manager for command-line dev tools"
authors = ["umzcio <umzcio@users.noreply.github.com>"]
license = "MIT"
repository = "https://github.com/umzcio/napm"
```

(Leave `name = "app"`, `version`, `edition`, `rust-version`, and the `[lib]` block unchanged.)

- [ ] **Step 2: Verify it still builds**

Run: `cd src-tauri && cargo build`
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10a): fill crate metadata"
```

---

## Task 7: Live verification (packaged build)

**Files:** none (manual build + run)

- [ ] **Step 1: Build the bundle**

Run: `source "$HOME/.cargo/env" && cd /Users/zach/Documents/GitHub/napm && npm run tauri build`
Expected: a release build producing `napm.app` (and a `.dmg`) under `src-tauri/target/release/bundle/`. The build may take several minutes. Report the bundle path.

- [ ] **Step 2: Install and launch from Finder (not a terminal)**

Copy `napm.app` to `/Applications`, then launch it from Finder/Dock (right-click -> Open the first time to pass Gatekeeper, since it is unsigned until M10b).

- [ ] **Step 3: Verify the PATH fix and the app**

Confirm:
- The Shared Library populates with npm/brew/pip/npx/manual tools (this proves the login-shell PATH was captured; a regression would show an empty or near-empty library).
- Search returns results and a real install/update runs (Transfers streams output, exit code shown).
- The npstr logo shows in the Dock, in Finder Get-Info, and in Help -> About napm.
- The titlebar reads `napm v0.1.0`.
- History from before the rename is present (migration worked). If the pre-rename `~/Library/Application Support/com.tauri.dev/history.json` existed, its entries appear in Transfers history under the new `com.napm.app` dir.

- [ ] **Step 4: Mark M10a done in the roadmap**

In `docs/ROADMAP.md`, change the "### M10a - Real app bundle + PATH" heading to "### M10a - Real app bundle + PATH (Done)" and add a one-line summary of what shipped, matching the style of the other Done entries. Commit:

```bash
git add docs/ROADMAP.md
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "docs: mark M10a (app bundle + PATH) done"
```

---

## Milestone-end review (after Task 7)

Per the standing convention, before declaring M10a complete, dispatch adversarial bug-hunt subagents. Focus areas:

- **PATH capture safety:** can `fix_path` ever hang startup (timeout airtight?), set an empty/garbage PATH, or drop the inherited PATH on a normal machine? Does the `-ilc` probe behave on zsh and bash? Is the `set_var` placement truly before any spawn (including the brew warm-up and any plugin)?
- **Migration correctness:** can `migrate_legacy` clobber newer data, copy caches, panic on a missing/unreadable legacy dir, or run after the user already has data under the new identifier?
- **Identifier change fallout:** does anything else key off the old identifier (capabilities, logs, deep links)? Does the data-dir move lose anything beyond the three migrated files that we should also carry?
- **Build/bundle:** does `tauri build` actually embed the npstr icon and the correct version end to end?

Fix findings, then mark M10a done.
