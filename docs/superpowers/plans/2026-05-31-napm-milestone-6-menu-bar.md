# napm M6 - Menu bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Win98 menu bar functional: real dropdown menus, the View filters/sorts (incl. "only tools I installed"), and the simple File/Swarm/Help/Edit actions.

**Architecture:** A small data-driven dropdown component in the frontend; View state persisted in localStorage and applied in a restructured `renderRows`; a `requested` flag on `InstalledTool` from brew's `installed_on_request`; three thin Rust commands (`open_data_dir`, `open_external`, `clear_caches`) using the existing shell-out pattern. No new plugins or capabilities.

**Tech Stack:** Rust, Tauri v2, vanilla-JS frontend. macOS `open` via `std::process`.

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-6-menu-bar.md`

## Conventions for every task

- `source "$HOME/.cargo/env"` before cargo. Tests: `cd /Users/zach/Documents/GitHub/napm/src-tauri && cargo test --lib` (currently 58). Build: `cargo build`.
- NO em dashes in code/comments/UI strings. Never "Napster" (brand "npstr"). Keep the late-90s look.
- Follow the existing `scan/` and frontend vanilla style (`var`, string-concat HTML, `esc()` for text, `window.__TAURI__.core.invoke`).
- After any frontend task: `cp frontend/index.html prototype/napm-prototype.html` (keep byte-identical).
- Commit after each task with the given message.

## File structure

- Modify `src-tauri/src/scan/mod.rs` - add `requested` field (Task 1).
- Modify `src-tauri/src/scan/{npm,brew,pip,npx}.rs` - set `requested` (Task 1).
- Modify `src-tauri/src/lib.rs` - three commands (Task 2).
- Modify `frontend/index.html` - menu mechanics + menus + View + About (Tasks 3-5).
- Modify `docs/ROADMAP.md` (Task 6).

---

### Task 1: `requested` field on InstalledTool (brew installed_on_request)

**Files:** `src-tauri/src/scan/mod.rs`, `src-tauri/src/scan/brew.rs`, `src-tauri/src/scan/npm.rs`, `src-tauri/src/scan/pip.rs`, `src-tauri/src/scan/npx.rs`

- [ ] **Step 1: Add the field to the struct.** In `src/scan/mod.rs`, add to `InstalledTool` (after `updated`):

```rust
    /// True when the user explicitly asked for this tool (npm/pip/npx globals are
    /// always user-chosen; for brew this is installed_on_request from the receipt).
    /// Unknown defaults to true so "only tools I installed" never wrongly hides a tool.
    pub requested: bool,
```

- [ ] **Step 2: Set `requested: true` in every InstalledTool constructor.** Run `grep -rn "InstalledTool {" src/scan/` and add `requested: true,` to EACH struct literal: `parse_npm` (npm.rs), `parse_brew` (brew.rs line ~55), `parse_pip` (pip.rs), `scan_npx` (npx.rs), AND the `npx_row` test helper in npx.rs `#[cfg(test)]`. Build to confirm no literal is missed (a missing field is a compile error).

- [ ] **Step 3: Write a failing test for the receipt parser.** In `brew.rs`, add a pure parser that reads both the install time and `installed_on_request` from an `INSTALL_RECEIPT.json` body:

```rust
/// Parse an INSTALL_RECEIPT.json body into (install_time_secs, installed_on_request).
/// Either may be None when absent.
pub fn parse_install_receipt(json: &str) -> (Option<i64>, Option<bool>) {
    let v: Value = match serde_json::from_str(json) { Ok(v) => v, Err(_) => return (None, None) };
    let time = v.get("time").and_then(|x| x.as_i64());
    let on_request = v.get("installed_on_request").and_then(|x| x.as_bool());
    (time, on_request)
}
```

Test (in the existing `mod tests`):

```rust
#[test]
fn receipt_yields_time_and_on_request() {
    let r = r#"{"time":1700000000,"installed_on_request":false}"#;
    assert_eq!(parse_install_receipt(r), (Some(1700000000), Some(false)));
    let r2 = r#"{"time":1700000000,"installed_on_request":true}"#;
    assert_eq!(parse_install_receipt(r2).1, Some(true));
    assert_eq!(parse_install_receipt("nope"), (None, None));
}
```

- [ ] **Step 4: Run the test, confirm it passes.** `cargo test --lib`.

