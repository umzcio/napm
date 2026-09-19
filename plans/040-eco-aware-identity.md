# Plan 040: Match packages by (eco, pkg) everywhere, not pkg alone

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- src-tauri/src/intel/mod.rs src-tauri/src/lib.rs frontend/index.html`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none (touches `frontend/index.html`; land after 038/039 if
  they are in flight to keep drift checks clean)
- **Category**: bug
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

Cross-ecosystem name collisions are common (`prettier` exists on npm and
Homebrew; `http`, `jq`, `fuzzy` similarly). The codebase already treats this
as real - `importer.rs:263-279` has a dedicated test
(`already_present_is_eco_aware_not_pkg_only`) proving a pkg-only match
misclassifies the brew row. But two surfaces still match on `pkg` alone:

1. **What's New verdict scope** (`src-tauri/src/intel/mod.rs:124-127`): the
   frontend asks for age verdicts by pkg name; the backend resolves the
   first tool with that name regardless of ecosystem. For a same-name pair,
   one row gets the other ecosystem's age verdict (mislabeled "safe"/"new").
2. **Search actions** (`frontend/index.html:618-619`, `:646-648`):
   `installPackage` and `searchMenu` find a SWARM entry by pkg alone, and
   the `data-install`/`data-pkg` attributes carry no ecosystem. The row's
   own button label is eco-aware (`findToolIdx(p.pkg, p.eco)` at `:595`), so
   the button can say "Update" for the npm row while clicking it installs
   the brew package of the same name.

## Current state

- Backend scope resolution, `src-tauri/src/intel/mod.rs:122-127`:

  ```rust
  let scope_tools: Vec<&ToolRef> = verdict_scope
      .iter()
      .filter_map(|pkg| installed.iter().find(|t| &t.pkg == pkg))
      .collect();
  ```

  `whats_new` is called from the `get_whats_new` command
  (`src-tauri/src/lib.rs:81-98`), whose signature is
  `verdict_scope: Vec<String>`. `ToolRef` (defined in `intel/mod.rs`,
  deserialized from the frontend) already carries `pkg`, `eco`, `installed`,
  `latest`. `ReleaseInfo` already carries `eco`, so the response shape needs
  no change.
- Frontend scope construction, `frontend/index.html` (`verdictScope()`, just
  before `loadWhatsNew` at `:668`):

  ```js
  function verdictScope(){
    // outdated, non-npx, unpinned, within the current appetite dial.
    return TOOLS.filter(function(t){
      return statusOf(t)==="update" && t.eco!=="npx" && !t.pinned && isSafe(bumpOf(t));
    }).map(function(t){ return t.pkg; });
  }
  ```

- Search row/action sites, `frontend/index.html`: button markup at
  `:598-599` (`data-install="${p.pkg}"`), row markup at `:603`
  (`<tr data-pkg="${p.pkg}">`), click delegate at `:614-617`, `searchMenu`
  pkg-only lookup at `:618-619`, contextmenu delegate at `:635-639`,
  `installPackage` pkg-only lookup at `:646-648`.
- Serde/Tauri arg convention: JS camelCase arg `verdictScope` maps to the
  Rust snake_case parameter `verdict_scope` automatically; object arrays
  (`Vec<SomeStruct>` with `#[derive(Deserialize)]`) are the established
  pattern - `installed: Vec<intel::ToolRef>` already works exactly this way.
- The importer's eco-aware test (`src-tauri/src/importer.rs:263-279`) is the
  exemplar for what "correct" means here.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Tests     | `cd src-tauri && cargo test`              | all pass            |
| Lint      | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 |
| Format    | `cd src-tauri && cargo fmt --all -- --check` | exit 0 |
| App check | `npm run tauri dev`                       | What's New + Search behave as before |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/intel/mod.rs`
- `src-tauri/src/lib.rs` (the `get_whats_new` signature only)
- `frontend/index.html` (`verdictScope`, the search row/button markup, the
  two delegates, `installPackage`, `searchMenu`)

**Out of scope** (do NOT touch):
- `ReleaseInfo` / the response shape - already eco-aware.
- `src-tauri/src/importer.rs` - already correct (it has the model test).
- The OSV scan path (`intel/osv.rs`) - already keyed by ToolRef.
- Any visual change to Search or What's New.

## Git workflow

- Branch: `advisor/040-eco-aware-identity`
- Commit backend and frontend separately; style: `fix(intel): ...` /
  `fix(ui): ...`.

## Steps

### Step 1: Backend - scope entries become (pkg, eco)

In `src-tauri/src/intel/mod.rs`:

