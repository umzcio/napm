# napm M8 - Right-click context menus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Right-click any meaningful row for a Win98 context menu, reusing the M6 menu engine and routing every item through existing functions.

**Architecture:** Generalize the M6 `renderMenu` into a cursor-positionable `openPopup(items, x, y)`; add four `contextmenu` handlers (library, search, transfers, history) that build item lists from the row under the cursor. Frontend only - no backend, no new commands.

**Tech Stack:** Vanilla-JS in `frontend/index.html`.

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-8-right-click.md`

## Conventions for every task

- Edit only `frontend/index.html`; after each task `cp frontend/index.html prototype/napm-prototype.html` (keep byte-identical).
- Match the existing vanilla style (`var`, string-concat HTML, `esc()` for any text from data, the `window.__TAURI__.core.invoke` access).
- NO em dashes in new strings/comments. Never "Napster" (brand "npstr"). Keep the late-90s look.
- No backend changes, no cargo. Verified live (the human runs `npm run tauri dev`).
- Commit after each task with the given message.

## File structure

All changes are in `frontend/index.html`: the menu engine (Task 1), shared helpers + history storage (Task 2), library + search context menus (Task 3), transfers + history context menus (Task 4). Roadmap in Task 5.

---

### Task 1: Generalize the menu engine into `openPopup`

Refactor the menu core (around lines 815-849, the `openMenuName`/`closeMenu`/`renderMenu`/`#menuPop` click block) so it can open at an arbitrary cursor position with an arbitrary item array, while the menubar keeps working.

- [ ] **Step 1: Replace the engine block.** Replace from `var openMenuName=null;` through the `#menuPop` click handler with:

```js
  var openMenuName=null, popupOpen=false, currentItems=null;
  function closePopup(){
    popupOpen=false; openMenuName=null; currentItems=null;
    document.getElementById("menuPop").style.display="none";
    document.querySelectorAll("#menubar span").forEach(function(s){ s.classList.remove("open"); });
  }
  // Open a popup menu of `items` at viewport coords (x,y). Items may include
  // falsy entries (omitted) so callers can conditionally include an item.
  function openPopup(items, x, y){
    items=items.filter(Boolean);
    currentItems=items; openMenuName=null;
    document.querySelectorAll("#menubar span").forEach(function(s){ s.classList.remove("open"); });
    var pop=document.getElementById("menuPop");
    pop.innerHTML=items.map(function(it,idx){
      if(it.sep) return '<div class="sep"></div>';
      var dis = it.disabled && it.disabled();
      var mark = it.checked&&it.checked() ? "✓" : it.dot&&it.dot() ? "•" : "";
      return '<div class="mi'+(dis?" disabled":"")+'" data-idx="'+idx+'"><span class="mark">'+mark+'</span>'+esc(it.label)+'</div>';
    }).join("");
    pop.style.left=x+"px"; pop.style.top=y+"px"; pop.style.display="block";
    var maxLeft=window.innerWidth-pop.offsetWidth-4; if(x>maxLeft) pop.style.left=Math.max(2,maxLeft)+"px";
    var maxTop=window.innerHeight-pop.offsetHeight-4; if(y>maxTop) pop.style.top=Math.max(2,maxTop)+"px";
    popupOpen=true;
  }
  function renderMenu(name, anchor){
    var items=MENUS[name]; if(!items) return;
    var r=anchor.getBoundingClientRect();
    openPopup(items, r.left, r.bottom);
    openMenuName=name;
    document.querySelectorAll("#menubar span").forEach(function(s){ s.classList.toggle("open", s.dataset.menu===name); });
  }
  document.getElementById("menubar").addEventListener("click",function(e){
    var s=e.target.closest("[data-menu]"); if(!s) return;
    if(openMenuName===s.dataset.menu) closePopup(); else renderMenu(s.dataset.menu, s);
  });
  document.getElementById("menubar").addEventListener("mouseover",function(e){
    var s=e.target.closest("[data-menu]"); if(!s||openMenuName==null||openMenuName===s.dataset.menu) return;
    renderMenu(s.dataset.menu, s); // hover-switch while a menu is open
  });
  document.getElementById("menuPop").addEventListener("click",function(e){
    var mi=e.target.closest(".mi"); if(!mi||mi.classList.contains("disabled")) return;
    var it=currentItems&&currentItems[+mi.dataset.idx];
    closePopup();
    if(it && it.run) it.run();
  });
```

