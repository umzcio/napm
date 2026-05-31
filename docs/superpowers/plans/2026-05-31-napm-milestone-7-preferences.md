# napm M7 - Preferences / Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A persisted settings store + Win98 Preferences dialog (GitHub token, source enable/disable), the settings wired into scan/search/intel, and an Export library action.

**Architecture:** Extend the existing JSON `Store` with `settings.json`; two commands plus an export command; a token helper in `intel` that reads the stored token (env fallback); a `Sources` flag struct threaded into `scan_all`/`search_all`; a Preferences modal reusing the M6 modal chrome.

**Tech Stack:** Rust, Tauri v2, vanilla-JS. macOS `open` via std::process; no new plugins.

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-7-preferences.md`

## Conventions for every task

- `source "$HOME/.cargo/env"` before cargo. Tests: `cd /Users/zach/Documents/GitHub/napm/src-tauri && cargo test --lib` (currently 59). Build: `cargo build`.
- NO em dashes in code/comments/UI strings. Never "Napster" (brand "npstr"). Keep the late-90s look.
- Follow the existing `store.rs`/`scan/`/frontend vanilla style.
- After any frontend task: `cp frontend/index.html prototype/napm-prototype.html`.
- Commit after each task with the given message.

## File structure

- Modify `src-tauri/src/store.rs` - `Settings`/`Sources` + methods (Task 1).
- Modify `src-tauri/src/lib.rs` - get/set_settings, export_library, pass sources (Tasks 1, 2).
- Modify `src-tauri/src/intel/mod.rs` - `github_token` helper (Task 2).
- Modify `src-tauri/src/intel/release.rs`, `src-tauri/src/intel/wire.rs` - use the helper (Task 2).
- Modify `src-tauri/src/scan/mod.rs`, `src-tauri/src/search/mod.rs` - `Sources` param (Task 2).
- Modify `frontend/index.html` - Preferences dialog (Task 3), Export (Task 4).
- Modify `docs/ROADMAP.md` (Task 5).

---

### Task 1: Settings store + get/set/export commands

**Files:** `src-tauri/src/store.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `Sources` and `Settings` to `store.rs`** (near `HistoryEntry`):

```rust
/// Which ecosystems the scan and search cover. Defaults to all enabled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sources {
    pub npm: bool,
    pub brew: bool,
    pub pip: bool,
    pub npx: bool,
}
impl Default for Sources {
    fn default() -> Self { Sources { npm: true, brew: true, pip: true, npx: true } }
}

/// Persisted user settings. Missing or corrupt file reads as defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub github_token: String,
    pub sources: Sources,
}
```

- [ ] **Step 2: Add Store methods** (alongside `pins`/`history`):

```rust
    fn settings_path(&self) -> PathBuf { self.dir.join("settings.json") }

    pub fn settings(&self) -> Settings {
        Self::read_json(&self.settings_path())
    }

    pub fn set_settings(&self, s: &Settings) {
        Self::write_json(&self.settings_path(), s);
    }
```

- [ ] **Step 3: Write failing tests** in the `store.rs` `mod tests`:

```rust
    #[test]
    fn settings_round_trip() {
        let s = temp_store();
        let def = s.settings();
        assert_eq!(def.github_token, "");
        assert!(def.sources.npm && def.sources.brew && def.sources.pip && def.sources.npx);
        s.set_settings(&Settings { github_token: "abc".into(),
            sources: Sources { npm: true, brew: false, pip: true, npx: true } });
        let got = s.settings();
        assert_eq!(got.github_token, "abc");
        assert!(!got.sources.brew);
        assert!(got.sources.npm && got.sources.pip);
    }

    #[test]
    fn corrupt_settings_reads_as_defaults() {
        let s = temp_store();
        std::fs::create_dir_all(&s.dir_for_test()).unwrap();
        std::fs::write(s.dir_for_test().join("settings.json"), b"not json").unwrap();
        let def = s.settings();
        assert_eq!(def.github_token, "");
        assert!(def.sources.brew); // corrupt -> all sources on, no panic
    }
```

