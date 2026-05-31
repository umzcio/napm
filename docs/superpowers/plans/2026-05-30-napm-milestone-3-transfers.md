# napm Milestone 3: Transfers (real execution, history, rollback)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Get / Update All / Roll back buttons actually change versions: run the real install/update/rollback command, stream its output live into the Transfers row, show the honest exit code, and persist a history of every change plus the user's pins.

**Architecture:** Two new Rust modules. `store.rs` persists pins and history as flat JSON in the app-data dir (clean interface so SQLite can swap in later). `ops.rs` builds the per-ecosystem command (pure, tested) and runs it on a background thread, streaming stdout/stderr to the frontend via Tauri events and logging history on success. The frontend replaces its fake progress bar with the real streamed output and loads history/pins from the backend.

**Tech Stack:** Rust, `serde`/`serde_json`, `std::process::Command` (piped streaming), Tauri v2 events (`Emitter`) and paths (`Manager`), vanilla-JS event listeners via `window.__TAURI__`.

**Scope note:** Milestone 3 of the roadmap (`docs/ROADMAP.md`). It makes Transfers, pins, and Update All real. Rollback works for npm and pip; brew is gated (Homebrew keeps no old bottles); npx offers Promote-to-global. Out of scope: Search (M4), What's New (M5), the menu bar (M6).

**Decision — store backend:** flat JSON files (`pins.json`, `history.json`) in the platform app-data dir. The spec sanctions JSON as the simpler start; the trivial data does not need SQL. `store.rs` exposes a small typed interface so a SQLite backend can replace the file I/O later without touching callers.

**Environment facts (verified):** Tauri v2.11, `withGlobalTauri: true` (so `window.__TAURI__.core.invoke` / `.event.listen` work). The backend already has the `scan` module returning `InstalledTool { ..., pinned: bool }` (pins currently always false). pip binary is `pip3`.

---

## File structure after this milestone

```
src-tauri/src/
├─ lib.rs        (MODIFIED: register pin/unpin/get_history/run_op commands; mark pins on scan)
├─ store.rs      (NEW: JSON persistence for pins + history; HistoryEntry type)
├─ ops.rs        (NEW: build_command (pure) + run_op streaming executor)
└─ scan/...      (MODIFIED: scan_all takes a pin set to mark rows)
frontend/index.html  (MODIFIED: real transfers via run_op + events; persistent history; real pins; rollback; Update All)
prototype/napm-prototype.html (mirror)
```

---

## Task 1: Persistence store (pins + history)

**Files:**
- Create: `src-tauri/src/store.rs`

The store reads/writes two JSON files in a directory it is given (the app-data dir at runtime; a temp dir in tests). Tests use a unique temp directory so they are deterministic and isolated.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/store.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// implementation added in Step 3

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> Store {
        // unique dir per test process+address; no Date/rand needed
        let mut dir = std::env::temp_dir();
        dir.push(format!("napm-test-{:p}", &dir));
        let _ = std::fs::remove_dir_all(&dir);
        Store::new(dir)
    }

    #[test]
    fn pins_round_trip_and_dedupe() {
        let s = temp_store();
        assert!(s.pins().is_empty());
        s.set_pin("typescript", true);
        s.set_pin("typescript", true); // idempotent
        s.set_pin("eslint", true);
        let pins = s.pins();
        assert!(pins.contains("typescript") && pins.contains("eslint") && pins.len() == 2);
        s.set_pin("typescript", false);
        assert!(!s.pins().contains("typescript"));
    }

    #[test]
    fn history_appends_newest_first() {
        let s = temp_store();
        assert!(s.history().is_empty());
        s.add_history(HistoryEntry { ts: 1, pkg: "a".into(), eco: "npm".into(), action: "install".into(), from: None, to: "1.0".into() });
        s.add_history(HistoryEntry { ts: 2, pkg: "b".into(), eco: "npm".into(), action: "update".into(), from: Some("1.0".into()), to: "2.0".into() });
        let h = s.history();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].pkg, "b"); // newest first
        assert_eq!(h[1].pkg, "a");
    }

    #[test]
    fn missing_or_corrupt_files_read_as_empty() {
        let s = temp_store();
        std::fs::create_dir_all(&s_dir(&s)).unwrap();
        std::fs::write(s_dir(&s).join("pins.json"), b"not json").unwrap();
        assert!(s.pins().is_empty()); // corrupt -> empty, no panic
    }

    fn s_dir(s: &Store) -> PathBuf { s.dir_for_test() }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib store 2>&1 | tail -10