```rust
/// A (pkg, eco) pair the frontend wants a release-age verdict for. Matching
/// is on both fields: the same name on two ecosystems (npm/brew "prettier")
/// is two different tools with two different verdicts.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScopeRef {
    pub pkg: String,
    pub eco: String,
}
```

Change `whats_new`'s `verdict_scope: &[String]` to `&[ScopeRef]` and the
resolution to:

```rust
.filter_map(|s| installed.iter().find(|t| t.pkg == s.pkg && t.eco == s.eco))
```

Update `get_whats_new` in `src-tauri/src/lib.rs` to
`verdict_scope: Vec<intel::ScopeRef>`.

**Verify**: `cd src-tauri && cargo build` → compiles.

### Step 2: Backend test

Add a unit test in `intel/mod.rs`'s test module (create one if absent,
mirroring the table style of `importer.rs`'s tests): installed tools are
npm-"prettier" (old, would be "safe") and brew-"prettier"; a scope of
`[{pkg:"prettier", eco:"brew"}]` must resolve the brew tool, not the npm
one. Verdict fetching hits the network, so test at the resolution layer:
extract the `filter_map` into a small helper
`fn resolve_scope<'a>(scope: &[ScopeRef], installed: &'a [ToolRef]) -> Vec<&'a ToolRef>`
and test that directly.

**Verify**: `cd src-tauri && cargo test intel` → passes, including the new
test.

### Step 3: Frontend - verdictScope carries eco

```js
}).map(function(t){ return {pkg:t.pkg, eco:t.eco}; });
```

(No call-site change needed: `get_whats_new` receives `verdictScope()`'s
return value directly.)

### Step 4: Frontend - search rows and actions carry eco

- Row/button markup (`:598-603`): add `data-eco="${p.eco}"` to both the
  `data-install` buttons and the `<tr data-pkg ...>` (the `h``` template
  escapes interpolations, so this is safe by construction).
- Click delegate (`:614-617`): `installPackage(b.dataset.install, b.dataset.eco);`
- Contextmenu delegate (`:635-639`): `searchMenu(tr.dataset.pkg, tr.dataset.eco, e.clientX, e.clientY);`
- `installPackage(pkg, eco)` and `searchMenu(pkg, eco, x, y)`: match on both:

  ```js
  var p=null; for(var i=0;i<SWARM.length;i++) if(SWARM[i].pkg===pkg && SWARM[i].eco===eco) p=SWARM[i];
  ```

**Verify**: `grep -n "data-install" frontend/index.html` shows `data-eco`
alongside; `grep -n "SWARM\[.\]\.pkg===pkg" frontend/index.html` shows both
lookups now also compare `.eco`.

### Step 5: Full gate + manual

`cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check` → all exit 0. Then `npm run tauri dev`: What's
New still renders verdict cards; a search for a colliding name (e.g.
`prettier`, which exists on npm and brew) shows both rows and each row's Get
button installs from ITS OWN ecosystem (check the transfer row's command).

## Test plan

- New backend unit test: step 2 (`resolve_scope` is eco-aware).
- Frontend: manual only (repo convention). The colliding-name search is the
  acceptance test.
- `cargo test` must stay green overall.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cd src-tauri && cargo test` exits 0, including the new eco-aware scope test
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` exits 0
- [ ] `cd src-tauri && cargo fmt --all -- --check` exits 0
- [ ] `grep -n "t.pkg == \|&t.pkg == " src-tauri/src/intel/mod.rs` shows the match also constrains `eco`
- [ ] `grep -n "data-eco" frontend/index.html` matches the search row/button markup
- [ ] `grep -n "SWARM\[" frontend/index.html` shows no pkg-only lookup remains
- [ ] Manual: colliding-name search installs from the clicked row's ecosystem
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `ToolRef` does not in fact carry `eco` (the excerpts say it does; if the
  struct drifted, report the new shape).
- `SWARM` entries lack an `eco` field for some source (check
  `search::SearchResult`'s serialization first).
- Tauri rejects the `Vec<ScopeRef>` argument (serde naming mismatch between
  `verdictScope` and `verdict_scope`); if arg mapping behaves unexpectedly,
  report what the invoke error says rather than renaming things at random.

## Maintenance notes

- The rule going forward: any map/lookup keyed by package name must be keyed
  by (eco, pkg). A reviewer should grep new code for `===pkg` / `== pkg`
  comparisons without an eco sibling.
- If a future feature dedupes SWARM rows across ecosystems, the display can
  merge, but the action identity must stay per-eco.
- Deferred: the `link` field on advisory cards and other pkg-keyed display
  paths that never drive an action (read-only, so a mismatch is cosmetic;
  none known at planning time).