- [ ] **Step 5: Use the parser in scan_brew.** Replace the body of `brew_install_time` so it (or a new sibling) returns both values from a single receipt read, and set `row.requested` in `scan_brew`. Concretely: replace `brew_install_time(keg)` usage with a helper `fn brew_receipt(keg: &std::path::Path) -> (i64, bool)` that reads the receipt once, returns `(time.unwrap_or_else(|| path_mtime(keg)), on_request.unwrap_or(true))`. Then in the `scan_brew` keg block set:

```rust
                let (updated, requested) = brew_receipt(&keg);
                row.size = super::size::human_size(super::size::dir_size(&keg));
                row.updated = updated;
                row.requested = requested;
```

Implement `brew_receipt`:

```rust
/// (install time secs, installed_on_request) for a keg, with sensible fallbacks:
/// time falls back to the keg mtime; on_request defaults to true when unknown.
fn brew_receipt(keg: &std::path::Path) -> (i64, bool) {
    let (time, on_request) = std::fs::read_to_string(keg.join("INSTALL_RECEIPT.json"))
        .map(|s| parse_install_receipt(&s))
        .unwrap_or((None, None));
    (time.unwrap_or_else(|| super::path_mtime(keg)), on_request.unwrap_or(true))
}
```

Remove the now-unused `brew_install_time` (or keep it delegating; cleanest is to delete it and use `brew_receipt`).

- [ ] **Step 6: Build and test.** `cargo build && cargo test --lib`
Expected: clean build, 59 tests (one new `receipt_yields_time_and_on_request`). The frontend reads `t.requested` (serde default snake_case key `requested`).

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/scan/
git commit -m "feat: requested flag on tools from brew installed_on_request"
```

---

### Task 2: open_data_dir, open_external, clear_caches commands

**Files:** `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the three commands.** Reuse the `app_data_dir` pattern. Add near the other commands:

```rust
#[tauri::command]
fn open_data_dir(app: tauri::AppHandle) {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("open").arg(&dir).spawn();
}

#[tauri::command]
fn open_external(url: String) {
    // Only open web URLs, never arbitrary local paths or args.
    if url.starts_with("https://") || url.starts_with("http://") {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
}

#[tauri::command]
fn clear_caches(app: tauri::AppHandle) {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    for name in ["brew_catalog.json", "brew_analytics.json", "wire.json"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    // Remove the per-version changelog caches (changelog_*.json).
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with("changelog_") && n.ends_with(".json") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    // Re-warm the brew catalog in the background so the next search is not cold.
    std::thread::spawn(move || {
        let d = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&d);
        search::brew::warm_brew(&d);
    });
}
```

- [ ] **Step 2: Register them.** Add `open_data_dir, open_external, clear_caches` to the `generate_handler!` list.

- [ ] **Step 3: Build.** `cargo build && cargo test --lib`
Expected: clean build, 59 tests still pass. (`std::process::Command::new("open")` needs no Tauri capability; it is our own backend shelling out, same as scan/ops.)

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: open_data_dir, open_external, clear_caches commands"
```

---

### Task 3: Menu mechanics + File/Edit/Swarm/Help menus

**Files:** `frontend/index.html` (the `.menubar` markup ~196-199, its CSS ~44-47, and a new menu engine in the script); mirror to prototype.

Touch only the menu bar and add the new engine. Do NOT alter the library/search/transfers/appetite/titlebar logic except adding menu handlers that call existing functions.

- [ ] **Step 1: Replace the menubar markup.** Replace lines ~196-199 with titles carrying `data-menu` and a shared dropdown container:

```html
  <div class="menubar" id="menubar">
    <span data-menu="file"><u>F</u>ile</span><span data-menu="edit"><u>E</u>dit</span><span data-menu="view"><u>V</u>iew</span><span data-menu="swarm"><u>S</u>warm</span><span data-menu="help"><u>H</u>elp</span>
  </div>
  <div class="menu-pop" id="menuPop" style="display:none"></div>