```
Expected: compile error, `cannot find type 'Store'`.

- [ ] **Step 3: Implement the store**

Add above the `#[cfg(test)]` block in `src-tauri/src/store.rs`:

```rust
/// One logged version change. Mirrors the prototype's HistoryEntry, plus `eco`
/// so rollback can rebuild the command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts: i64,
    pub pkg: String,
    pub eco: String,
    pub action: String, // "install" | "update" | "rollback"
    pub from: Option<String>,
    pub to: String,
}

/// Flat-JSON persistence for pins and history in a single directory.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        let _ = std::fs::create_dir_all(&dir);
        Store { dir }
    }

    fn pins_path(&self) -> PathBuf { self.dir.join("pins.json") }
    fn history_path(&self) -> PathBuf { self.dir.join("history.json") }

    fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write_json<T: Serialize>(path: &Path, value: &T) {
        if let Ok(s) = serde_json::to_string_pretty(value) {
            let _ = std::fs::write(path, s);
        }
    }

    pub fn pins(&self) -> BTreeSet<String> {
        Self::read_json(&self.pins_path())
    }

    pub fn set_pin(&self, pkg: &str, on: bool) {
        let mut pins = self.pins();
        if on {
            pins.insert(pkg.to_string());
        } else {
            pins.remove(pkg);
        }
        Self::write_json(&self.pins_path(), &pins);
    }

    /// History newest-first.
    pub fn history(&self) -> Vec<HistoryEntry> {
        let mut h: Vec<HistoryEntry> = Self::read_json(&self.history_path());
        h.sort_by(|a, b| b.ts.cmp(&a.ts));
        h
    }

    pub fn add_history(&self, entry: HistoryEntry) {
        let mut h: Vec<HistoryEntry> = Self::read_json(&self.history_path());
        h.push(entry);
        Self::write_json(&self.history_path(), &h);
    }

    #[cfg(test)]
    pub fn dir_for_test(&self) -> PathBuf {
        self.dir.clone()
    }
}
```

- [ ] **Step 4: Register the module and run the tests**

In `src-tauri/src/lib.rs`, add `mod store;` near the top (next to `mod scan;`). Then:

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib store 2>&1 | tail -8
```
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/store.rs src-tauri/src/lib.rs
git commit -m "feat: JSON persistence store for pins and history"
```

---

## Task 2: Wire pins + history into the backend

**Files:**
- Modify: `src-tauri/src/scan/mod.rs` (mark pinned rows from a pin set)
- Modify: `src-tauri/src/lib.rs` (commands + app-data dir helper)

- [ ] **Step 1: Make scan_all accept a pin set**

In `src-tauri/src/scan/mod.rs`, change `scan_all` to take pins and mark rows. Replace the existing `scan_all` with:

```rust
/// Aggregate across all sources, marking rows whose pkg is in `pins`.
pub fn scan_all(pins: &std::collections::BTreeSet<String>) -> Vec<InstalledTool> {
    let mut all = Vec::new();
    all.extend(npm::scan_npm());
    all.extend(brew::scan_brew());
    all.extend(pip::scan_pip());
    all.extend(npx::scan_npx());
    for row in all.iter_mut() {
        row.pinned = pins.contains(&row.pkg);
    }
    all
}
```

- [ ] **Step 2: Add an app-data dir helper and the commands in lib.rs**

In `src-tauri/src/lib.rs`, add the imports and a helper to build a `Store` from the app's data dir, then the commands. The `scan_installed` command changes to pass pins. Full additions:

```rust
mod scan;
mod store;
mod ops;

use scan::InstalledTool;
use store::{HistoryEntry, Store};
use tauri::Manager;

/// Open the Store rooted at the platform app-data directory.
fn open_store(app: &tauri::AppHandle) -> Store {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    Store::new(dir)
}

#[tauri::command]
fn scan_installed(app: tauri::AppHandle) -> Vec<InstalledTool> {
    let pins = open_store(&app).pins();
    scan::scan_all(&pins)
}

#[tauri::command]
fn set_pin(app: tauri::AppHandle, pkg: String, pinned: bool) {
    open_store(&app).set_pin(&pkg, pinned);
}

#[tauri::command]
fn get_history(app: tauri::AppHandle) -> Vec<HistoryEntry> {
    open_store(&app).history()
}
```

- [ ] **Step 3: Register the commands**

In `src-tauri/src/lib.rs`, update the `generate_handler!` to include the new commands (and `run_op` from Task 4, which you will add then). For now:

```rust
.invoke_handler(tauri::generate_handler![scan_installed, set_pin, get_history])
```

- [ ] **Step 4: Build and run tests**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo build 2>&1 | tail -8 && cargo test --lib 2>&1 | tail -4
```
Expected: clean build; all existing tests still pass. (`ops` is referenced by `mod ops;` but not created until Task 3 — if the build fails on the missing module, create an empty `src-tauri/src/ops.rs` with a single line `// implemented in Task 3` now, then continue. It will be filled in Tasks 3-4.)