- [ ] **Step 4: Run tests, confirm pass.** `cargo test --lib`.

- [ ] **Step 5: Add the three commands to `lib.rs`** and register them:

```rust
#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> store::Settings {
    open_store(&app).settings()
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, settings: store::Settings) {
    open_store(&app).set_settings(&settings);
}

#[tauri::command]
fn export_library(app: tauri::AppHandle, filename: String, content: String) {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    // Sanitize the frontend-supplied filename: no path separators or traversal.
    let safe = filename.replace(['/', '\\'], "_").replace("..", "_");
    let path = dir.join(safe);
    if std::fs::write(&path, content).is_ok() {
        let _ = std::process::Command::new("open").arg(&dir).spawn();
    }
}
```

Add `get_settings, set_settings, export_library` to the `generate_handler!` list.

- [ ] **Step 6: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, 61 tests (two new store tests).

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/store.rs src-tauri/src/lib.rs
git commit -m "feat: settings store with get/set_settings and export_library commands"
```

---

### Task 2: Wire the token and sources through

**Files:** `src-tauri/src/intel/mod.rs`, `src-tauri/src/intel/release.rs`, `src-tauri/src/intel/wire.rs`, `src-tauri/src/scan/mod.rs`, `src-tauri/src/search/mod.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the token helper to `intel/mod.rs`.**

```rust
/// The GitHub token for API calls: the stored settings value if present and
/// non-empty, otherwise the GITHUB_TOKEN env var. None when neither is set.
/// Reads settings.json directly from the cache dir (== app-data dir) to avoid a
/// dependency on the store module.
pub fn github_token(cache_dir: &Path) -> Option<String> {
    let from_file = std::fs::read_to_string(cache_dir.join("settings.json")).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("githubToken").and_then(|t| t.as_str()).map(String::from))
        .filter(|t| !t.trim().is_empty());
    from_file.or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty()))
}
```

(`Path` is already imported in `intel/mod.rs`.)

- [ ] **Step 2: Use it in `release.rs` changelog.** Replace the `std::env::var("GITHUB_TOKEN")` block (around line 188) with the helper. `changelog` already has `cache_dir: &Path` in scope:

```rust
        let token_str: String;
        let token_header: String;
        if let Some(token) = super::github_token(cache_dir) {
            token_str = token;
            token_header = format!("Bearer {}", token_str);
            headers.push(("Authorization", &token_header));
        }
```

- [ ] **Step 3: Use it in `wire.rs` fetch_wire.** The wire GitHub-advisories requests currently send only the `Accept` header. Add the `Authorization: Bearer <token>` header when `super::github_token(cache_dir)` returns Some, using `crate::http::get_with_headers` (the same helper changelog uses). `fetch_wire` already has `cache_dir`. Build the header strings in bindings that outlive the request (mirror the changelog pattern so references live long enough).

- [ ] **Step 4: Add a `Sources` param to `scan_all`.** In `scan/mod.rs`:

```rust
use crate::store::Sources;

pub fn scan_all(pins: &std::collections::BTreeSet<String>, sources: Sources) -> Vec<InstalledTool> {
    let mut all = Vec::new();
    if sources.npm { all.extend(npm::scan_npm()); }
    if sources.brew { all.extend(brew::scan_brew()); }
    if sources.pip { all.extend(pip::scan_pip()); }
    if sources.npx { all.extend(npx::scan_npx()); }
    for row in all.iter_mut() {
        row.pinned = pins.contains(&row.pkg);
    }
    all
}
```

- [ ] **Step 5: Add a `Sources` param to `search_all`.** In `search/mod.rs`, gate each source (search covers npm/brew/pip; npx is not a search source):