- [ ] **Step 2: Update the two dismiss handlers** (the outside-mousedown-capture and the Escape keydown that follow the block). They currently gate on `openMenuName!=null` and call `closeMenu()`. Change both to gate on `popupOpen` and call `closePopup()`:

```js
  document.addEventListener("mousedown",function(e){
    if(popupOpen && !e.target.closest("#menubar") && !e.target.closest("#menuPop")) closePopup();
  }, true);
```

And the Escape handler (the unified menu/modal one): change `if(openMenuName!=null) closeMenu(); else closeModal();` to `if(popupOpen) closePopup(); else closeModal();`.

- [ ] **Step 3: Grep for any leftover `closeMenu(`** references: `grep -n "closeMenu" frontend/index.html`. There should be NONE after the rename (everything is `closePopup`). Fix any stragglers.

- [ ] **Step 4: Mirror + manual check.** `cp ...`. Human confirms the menu BAR still works exactly as before (open, switch, dismiss, items act) - this task is a pure refactor with no behavior change yet.

- [ ] **Step 5: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "refactor: generalize menu engine into cursor-positionable openPopup"
```

---

### Task 2: Shared helpers + history storage

**Files:** `frontend/index.html`

- [ ] **Step 1: Add the helper functions** (near the other menu helpers, e.g. after `removeCmd`):

```js
  function registryUrl(eco, pkg){
    if(eco==="npm"||eco==="npx") return "https://www.npmjs.com/package/"+pkg;
    if(eco==="brew") return "https://formulae.brew.sh/formula/"+pkg;
    if(eco==="pip") return "https://pypi.org/project/"+pkg+"/";
    return null;
  }
  function installCmd(eco, pkg, version){
    if(eco==="pip") return "pip install "+pkg+(version?"=="+version:"");
    if(eco==="brew") return "brew install "+pkg;
    return "npm i -g "+pkg+(version?"@"+version:""); // npm / npx
  }
  function openExt(url){ var i=inv(); if(i && url) i("open_external",{url:url}); }
  // Most recent prior version for a tool from the loaded history (newest-first),
  // used by the library Roll back item. Null when none.
  function priorVersion(pkg, eco){
    for(var k=0;k<history.length;k++){ var h=history[k];
      if(h.pkg===pkg && h.eco===eco && h.action!=="rollback" && h.from) return h.from; }
    return null;
  }
  function togglePin(i){
    var t=TOOLS[i]; if(!t) return;
    t.pinned=!t.pinned; renderRows();
    var iv=inv(); if(iv) iv("set_pin",{pkg:t.pkg,pinned:t.pinned});
  }
```

(`inv()` is the existing global helper added in M6. `history` is the module var declared at `var xfers=[], history=[];`.)

- [ ] **Step 2: Store the loaded history.** In `loadHistory`, set the module `history` var before rendering so `priorVersion` can read it. Change the `.then`/`.catch`:

```js
  function loadHistory(){
    var iv=inv();
    if(!iv){ history=[]; renderHistory([]); return; }
    iv("get_history").then(function(h){ history=h||[]; renderHistory(history); }).catch(function(){ history=[]; renderHistory([]); });
  }
```

- [ ] **Step 3: Route the existing pin click through `togglePin`** (DRY). In the `rowsEl` click handler, replace the `data-pin` branch body with `togglePin(+p.dataset.pin); return;`:

```js
    var p=e.target.closest("[data-pin]");
    if(p){ e.stopPropagation(); togglePin(+p.dataset.pin); return; }
```

- [ ] **Step 4: Mirror + build sanity.** `cp ...`. No visible change yet (helpers unused until Tasks 3-4; pin still works via togglePin). Human can confirm pin toggle still works.

- [ ] **Step 5: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: context-menu helpers (registry URL, install cmd, prior version, pin) and history storage"
```

---

### Task 3: Library + Search context menus

**Files:** `frontend/index.html`

- [ ] **Step 1: Add the library context menu builder + handler.** Place near the `rowsEl` handlers:

```js
  function libMenu(i, x, y){
    var t=TOOLS[i]; if(!t) return;
    var st=statusOf(t);
    var prev=t.eco!=="brew"?priorVersion(t.pkg,t.eco):null;
    var ru=registryUrl(t.eco,t.pkg);
    openPopup([
      st==="update" ? {label:"Update to "+t.latest, run:function(){ queueTransfer(i,t.latest,"update"); switchTab("transfers"); }}
        : !t.installed ? {label:"Install", run:function(){ queueTransfer(i,t.latest,"install"); switchTab("transfers"); }}
        : {label:"Up to date", disabled:function(){return true;}},
      {label: prev?("Roll back to "+prev):"Roll back", disabled:function(){ return !prev; },
        run:function(){ if(prev){ queueTransfer(i,prev,"rollback"); switchTab("transfers"); } }},
      {sep:true},
      {label: t.pinned?"Unpin":"Pin", run:function(){ togglePin(i); }},
      {label:"Copy package name", run:function(){ copyText(t.pkg); }},
      {label:"Copy install command", run:function(){ copyText(installCmd(t.eco,t.pkg,t.installed||t.latest)); }},
      ru?{label:"Open "+t.eco+" page", run:function(){ openExt(ru); }}:null,
      {sep:true},
      {label:"What's New for this", run:function(){ if(!FEED_LOADED) loadWhatsNew(); switchTab("whatsnew"); }}
    ], x, y);
  }
  rowsEl.addEventListener("contextmenu",function(e){
    var tr=e.target.closest("tr[data-i]"); if(!tr) return;
    e.preventDefault();
    libMenu(+tr.dataset.i, e.clientX, e.clientY);
  });
```

- [ ] **Step 2: Add `data-pkg` to search result rows.** In `renderSearchResults`, add `data-pkg` to each result `<tr>` so the context handler can identify the result. Change `return '<tr>'+` to `return '<tr data-pkg="'+esc(p.pkg)+'">'+`.

- [ ] **Step 3: Add the search context menu builder + handler.** Near the `resEl` handlers:

```js
  function searchMenu(pkg, x, y){
    var p=null; for(var k=0;k<SWARM.length;k++) if(SWARM[k].pkg===pkg) p=SWARM[k];
    if(!p) return;
    var ru=registryUrl(p.eco,p.pkg);
    openPopup([
      {label:"Get / Install", run:function(){ installPackage(p.pkg); }},
      {label:"Copy package name", run:function(){ copyText(p.pkg); }},
      {label:"Copy install command", run:function(){ copyText(installCmd(p.eco,p.pkg,p.version)); }},
      ru?{label:"Open "+p.eco+" page", run:function(){ openExt(ru); }}:null,
      {sep:true},
      {label:"Filter swarm to "+p.eco, run:function(){
        searchSource=p.eco;
        document.querySelectorAll("#srcChips .chip").forEach(function(c){ c.classList.toggle("on", c.dataset.src===p.eco); });
        renderSearchResults();
      }}
    ], x, y);
  }
  resEl.addEventListener("contextmenu",function(e){
    var tr=e.target.closest("tr[data-pkg]"); if(!tr) return;
    e.preventDefault();
    searchMenu(tr.dataset.pkg, e.clientX, e.clientY);
  });
```

- [ ] **Step 4: Mirror + manual check.** `cp ...`. Human verifies: right-click a library row -> Update/Install + Roll back (greyed for brew or no-prior) + Pin/Unpin + Copy name/command + Open page + What's New; the actions route correctly (install/rollback go to Transfers, copy lands on the clipboard, page opens). Right-click a search result -> Get + copies + Open page + Filter to source. Right-clicking empty space opens nothing.