```

- [ ] **Step 2: Add dropdown CSS** near the `.menubar` rules (~44-47), beveled like the existing chrome (reuse `--face`, `--white`, `--dgray`, `--navy`, `--ddgray`; verify names in `:root`):

```css
  .menubar span.open{background:var(--navy); color:#fff;}
  .menu-pop{position:absolute; z-index:50; background:var(--face); border:1px solid var(--ddgray);
    box-shadow:1px 1px 0 #000; padding:2px; font-size:12px; min-width:180px;}
  .menu-pop .mi{display:flex; align-items:center; gap:6px; padding:3px 18px 3px 8px; cursor:default; white-space:nowrap;}
  .menu-pop .mi:hover{background:var(--navy); color:#fff;}
  .menu-pop .mi.disabled{color:var(--dgray);}
  .menu-pop .mi.disabled:hover{background:transparent; color:var(--dgray);}
  .menu-pop .mark{width:12px; text-align:center;}
  .menu-pop .sep{height:0; border-top:1px solid var(--dgray); margin:3px 1px;}
```

- [ ] **Step 3: Add the menu engine.** Add this near the other top-level script setup. `MENUS` is the data; later tasks add the `view` entry and the real About handler.

```js
  // ---- MENU BAR ----------------------------------------------------------
  function inv(){ return window.__TAURI__&&window.__TAURI__.core&&window.__TAURI__.core.invoke; }
  function copyText(s){
    if(navigator.clipboard && navigator.clipboard.writeText){ navigator.clipboard.writeText(s).catch(function(){fallbackCopy(s);}); }
    else fallbackCopy(s);
  }
  function fallbackCopy(s){ var ta=document.createElement("textarea"); ta.value=s; document.body.appendChild(ta); ta.select(); try{document.execCommand("copy");}catch(e){} document.body.removeChild(ta); }
  function quitApp(){ try{ window.__TAURI__.window.getCurrentWindow().close(); }catch(e){} }
  function copyToolDetails(){
    if(selected==null || !TOOLS[selected]) return;
    var t=TOOLS[selected];
    copyText([t.name+" ("+t.eco+")", "package: "+t.pkg, "installed: "+(t.installed||"-"),
      "latest: "+t.latest, "publisher: @"+(t.publisher||"unknown"), "size: "+(t.size||"-")].join("\n"));
  }
  var MENUS = {
    file: [
      {label:"Rescan now", run:function(){ scanLibrary(); }},
      {label:"Open data folder", run:function(){ var i=inv(); if(i) i("open_data_dir"); }},
      {sep:true},
      {label:"Quit", run:quitApp}
    ],
    edit: [
      {label:"Copy tool details", disabled:function(){ return selected==null; }, run:copyToolDetails}
    ],
    swarm: [
      {label:"Refresh registry caches", run:function(){ var i=inv(); if(i) i("clear_caches"); }}
    ],
    help: [
      {label:"About napm", run:function(){ if(window.showAbout) showAbout(); }},
      {label:"Repo on GitHub", run:function(){ var i=inv(); if(i) i("open_external",{url:"https://github.com/umzcio/napm"}); }}
    ]
  };
  var openMenuName=null;
  function closeMenu(){
    openMenuName=null;
    document.getElementById("menuPop").style.display="none";
    document.querySelectorAll("#menubar span").forEach(function(s){ s.classList.remove("open"); });
  }
  function renderMenu(name, anchor){
    var items=MENUS[name]; if(!items) return;
    var pop=document.getElementById("menuPop");
    pop.innerHTML=items.map(function(it,idx){
      if(it.sep) return '<div class="sep"></div>';
      var dis = it.disabled && it.disabled();
      var mark = it.checked&&it.checked() ? "✓" : it.dot&&it.dot() ? "•" : "";
      return '<div class="mi'+(dis?" disabled":"")+'" data-idx="'+idx+'"><span class="mark">'+mark+'</span>'+esc(it.label)+'</div>';
    }).join("");
    var r=anchor.getBoundingClientRect();
    pop.style.left=r.left+"px"; pop.style.top=r.bottom+"px"; pop.style.display="block";
    openMenuName=name;
    document.querySelectorAll("#menubar span").forEach(function(s){ s.classList.toggle("open", s.dataset.menu===name); });
  }
  document.getElementById("menubar").addEventListener("click",function(e){
    var s=e.target.closest("[data-menu]"); if(!s) return;
    if(openMenuName===s.dataset.menu) closeMenu(); else renderMenu(s.dataset.menu, s);
  });
  document.getElementById("menubar").addEventListener("mouseover",function(e){
    var s=e.target.closest("[data-menu]"); if(!s||openMenuName==null||openMenuName===s.dataset.menu) return;
    renderMenu(s.dataset.menu, s); // hover-switch while a menu is open
  });
  document.getElementById("menuPop").addEventListener("click",function(e){
    var mi=e.target.closest(".mi"); if(!mi||mi.classList.contains("disabled")) return;
    var it=MENUS[openMenuName][+mi.dataset.idx];
    closeMenu();
    if(it && it.run) it.run();
  });
  document.addEventListener("click",function(e){
    if(openMenuName!=null && !e.target.closest("#menubar") && !e.target.closest("#menuPop")) closeMenu();
  });
  document.addEventListener("keydown",function(e){ if(e.key==="Escape") closeMenu(); });
```

- [ ] **Step 4: Mirror + manual check.** `cp frontend/index.html prototype/napm-prototype.html`. The human runs the app: each title opens a dropdown; Rescan/Open data folder/Quit/Refresh caches/Copy details/Repo work; click-outside and Escape close; only one open at a time; hover switches while open. (About shows nothing yet; View not present yet.)

- [ ] **Step 5: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: working Win98 dropdown menus with File/Edit/Swarm/Help actions"
```

---

### Task 4: View menu - filters, sort, descriptions

**Files:** `frontend/index.html` (add the `view` entry to `MENUS`, a `VIEW` state object, and restructure `renderRows`); mirror.

- [ ] **Step 1: Add the VIEW state with localStorage persistence.** Near the `APPETITE` block:

```js
  // ---- VIEW (library filters/sort, persisted) ----------------------------
  var VIEW = { requested:false, outdated:false, sources:{npm:true,brew:true,pip:true,npx:true}, sort:"name", desc:true };
  try{ var sv=JSON.parse(localStorage.getItem("napm.view")); if(sv){ VIEW=Object.assign(VIEW, sv); VIEW.sources=Object.assign({npm:true,brew:true,pip:true,npx:true}, sv.sources||{}); } }catch(e){}
  function saveView(){ try{ localStorage.setItem("napm.view", JSON.stringify(VIEW)); }catch(e){} }
  function setSort(k){ VIEW.sort=k; saveView(); renderRows(); }
  function toggleView(k){ VIEW[k]=!VIEW[k]; saveView(); renderRows(); }
  function toggleSource(s){ VIEW.sources[s]=!VIEW.sources[s]; saveView(); renderRows(); }
  function sizeBytes(s){ var m=/([\d.]+)\s*([KMGT]?B)?/i.exec(s||""); if(!m) return 0; var n=parseFloat(m[1])||0; var u=(m[2]||"B").toUpperCase(); var mul={B:1,KB:1e3,MB:1e6,GB:1e9,TB:1e12}[u]||1; return n*mul; }
  function statusRank(t){ var s=statusOf(t); return s==="update"?0:s==="offline"?1:2; }
```

- [ ] **Step 2: Add the View menu to `MENUS`** (place it so menu order reads File, Edit, View, Swarm, Help - the markup order already fixes display order; the object key just needs to exist). Insert:

```js
    view: [
      {label:"Only tools I installed", checked:function(){return VIEW.requested;}, run:function(){toggleView("requested");}},
      {label:"Only outdated", checked:function(){return VIEW.outdated;}, run:function(){toggleView("outdated");}},
      {sep:true},
      {label:"Source: npm", checked:function(){return VIEW.sources.npm;}, run:function(){toggleSource("npm");}},
      {label:"Source: brew", checked:function(){return VIEW.sources.brew;}, run:function(){toggleSource("brew");}},
      {label:"Source: pip", checked:function(){return VIEW.sources.pip;}, run:function(){toggleSource("pip");}},
      {label:"Source: npx", checked:function(){return VIEW.sources.npx;}, run:function(){toggleSource("npx");}},
      {sep:true},
      {label:"Sort by name", dot:function(){return VIEW.sort==="name";}, run:function(){setSort("name");}},
      {label:"Sort by size", dot:function(){return VIEW.sort==="size";}, run:function(){setSort("size");}},
      {label:"Sort by updated", dot:function(){return VIEW.sort==="updated";}, run:function(){setSort("updated");}},
      {label:"Sort by status", dot:function(){return VIEW.sort==="status";}, run:function(){setSort("status");}},
      {sep:true},
      {label:"Show descriptions", checked:function(){return VIEW.desc;}, run:function(){toggleView("desc");}}
    ],
```

- [ ] **Step 3: Restructure `renderRows`** to filter + sort a derived list while keeping each row's original TOOLS index. Replace the `TOOLS.forEach(function(t,i){ ... })` wrapper so it iterates a derived `display` list of `{t, i}`:

```js
  function renderRows(){
    rowsEl.innerHTML="";
    var display=TOOLS.map(function(t,i){return {t:t,i:i};}).filter(function(x){
      var t=x.t;
      if(VIEW.requested && t.requested===false) return false;
      if(VIEW.outdated && statusOf(t)!=="update") return false;
      if(!VIEW.sources[t.eco]) return false;
      return true;
    });
    display.sort(function(a,b){
      if(VIEW.sort==="size") return sizeBytes(b.t.size)-sizeBytes(a.t.size);
      if(VIEW.sort==="updated") return (b.t.updated||0)-(a.t.updated||0);
      if(VIEW.sort==="status") return statusRank(a.t)-statusRank(b.t) || a.t.name.localeCompare(b.t.name);
      return a.t.name.localeCompare(b.t.name);
    });
    display.forEach(function(x){
      var t=x.t, i=x.i;
      var npx = t.eco==="npx";
      var st=statusOf(t), off=st==="offline";
      var kind = (st==="update"&&!npx) ? bumpKind(t.installed,t.latest) : "none";
      var safe = st==="update" && !npx && isSafe(kind);
      var held = st==="update" && !npx && !safe;
      var g = npx?["♪","g-off"] : st==="update" ? (safe?["↑","g-safe"]:["↑","g-hold"]) : GLYPH[st];
      var gTitle = (st==="update"&&!npx) ? kind+" update"+(held?" - above your appetite":"") : "";
      var tr=document.createElement("tr"); tr.dataset.i=i;
      if(selected===i) tr.className="sel";
      var action = npx ? '<span class="muted">—</span>'
                 : st==="update" ? '<button class="btn rowbtn" data-get="'+i+'">Get</button>'
                 : off ? '<button class="btn rowbtn" data-get="'+i+'">Install</button>' : '<span class="muted">—</span>';
      tr.innerHTML=
        '<td class="glyph '+g[1]+'"'+(gTitle?' title="'+esc(gTitle)+'"':'')+'>'+g[0]+'</td>'+
        '<td>'+(t.pinned?'📌 ':'')+esc(t.name)+' <span class="src '+esc(t.eco)+'">'+esc(t.eco)+'</span>'+
          (VIEW.desc&&t.description?'<span class="toold">'+esc(t.description)+'</span>':'')+'</td>'+
        '<td class="'+(off?'muted':'')+'">'+esc(t.installed||"—")+'</td>'+
        '<td class="'+(npx?'muted':safe?'vernew':held?'verhold':'muted')+'">'+(npx?"—":esc(t.latest))+'</td>'+
        '<td class="user'+(t.publisher?'':' muted')+'">@'+esc(t.publisher||"unknown")+'</td>'+
        '<td class="muted">'+esc(t.size)+'</td>'+
        '<td class="muted">'+ago(t.updated)+'</td>'+
        '<td><span class="pin '+(t.pinned?'on':'')+'" data-pin="'+i+'" title="freeze version, skip Update All">📌</span></td>'+
        '<td>'+action+'</td>';
      tr.addEventListener("click",function(){selected=i; renderRows();});
      rowsEl.appendChild(tr);
    });
    renderStatus();
  }
```

(This preserves every existing behavior; it only adds the filter/sort/desc layer and keeps `data-i`/`data-get`/`data-pin` on the original index. Match the exact glyphs/classes already in the file - the snippet uses escape forms; if the current file uses literal chars, keeping either is fine as long as it renders the same.)

- [ ] **Step 4: Mirror + manual check.** `cp ...`. Human verifies: toggling "Only tools I installed" collapses the brew dependency rows; "Only outdated" hides current rows; source toggles hide/show ecosystems; sort options reorder; "Show descriptions" hides the sub-line; checks/dots reflect state; selection/pin/Get still work on filtered rows; the view persists across relaunch.

- [ ] **Step 5: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: View menu filters, sort, and descriptions over the library"
```

---

### Task 5: About napm modal

**Files:** `frontend/index.html` (a reusable modal + `showAbout`); mirror.

- [ ] **Step 1: Add modal markup** before `</body>` (after the existing content):

```html
  <div class="modal-back" id="modalBack" style="display:none">
    <div class="modal raised" id="modalBox"></div>
  </div>
```

- [ ] **Step 2: Add modal CSS** (beveled, centered, dim backdrop):

```css
  .modal-back{position:fixed; inset:0; background:rgba(0,0,0,.4); z-index:100; display:flex; align-items:center; justify-content:center;}
  .modal{background:var(--face); border:1px solid var(--ddgray); box-shadow:2px 2px 0 #000; padding:0; min-width:320px; max-width:420px;}
  .modal .m-h{background:linear-gradient(90deg,var(--navy),var(--navy2)); color:#fff; font-weight:bold; padding:3px 6px; display:flex; justify-content:space-between;}
  .modal .m-h .x{cursor:pointer; padding:0 4px;}
  .modal .m-b{padding:14px; font-size:12px; line-height:1.5;}
  .modal .m-b img{width:48px; height:48px; vertical-align:middle; margin-right:8px;}
```

- [ ] **Step 3: Add `showAbout` + close wiring.** Make `showAbout` global (so the Help menu's `window.showAbout` reference resolves):

```js
  function closeModal(){ document.getElementById("modalBack").style.display="none"; }
  window.showAbout=function(){
    var box=document.getElementById("modalBox");
    var ver = window.__appVer || "";
    box.innerHTML='<div class="m-h">About napm<span class="x" data-close>×</span></div>'+
      '<div class="m-b"><div><img src="npstr-logo.svg" alt="npstr">'+
      '<b style="font-family:var(--mono);font-size:18px;color:#00880c">napm</b> '+esc(ver)+'</div>'+
      '<div style="margin-top:8px">npstr AI Package Manager. Tracks the CLI tools you have across npm, brew, pip, and npx; tells you what is out of date and whether it is safe to take.</div>'+
      '<div style="margin-top:8px">A late-90s file-sharing client for your dev tools. Homage, not affiliation.</div>'+
      '<div style="margin-top:8px"><a href="#" data-repo>github.com/umzcio/napm</a> · MIT</div></div>';
    document.getElementById("modalBack").style.display="flex";
  };
  document.getElementById("modalBack").addEventListener("click",function(e){
    if(e.target.id==="modalBack" || e.target.hasAttribute("data-close")) { closeModal(); return; }
    if(e.target.hasAttribute("data-repo")){ e.preventDefault(); var i=inv(); if(i) i("open_external",{url:"https://github.com/umzcio/napm"}); }
  });
  document.addEventListener("keydown",function(e){ if(e.key==="Escape") closeModal(); });
```

- [ ] **Step 4: Capture the version for About.** In the existing `setAppVersion` success callback (where it sets the titlebar), also store it: add `window.__appVer="v"+v;` so About shows the live version.

- [ ] **Step 5: Mirror + manual check.** `cp ...`. Human verifies Help -> About napm opens the modal with the logo + live version; the X, backdrop-click, and Escape close it; the repo link opens the browser.

- [ ] **Step 6: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: About napm modal with logo, live version, and repo link"
```

---

### Task 6: Update the roadmap

**Files:** `docs/ROADMAP.md`

- [ ] **Step 1:** Move M6 from "Next" to "Done" with a summary (working Win98 dropdown menus; View filters - only-installed via brew installed_on_request, only-outdated, per-source, sort, descriptions, persisted; File rescan/open-data-folder/quit; Swarm refresh caches; Help about/repo; Edit copy-details). Note the carried-forward deferrals (Preferences/Settings store + token + source enable/disable, Export, Keyboard Shortcuts). Set the next milestone to the Settings milestone or M7 (Packaging), whichever the owner prefers - default to listing both M7 Packaging and the new Settings milestone as upcoming.

- [ ] **Step 2: Commit.**

```bash
git add docs/ROADMAP.md
git commit -m "docs: mark M6 menu bar done"
```

---

## Self-review notes

- Spec coverage: menu mechanics (T3), View filters/sort/desc incl. only-installed (T4 + T1 backend), File/Swarm/Help/Edit actions (T3), open/clear_caches backend (T2), About modal (T5), persistence in localStorage (T4), renderRows keeps original index (T3 step 3 / T4 step 3). Deferred items remain deferred.
- Type consistency: `requested` field added once (T1) and read as `t.requested` in renderRows (T4). `MENUS` object built in T3, `view` key added in T4, `showAbout` defined in T5 and referenced via `window.showAbout` in T3 (forward-safe because it is called at click time, not load time). `inv()` helper defined in T3 and reused. Commands `open_data_dir`/`open_external`/`clear_caches` defined T2, called T3.
- No placeholders: every step has concrete code; backend steps have real test assertions.
- No new crates/plugins: `open` via std::process; clipboard via web API.