```rust
use crate::store::Sources;

pub fn search_all(query: &str, cache_dir: &Path, sources: Sources) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() { return Vec::new(); }
    std::thread::scope(|s| {
        let n = if sources.npm { Some(s.spawn(|| npm::search_npm(query))) } else { None };
        let b = if sources.brew { Some(s.spawn(|| brew::search_brew(query, cache_dir))) } else { None };
        let p = if sources.pip { Some(s.spawn(|| pip::search_pip(query))) } else { None };
        merge(vec![
            n.map(|h| h.join().unwrap_or_default()).unwrap_or_default(),
            b.map(|h| h.join().unwrap_or_default()).unwrap_or_default(),
            p.map(|h| h.join().unwrap_or_default()).unwrap_or_default(),
        ])
    })
}
```

- [ ] **Step 6: Pass sources from the commands in `lib.rs`.** Update the two commands:

```rust
#[tauri::command]
fn scan_installed(app: tauri::AppHandle) -> Vec<InstalledTool> {
    let store = open_store(&app);
    let pins = store.pins();
    scan::scan_all(&pins, store.settings().sources)
}

#[tauri::command]
fn search_registry(app: tauri::AppHandle, query: String) -> Vec<search::SearchResult> {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let sources = open_store(&app).settings().sources;
    search::search_all(&query, &dir, sources)
}
```

- [ ] **Step 7: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, 61 tests still pass. (Source-skipping shells out, so it is verified live, not unit-tested; the existing parse/merge tests are unaffected by the new params.)

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/intel/ src-tauri/src/scan/mod.rs src-tauri/src/search/mod.rs src-tauri/src/lib.rs
git commit -m "feat: wire stored GitHub token and source toggles into intel, scan, and search"
```

---

### Task 3: Preferences dialog (frontend)

**Files:** `frontend/index.html`; mirror to prototype.

- [ ] **Step 1: Add a Preferences item to the Edit menu.** In the `MENUS.edit` array, add after Copy tool details:

```js
      {sep:true},
      {label:"Preferences...", run:function(){ if(window.showPrefs) showPrefs(); }}
```

- [ ] **Step 2: Add `showPrefs` (global, near `showAbout`).** It loads current settings, renders the form into the shared modal, and shows it:

```js
  window.showPrefs=function(){
    var i=inv(); if(!i) return;
    i("get_settings").then(function(s){
      s=s||{}; var src=s.sources||{npm:true,brew:true,pip:true,npx:true};
      var rows=["npm","brew","pip","npx"].map(function(k){
        return '<label style="display:block;margin-top:2px"><input type="checkbox" id="prefSrc_'+k+'"'+(src[k]!==false?' checked':'')+'> '+k+'</label>';
      }).join("");
      var box=document.getElementById("modalBox");
      box.innerHTML='<div class="m-h">Preferences<span class="x" data-close>×</span></div>'+
        '<div class="m-b">'+
        '<div><b>GitHub token</b><div class="muted" style="font-size:11px">optional - raises the GitHub API rate limit for changelogs and the supply-chain wire.</div>'+
        '<input id="prefToken" class="search" style="width:100%;margin-top:4px" value="'+esc(s.githubToken||"")+'"></div>'+
        '<div style="margin-top:10px"><b>Sources</b>'+rows+'</div>'+
        '<div style="margin-top:12px;text-align:right"><button class="btn" data-close>Cancel</button> <button class="btn primary" data-prefs-save>Save</button></div>'+
        '</div>';
      document.getElementById("modalBack").style.display="flex";
    }).catch(function(){});
  };
```

- [ ] **Step 3: Handle Save in the existing `#modalBack` click handler.** Add a branch (alongside the `data-close`/`data-repo` branches):