- [ ] **Step 5: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/scan/mod.rs src-tauri/src/lib.rs src-tauri/src/ops.rs
git commit -m "feat: persist pins, expose pin/history commands, mark pinned on scan"
```

---

## Task 3: Command builder (TDD)

**Files:**
- Modify: `src-tauri/src/ops.rs`

The pure, testable core of execution: given an action, produce the exact program + args. Rollback for brew is intentionally unsupported (returns None).

- [ ] **Step 1: Write the failing tests**

Replace the contents of `src-tauri/src/ops.rs` with the tests first:

```rust
// implementation added in Step 3

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_install_pins_version() {
        let (prog, args) = build_command("npm", "typescript", "5.6.2", "update", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "typescript@5.6.2"]);
    }

    #[test]
    fn pip_uses_double_equals_and_given_binary() {
        let (prog, args) = build_command("pip", "httpie", "3.2.2", "rollback", "pip3").unwrap();
        assert_eq!(prog, "pip3");
        assert_eq!(args, vec!["install", "httpie==3.2.2"]);
    }

    #[test]
    fn brew_installs_without_a_version() {
        let (prog, args) = build_command("brew", "ripgrep", "14.1.1", "update", "pip3").unwrap();
        assert_eq!(prog, "brew");
        assert_eq!(args, vec!["install", "ripgrep"]);
    }

    #[test]
    fn npx_promote_installs_globally_via_npm() {
        let (prog, args) = build_command("npx", "create-vite", "5.0.0", "promote", "pip3").unwrap();
        assert_eq!(prog, "npm");
        assert_eq!(args, vec!["i", "-g", "create-vite"]);
    }

    #[test]
    fn brew_rollback_is_unsupported() {
        assert!(build_command("brew", "ripgrep", "14.0.0", "rollback", "pip3").is_none());
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib ops 2>&1 | tail -10
```
Expected: compile error, `cannot find function 'build_command'`.

- [ ] **Step 3: Implement build_command**

Add above the `#[cfg(test)]` block in `src-tauri/src/ops.rs`:

```rust
/// Build the (program, args) for an operation. `pip_bin` is the resolved pip
/// binary (e.g. "pip3"). Returns None for unsupported combinations, notably
/// brew rollback (Homebrew keeps no old bottles).
pub fn build_command(
    eco: &str,
    pkg: &str,
    version: &str,
    action: &str,
    pip_bin: &str,
) -> Option<(String, Vec<String>)> {
    match (eco, action) {
        ("npm", _) => Some((
            "npm".to_string(),
            vec!["i".to_string(), "-g".to_string(), format!("{}@{}", pkg, version)],
        )),
        ("pip", _) => Some((
            pip_bin.to_string(),
            vec!["install".to_string(), format!("{}=={}", pkg, version)],
        )),
        // brew install/update only; no version pinning and no rollback.
        ("brew", "install") | ("brew", "update") => {
            Some(("brew".to_string(), vec!["install".to_string(), pkg.to_string()]))
        }
        // npx Promote to global: install the package globally via npm.
        ("npx", "promote") => Some((
            "npm".to_string(),
            vec!["i".to_string(), "-g".to_string(), pkg.to_string()],
        )),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo test --lib ops 2>&1 | tail -8
```
Expected: `test result: ok. 5 passed`.

- [ ] **Step 5: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/ops.rs
git commit -m "feat: per-ecosystem command builder with unit tests"
```

---

## Task 4: Streaming run_op command

**Files:**
- Modify: `src-tauri/src/ops.rs` (add run_op), `src-tauri/src/scan/pip.rs` (expose pip_bin), `src-tauri/src/lib.rs` (register run_op)

`run_op` runs on a background thread so the command returns immediately; the frontend listens for events keyed by `opId`. stdout and stderr are each read on their own thread and emitted line by line. On a zero exit code the change is logged to history.

- [ ] **Step 1: Expose the pip binary resolver**

In `src-tauri/src/scan/pip.rs`, change `fn pip_bin()` to `pub(crate) fn pip_bin()` so ops can resolve it. (Leave the body unchanged.)

- [ ] **Step 2: Add run_op to ops.rs**

Add to `src-tauri/src/ops.rs` (above the tests):

```rust
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

use crate::store::{HistoryEntry, Store};

#[derive(Clone, Serialize)]
struct LineEvent {
    op_id: String,
    stream: String, // "stdout" | "stderr"
    line: String,
}

#[derive(Clone, Serialize)]
struct DoneEvent {
    op_id: String,
    success: bool,
    code: i32,
}

/// Spawn the operation on a background thread, streaming `transfer-line` events
/// and a final `transfer-done`. On success, log a HistoryEntry to `store`.
/// Returns immediately. `ts` is the current Unix time (the caller stamps it so
/// the executor stays deterministic to test around).
#[allow(clippy::too_many_arguments)]
pub fn run_op(
    app: AppHandle,
    store: Store,
    op_id: String,
    eco: String,
    pkg: String,
    from: Option<String>,
    to: String,
    action: String,
    ts: i64,
) {
    let pip = crate::scan::pip::pip_bin().unwrap_or("pip3");
    let built = build_command(&eco, &pkg, &to, &action, pip);

    std::thread::spawn(move || {
        let (prog, args) = match built {
            Some(c) => c,
            None => {
                let _ = app.emit(
                    "transfer-done",
                    DoneEvent { op_id: op_id.clone(), success: false, code: -1 },
                );
                return;
            }
        };

        let mut child = match Command::new(&prog)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = app.emit(
                    "transfer-line",
                    LineEvent { op_id: op_id.clone(), stream: "stderr".into(), line: format!("failed to start {}: {}", prog, e) },
                );
                let _ = app.emit("transfer-done", DoneEvent { op_id: op_id.clone(), success: false, code: -1 });
                return;
            }
        };

        let mut handles = Vec::new();
        for (name, pipe) in [("stdout", child.stdout.take()), ("stderr", child.stderr.take())] {
            if let Some(pipe) = pipe {
                let app2 = app.clone();
                let id2 = op_id.clone();
                let stream = name.to_string();
                handles.push(std::thread::spawn(move || {
                    for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                        let _ = app2.emit("transfer-line", LineEvent { op_id: id2.clone(), stream: stream.clone(), line });
                    }
                }));
            }
        }

        let status = child.wait();
        for h in handles {
            let _ = h.join();
        }

        let code = status.as_ref().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        let success = status.map(|s| s.success()).unwrap_or(false);

        if success {
            store.add_history(HistoryEntry { ts, pkg, eco, action, from, to });
        }
        let _ = app.emit("transfer-done", DoneEvent { op_id, success, code });
    });
}
```

- [ ] **Step 3: Register the run_op command in lib.rs**

In `src-tauri/src/lib.rs`, add the command wrapper (which stamps the timestamp) and register it. Add:

```rust
#[tauri::command]
fn run_op(
    app: tauri::AppHandle,
    op_id: String,
    eco: String,
    pkg: String,
    from: Option<String>,
    to: String,
    action: String,
) {
    let store = open_store(&app);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    ops::run_op(app.clone(), store, op_id, eco, pkg, from, to, action, ts);
}
```

And update the handler:

```rust
.invoke_handler(tauri::generate_handler![scan_installed, set_pin, get_history, run_op])
```

- [ ] **Step 4: Build**

```bash
source "$HOME/.cargo/env" && cd src-tauri && cargo build 2>&1 | tail -10
```
Expected: `Finished`, no errors.

- [ ] **Step 5: Commit**

```bash
cd /Users/zach/Documents/GitHub/napm
git add src-tauri/src/ops.rs src-tauri/src/scan/pip.rs src-tauri/src/lib.rs
git commit -m "feat: streaming run_op executor with live events and history logging"
```

---

## Task 5: Frontend — real Transfers, history, pins, rollback, Update All

**Files:**
- Modify: `frontend/index.html`, then mirror to `prototype/napm-prototype.html`

Replace the fake `setInterval` progress with a real op: call `run_op`, render an active row, append streamed lines, finalize on `transfer-done`. Load history from the backend. Make pins call the backend. Gate brew rollback.

- [ ] **Step 1: Add CSS for the streamed output log**

In `frontend/index.html`, find the transfers progress-bar CSS:
```css
  .pbar{height:16px; background:#fff; position:relative; overflow:hidden;}
```
Add directly above it:
```css
  .xout{font-family:var(--mono); font-size:13px; line-height:1.25; background:#fff; color:var(--ddgray);
    height:84px; overflow:auto; padding:3px 5px; white-space:pre-wrap; word-break:break-word;}
  .xout .err{color:var(--red);}
  .xstat{font-weight:bold; margin-top:3px;} .xstat.ok{color:var(--green);} .xstat.fail{color:var(--red);}
```

- [ ] **Step 2: Replace queueTransfer with a real streaming version**

In `frontend/index.html`, find the entire `queueTransfer` function (it builds `cmd`, pushes an `x` object, and runs a `setInterval` that fakes progress). Replace the whole function with:

```javascript
  var opSeq=0;
  function queueTransfer(ti,target,action){
    var t=TOOLS[ti], from=t.installed;
    var cmd=(t.eco==="npm"?"npm i -g "+t.pkg+"@"+target
           :t.eco==="pip"?"pip install "+t.pkg+"=="+target
           :t.eco==="brew"?"brew install "+t.pkg
           :"npm i -g "+t.pkg);
    var opId="op"+(++opSeq);
    var x={opId:opId,ti:ti,name:t.name,user:t.publisher||"unknown",cmd:cmd,lines:[],done:false,ok:false,
           action:action,from:from,to:target};
    xfers.unshift(x); renderXfers();
    var inv=window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke;
    if(!inv){ x.done=true; x.ok=false; x.lines.push("(not running in napm)"); renderXfers(); return; }
    inv("run_op",{opId:opId,eco:t.eco,pkg:t.pkg,from:from,to:target,action:action});
  }
```

- [ ] **Step 3: Replace renderXfers to show the streamed log**

Find the `renderXfers` function and replace it with:

```javascript
  function renderXfers(){
    if(!xfers.length){ xferListEl.innerHTML='<div class="empty">Nothing transferring.</div>'; }
    else{ xferListEl.innerHTML="";
      xfers.forEach(function(x){
        var d=document.createElement("div"); d.className="xfer-row raised";
        var body=x.lines.map(function(l){ return '<span class="'+(l.stream==="stderr"?"err":"")+'">'+esc(l.line)+'</span>'; }).join("\n");
        var stat=x.done?('<div class="xstat '+(x.ok?"ok":"fail")+'">'+(x.ok?"✓ complete":"✗ failed")+'</div>'):"";
        d.innerHTML='<div class="xfer-top"><div class="name">'+esc(x.name)+'</div><div class="from">@'+esc(x.user)+'</div>'+
          '<div class="kbps">'+(x.done?(x.ok?"done":"error"):"running")+'</div></div>'+
          '<div class="cmd">$ '+esc(x.cmd)+'</div>'+
          '<div class="xout">'+body+'</div>'+stat;
        xferListEl.appendChild(d);
      });
    }
    var active=xfers.filter(function(x){return !x.done;}).length;
    badge.textContent=active; badge.style.display=active?"":"none";
  }
```

- [ ] **Step 4: Listen for transfer events on boot**

In the bootstrap area (near the appetite dial IIFE at the end), add an event-wiring block:

```javascript
  (function(){
    var ev=window.__TAURI__&&window.__TAURI__.event;
    if(!ev) return;
    ev.listen("transfer-line",function(e){
      var x=xfers.find(function(z){return z.opId===e.payload.op_id;});
      if(x){ x.lines.push({stream:e.payload.stream,line:e.payload.line}); renderXfers(); }
    });
    ev.listen("transfer-done",function(e){
      var x=xfers.find(function(z){return z.opId===e.payload.op_id;});
      if(!x) return;
      x.done=true; x.ok=e.payload.success;
      if(x.ok && x.ti!=null && TOOLS[x.ti]){ TOOLS[x.ti].installed=x.to; }
      renderXfers(); renderRows();
      loadHistory();
    });
  })();
```

- [ ] **Step 5: Load persistent history from the backend**

Find `renderHistory` and the in-memory `history` array. Replace the history rendering to read from the backend. Add a `loadHistory` function near `renderHistory`:

```javascript
  function loadHistory(){
    var inv=window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke;
    if(!inv){ renderHistory([]); return; }
    inv("get_history").then(function(h){ renderHistory(h||[]); }).catch(function(){ renderHistory([]); });
  }
```

Replace the `renderHistory` function with a version that takes the history list and renders it (brew rollback gated):

```javascript
  function renderHistory(hist){
    if(!hist || !hist.length){ histWrap.innerHTML='<div class="empty">No version changes yet.</div>'; return; }
    var rows=hist.map(function(h){
      var label=h.action==="update"?"updated":h.action==="rollback"?"rolled back":"installed";
      var canRoll = h.action!=="rollback" && h.from && h.eco!=="brew";
      var rb=canRoll?'<button class="btn rowbtn" data-roll-pkg="'+esc(h.pkg)+'" data-roll-eco="'+esc(h.eco)+'" data-roll-from="'+esc(h.from)+'">Roll back</button>'
                    :'<span class="muted" title="'+(h.eco==="brew"?"Homebrew cannot downgrade":"")+'">—</span>';
      var when=new Date(h.ts*1000).toLocaleString([], {month:"short",day:"numeric",hour:"2-digit",minute:"2-digit"});
      return '<tr><td class="t">'+esc(when)+'</td><td><b>'+esc(h.pkg)+'</b></td><td class="act-'+esc(h.action)+'">'+label+'</td>'+
        '<td class="user">'+esc(h.from||"—")+' → '+esc(h.to)+'</td><td>'+rb+'</td></tr>';
    }).join("");
    histWrap.innerHTML='<table class="hist"><tbody>'+rows+'</tbody></table>';
  }
```

- [ ] **Step 6: Wire rollback clicks and remove the old history wiring**

Find the old `histWrap.addEventListener("click", ...)` and the old `addHistory` function. Replace the history click handler with one that rolls back by reinstalling the prior version, and delete the now-unused `addHistory` function (history now comes from the backend):

```javascript
  histWrap.addEventListener("click",function(e){
    var b=e.target.closest("[data-roll-pkg]"); if(!b) return;
    var pkg=b.dataset.rollPkg, eco=b.dataset.rollEco, fromV=b.dataset.rollFrom;
    var ti=findTool(pkg); if(ti<0) return;
    queueTransfer(ti, fromV, "rollback"); switchTab("transfers");
  });
```

- [ ] **Step 7: Make pin toggles persist**

Find the rows click handler branch that toggles pins (`var p=e.target.closest("[data-pin]")`). Replace its body so it calls the backend and re-scans:

```javascript
    var p=e.target.closest("[data-pin]");
    if(p){ e.stopPropagation(); var i=+p.dataset.pin; var nowPinned=!TOOLS[i].pinned;
      TOOLS[i].pinned=nowPinned; renderRows();
      var inv=window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke;
      if(inv) inv("set_pin",{pkg:TOOLS[i].pkg,pinned:nowPinned});
      return; }
```

- [ ] **Step 8: Call loadHistory on boot**

In the bootstrap line, replace the `renderHistory();` call (which took no args) with `loadHistory();`. The final bootstrap line should read:

```javascript
  renderXfers(); loadHistory(); runSearch(""); nextStep(); scanLibrary();
```

- [ ] **Step 9: Mirror to the prototype**

```bash
cd /Users/zach/Documents/GitHub/napm
cp frontend/index.html prototype/napm-prototype.html
diff -q frontend/index.html prototype/napm-prototype.html && echo identical
```

- [ ] **Step 10: Manual verification (human, GUI)**

Restart and exercise a real, reversible operation:
```bash
source "$HOME/.cargo/env" && npm run tauri dev
```
1. Pin a tool, quit the app, relaunch — the pin survives (it is in `~/Library/Application Support/<id>/pins.json`).
2. Update a small npm tool (or pick one): the Transfers row shows real streamed npm output and ends with "✓ complete"; the library row flips to current.
3. The History section lists the change with a real timestamp.
4. Click Roll back on that history row: it reinstalls the prior version, streams output, and the library reflects it.
5. Confirm brew history rows show a disabled "—" rollback with the "Homebrew cannot downgrade" tooltip.

- [ ] **Step 11: Commit**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: real streamed Transfers with persistent history, pins, and rollback"
```

---

## Self-review (completed during planning)

- **Spec coverage (M3):** real streamed install/update with exit code — Task 4/5; persistent history with timestamp + from/to — Task 1/4/5; rollback npm+pip, brew gated, npx promote — Task 3 (build_command) + Task 5 (UI gating); pins persisted and excluded from Update All — Task 1/2/7 (Update All already filters `!pinned` and `isSafe`); `run()` stderr/exit-code gap from M2 — addressed by `run_op`'s dedicated streaming executor.
- **Placeholder scan:** every step has concrete code/commands. The one cross-task forward reference (`mod ops;` before ops.rs exists) is handled explicitly in Task 2 Step 4.
- **Type consistency:** `HistoryEntry { ts, pkg, eco, action, from: Option<String>, to }` is identical in store.rs, ops.rs, and the JS renderer. `build_command(eco, pkg, version, action, pip_bin)` signature matches its caller in `run_op`. Events `transfer-line {op_id, stream, line}` and `transfer-done {op_id, success, code}` match between the Rust `Serialize` structs (serde renames snake_case fields as-is) and the JS `e.payload.op_id` reads. `scan_all(&pins)` matches its `scan_installed` caller.
- **Serde field names:** Rust structs serialize `op_id`/`from`/`to` as snake_case; the JS reads `e.payload.op_id` and sends `{from, to}` — aligned. (If a future camelCase mismatch appears, add `#[serde(rename_all = "camelCase")]`; not needed here since JS uses snake_case.)
- **Known risk:** a Finder-launched bundled app may not have npm/brew on PATH (dev inherits the terminal PATH). Real execution works in `npm run tauri dev`; the bundled-app PATH fix is tracked for M7 packaging.
```