- [ ] **Step 5: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: library and search right-click context menus"
```

---

### Task 4: Transfers + History context menus

**Files:** `frontend/index.html`

- [ ] **Step 1: Tag transfer rows with their index.** In `renderXfers`, change `xfers.forEach(function(x){` to `xfers.forEach(function(x,xi){` and add `d.dataset.i=xi;` right after `d.className="xfer-row raised";`.

- [ ] **Step 2: Add the transfers context menu + handler.** `queueTransfer` already stamps `action`, `from`, and `to` onto each row, so Re-run just re-queues:

```js
  function xferMenu(xi, x, y){
    var t=xfers[xi]; if(!t) return;
    openPopup([
      {label:"Copy log output", disabled:function(){ return !t.lines.length; },
        run:function(){ copyText(t.lines.map(function(l){return l.line;}).join("\n")); }},
      {label:"Copy command", run:function(){ copyText(t.cmd); }},
      {sep:true},
      {label:"Re-run", run:function(){ queueTransfer(t.ti, t.to, t.action); }}
    ], x, y);
  }
  xferListEl.addEventListener("contextmenu",function(e){
    var d=e.target.closest(".xfer-row[data-i]"); if(!d) return;
    e.preventDefault();
    xferMenu(+d.dataset.i, e.clientX, e.clientY);
  });
```

- [ ] **Step 3: Tag history rows with their index.** In `renderHistory`, the rows are built from `hist` via `.map(function(h){...})`. Change to `.map(function(h,hi){...})` and add `data-i="'+hi+'"` to the `<tr>`: change `return '<tr>'` to `return '<tr data-i="'+hi+'">'`.

- [ ] **Step 4: Add the history context menu + handler.** It reads the stored `history` array (set in Task 2):

```js
  function histMenu(hi, x, y){
    var h=history[hi]; if(!h) return;
    var canRoll = h.action!=="rollback" && h.from && h.eco!=="brew";
    var ti=findTool(h.pkg);
    openPopup([
      {label: canRoll?("Roll back to "+h.from):"Roll back", disabled:function(){ return !canRoll || ti<0; },
        run:function(){ if(canRoll && ti>=0){ queueTransfer(ti, h.from, "rollback"); switchTab("transfers"); } }},
      {label:"Copy entry", run:function(){ copyText(h.pkg+" "+h.action+" "+(h.from||"-")+" -> "+h.to); }},
      {sep:true},
      {label:"Jump to tool in library", disabled:function(){ return ti<0; },
        run:function(){ if(ti>=0){ selected=ti; switchTab("library"); renderRows(); } }}
    ], x, y);
  }
  histWrap.addEventListener("contextmenu",function(e){
    var tr=e.target.closest("tr[data-i]"); if(!tr) return;
    e.preventDefault();
    histMenu(+tr.dataset.i, e.clientX, e.clientY);
  });
```

- [ ] **Step 5: Mirror + manual check.** `cp ...`. Human verifies: right-click a transfer row -> Copy log / Copy command / Re-run (Re-run re-queues the same op); right-click a history entry -> Roll back (greyed for brew/rollback entries) + Copy entry + Jump to tool (switches to Library with the row selected). Dismiss (outside, Escape, picking an item) works for all context menus.

- [ ] **Step 6: Commit.**

```bash
git add frontend/index.html prototype/napm-prototype.html
git commit -m "feat: transfers and history right-click context menus"
```

---

### Task 5: Update the roadmap

**Files:** `docs/ROADMAP.md`

- [ ] **Step 1:** Move M8 from "Next" to "Done" with a summary (the M6 engine generalized into `openPopup`; four cursor-positioned context menus - library, search, transfers, history - routing through existing actions; registry-page links via `open_external`; library rollback driven by history and brew-gated). Set M9 (Manual / standalone installs) as the next milestone.

- [ ] **Step 2: Commit.**

```bash
git add docs/ROADMAP.md
git commit -m "docs: mark M8 right-click context menus done, M9 next"
```

---

## Self-review notes

- Spec coverage: engine generalization (T1), helpers + history storage + Re-run plumbing-already-present (T2), library + search menus with registry page + rollback (T3), transfers + history menus (T4), roadmap (T5). Registry page used instead of homepage (no backend). brew/no-prior rollback greyed.
- Type consistency: `openPopup(items, x, y)` defined T1, called by `renderMenu` and all four context builders (T3/T4). `currentItems` read by the `#menuPop` click handler (T1). `priorVersion`/`installCmd`/`registryUrl`/`openExt`/`togglePin` defined T2, used T3/T4. `history` stored T2, read by `priorVersion` and `histMenu`. Transfer rows carry `action`/`from`/`to` from `queueTransfer` (already present).
- No placeholders: every step has concrete code. The dismiss handlers and the closeMenu->closePopup rename are explicit.
- Frontend only, no backend, no new commands.