```js
    if(e.target.hasAttribute("data-prefs-save")){
      var settings={
        githubToken:(document.getElementById("prefToken").value||"").trim(),
        sources:{
          npm:document.getElementById("prefSrc_npm").checked,
          brew:document.getElementById("prefSrc_brew").checked,
          pip:document.getElementById("prefSrc_pip").checked,
          npx:document.getElementById("prefSrc_npx").checked
        }
      };
      var i=inv(); if(i) i("set_settings",{settings:settings});
      closeModal();
      FEED_LOADED=false; scanLibrary(); // rescan with the new sources, refresh the feed
      return;
    }
```

- [ ] **Step 4: Mirror + manual check.** `cp ...`. Human verifies: Edit -> Preferences opens the dialog with the current token + source checkboxes; unchecking a source and Saving rescans and that ecosystem's rows disappear (and reappear when re-enabled); the token persists across relaunch; Cancel/X/Escape/backdrop close without saving.

- [ ] **Step 5: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: Preferences dialog for GitHub token and source toggles"
```

---

### Task 4: Export library (frontend)

**Files:** `frontend/index.html`; mirror.

- [ ] **Step 1: Add the export helper** (near the other menu helpers):

```js
  function exportLibrary(format){
    var i=inv(); if(!i) return;
    var date=new Date().toISOString().slice(0,10);
    var content, fn;
    if(format==="markdown"){
      fn="napm-library-"+date+".md";
      content="# napm library ("+date+")\n\n| Tool | Source | Installed | Latest | Publisher | Size |\n|---|---|---|---|---|---|\n"+
        TOOLS.map(function(t){ return "| "+t.name+" | "+t.eco+" | "+(t.installed||"-")+" | "+t.latest+" | @"+(t.publisher||"unknown")+" | "+(t.size||"-")+" |"; }).join("\n")+"\n";
    } else {
      fn="napm-library-"+date+".json";
      content=JSON.stringify(TOOLS, null, 2);
    }
    i("export_library",{filename:fn, content:content});
  }
```

- [ ] **Step 2: Add the two File-menu items.** In `MENUS.file`, after "Open data folder" and before the separator/Quit:

```js
      {label:"Export library (JSON)", run:function(){ exportLibrary("json"); }},
      {label:"Export library (Markdown)", run:function(){ exportLibrary("markdown"); }},
```

- [ ] **Step 3: Mirror + manual check.** `cp ...`. Human verifies: File -> Export library (JSON / Markdown) writes `napm-library-<date>.{json,md}` and opens the data folder in Finder; the file contents are correct.

- [ ] **Step 4: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: Export library as JSON or Markdown from the File menu"
```

---

### Task 5: Update the roadmap

**Files:** `docs/ROADMAP.md`

- [ ] **Step 1:** Move M7 from "Next" to "Done" with a summary (settings store; Preferences dialog for GitHub token + source enable/disable; token read from settings with env fallback; sources threaded into scan_all/search_all; Export library JSON/Markdown to the data folder). Note carried-forward deferrals (default-appetite stays on the dial, "Save As" picker, Keyboard Shortcuts). Set M8 (Right-click context menus) as the next milestone.

- [ ] **Step 2: Commit.**

```bash
git add docs/ROADMAP.md
git commit -m "docs: mark M7 Preferences done, M8 next"
```

---

## Self-review notes

- Spec coverage: settings store (T1), Preferences dialog with token + sources (T3), token wiring with env fallback (T2 helper + release/wire), sources into scan_all/search_all (T2), Export JSON/Markdown + reveal (T1 command + T4 frontend), corrupt-file defaults (T1 test). Appetite stays on the dial (not added). Save-As picker deferred.
- Type consistency: `Settings`/`Sources` defined once (T1), `Sources` (Copy) threaded into `scan_all`/`search_all` (T2) and read via `store.settings().sources` in the commands (T2). `github_token(cache_dir)` defined T2, used in release/wire. Frontend reads `githubToken`/`sources` (camelCase serde) and posts `set_settings({settings})` matching the command arg name. `export_library({filename, content})` matches.
- No placeholders: every step has concrete code; backend steps have real test assertions where unit-testable.
- No new crates/plugins.
