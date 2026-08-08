# Plan 005: Turn on a Content-Security-Policy and make HTML escaping the default, not a discipline

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/tauri.conf.json frontend/index.html`
> If either file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED (a too-strict CSP blanks the UI; the escaping refactor touches render paths in a file with no tests)
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

The webview ships with `"csp": null` — no Content-Security-Policy at all — and `withGlobalTauri: true` exposes `window.__TAURI__.core.invoke` to every script in the document. The frontend renders strings the app does not control (package names/descriptions/publishers from the npm and PyPI registries and the brew catalog, advisory summaries from OSV and GitHub, release notes from GitHub, raw package-manager stdout/stderr) into the DOM via roughly 25 `innerHTML` sites. Today every one of them is manually escaped with `esc()` — except one, which already slipped (`p.size` in the search results row; currently latent because backends set size to `""`). One future slip means a malicious package description executes script that can call `invoke("run_op", ...)`, which runs `npm i -g <pkg>` — and npm lifecycle scripts execute as the user. The Tauri capability file is already tight (no fs/shell/http plugins), so the reachable surface is the app's 18 commands; a CSP plus escaping-by-construction closes the two remaining doors.

## Current state

- `src-tauri/tauri.conf.json:22-25`:
  ```json
  "security": {
    "csp": null
  }
  ```
  and `tauri.conf.json:10`: `"withGlobalTauri": true`.
- `frontend/index.html` is one file: inline `<style>` block, inline `<script>` block, one `@font-face` for the bundled `vt323.ttf`, one `<img>` of `npstr-logo.svg`. No external resources at runtime (fonts and logo are local files served from the app dir).
- `frontend/index.html:336` — the escape helper (correct; escapes `& < > " '`):
  ```js
  function esc(s){ return String(s).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;").replace(/'/g,"&#39;"); }
  ```
- The one known missing escape, `frontend/index.html:463` (search results row):
  ```js
  '<td class="muted">'+(p.size||"—")+'</td>'+
  ```
  The sibling library row at `:406` escapes the same field: `'<td class="muted">'+esc(t.size)+'</td>'`.
- The renderers that interpolate untrusted (registry/OSV/GitHub/subprocess) strings:
  - `renderRows` (`:368-412`) — `t.name`, `t.description`, `t.publisher`, `t.installed`, `t.latest`, npx drift hint
  - `renderSearchResults` (around `:445-470`) — `p.name`, `p.description`, `p.pkg`, `p.size`
  - `renderFeed` (`:563-611`) — `w.summary` (wire), `it.blurb` (OSV/GitHub), `it.changelog` lines (GitHub release bodies), `removeCmd` output
  - `renderXfers` (`:677-693`) — raw stdout/stderr lines via `esc()`
  - `renderHistory` (`:699-711`) — history fields
- Tauri v2 behavior worth knowing: when `csp` is set (non-null), Tauri augments the policy for its own injected IPC scripts (it appends hashes/nonces for its inline bootstrap). The app's OWN inline `<script>`/`<style>` blocks are not covered by Tauri's nonces, so the policy must allow them: `'unsafe-inline'` in `script-src`/`style-src` is the pragmatic v1 (still blocks remote script loads, `eval`, and remote connect targets, which is most of the value here).
- ES5 style throughout (`var`, string concatenation). A tagged template literal (`` h`...` ``) is ES6; the Tauri webview (WKWebView) supports it fine — the ES5 style is convention, not a hard constraint. Note it in a comment where you introduce the helper.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Backend regression | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | app launches, all four tabs work |
| Webview console | right-click in the app → Inspect Element → Console | no CSP violation errors after Step 1 tuning |

## Scope

**In scope**:
- `src-tauri/tauri.conf.json` (the `security.csp` value only)
- `frontend/index.html` (the `esc` slip at `:463`, the new `h` helper, and the three registry-facing renderers: `renderRows`, `renderSearchResults`, `renderFeed`)

**Out of scope**:
- `withGlobalTauri` — turning it off means importing `@tauri-apps/api` and a build step; that trade-off is the maintainer's call, not this plan's. Record it as follow-up.
- `renderXfers`, `renderHistory`, the menus, modals, and status bar — their `esc()` discipline is currently correct; converting them is optional follow-up once the pattern exists. Do not grow the diff.
- Any Rust file other than `tauri.conf.json`.

## Git workflow

- Branch: `advisor/005-csp-escaping`
- Commits: `fix(security): enable a Content-Security-Policy` then `fix(ui): escape-by-default templating for registry-facing renderers`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Set the CSP

In `src-tauri/tauri.conf.json`, replace `"csp": null` with:

```json
"csp": "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost"
```

Launch the app (`npm run tauri dev`) and open the webview console. Walk every surface: splash → library renders → select a row → Search (run a query) → What's New (expand a card, load a changelog) → Transfers (run one real install) → menus → Preferences → About → Help → Check for updates. Fix any CSP violation the console reports by adding the *narrowest* missing directive (e.g. if Tauri's IPC needs an additional `connect-src` origin on this Tauri version, add exactly that origin). Do NOT add `unsafe-eval`, and do not add any remote (`https://...`) source — the frontend makes no direct network requests; everything goes through `invoke()`.

**Verify**: all app surfaces above work; console shows zero CSP violations; `grep -n '"csp"' src-tauri/tauri.conf.json` shows the policy string (not `null`).

### Step 2: Fix the known missing escape

At `frontend/index.html:463`, wrap the interpolation: `esc(p.size||"—")`.

**Verify**: `grep -n 'p.size' frontend/index.html` → every interpolation of `p.size` into HTML goes through `esc(`.

### Step 3: Add the escape-by-default template helper

Next to `esc()` (`:336`), add:

```js
// Tagged template: every ${} interpolation is HTML-escaped. Interpolate
// pre-built trusted HTML by wrapping it in raw(): h`<td>${raw(btnHtml)}</td>`.
// (ES6 tagged templates; fine in the shipped WKWebView.)
function raw(s){ return {__raw:String(s)}; }
function h(strings){ var out=strings[0]; for(var i=1;i<arguments.length;i++){ var v=arguments[i]; out+=(v&&v.__raw!==undefined)?v.__raw:esc(v==null?"":v); out+=strings[i]; } return out; }
```

**Verify**: temporarily add `console.log(h\`<b>${"<x>"}</b>\`)` in the console of the running app → prints `<b>&lt;x&gt;</b>`. Remove the test line.

### Step 4: Convert the three registry-facing renderers

Convert the HTML string construction in `renderRows`, `renderSearchResults`, and `renderFeed` from `'...'+esc(x)+'...'` concatenation to `` h`...${x}...` `` templates, using `raw()` only for values that are themselves app-built HTML (e.g. the `action` button string, the `drift` hint, the pre-built `body` in `renderFeed`). Work one renderer per commit-able chunk and re-run the app after each. The rendered output must be byte-identical for benign data — this is a mechanical conversion, not a redesign.

Two spots need care:
- Attribute contexts like `data-install="'+esc(p.pkg)+'"` become `data-install="${p.pkg}"` inside `h` — the helper's escaping covers `"` so attributes stay safe.
- Conditional class fragments (`(off?'muted':'')`) are app constants — interpolate them directly (they escape to themselves) or via `raw()`; either is fine.

**Verify** after each renderer: app run — library table renders identically (names, sources, glyphs, pins, buttons work); search results render and Get installs; What's New cards render, expand, and load changelogs. Console: zero errors.

## Test plan

No JS harness exists; verification is the app-run checklist per step plus one adversarial check: temporarily edit a mock — in the running app's console, call the renderer with a crafted value, e.g. set a search result description to a string containing `<img src=x onerror=...>` via the console and re-render; the markup must appear as literal text, never execute. (Do this in the dev console only; do not commit test data.) Backend regression: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `grep -n '"csp": null' src-tauri/tauri.conf.json` → no matches
- [ ] App-run walkthrough (Step 1 list) passes with zero console CSP violations
- [ ] `:463` size cell is escaped
- [ ] `function h(` and `function raw(` exist in `frontend/index.html`; `renderRows`, `renderSearchResults`, `renderFeed` build their HTML via `h`
- [ ] Adversarial console check renders markup inert
- [ ] `cd src-tauri && cargo test` exits 0
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The app renders blank or the splash never dismisses after Step 1 and no console violation explains it (Tauri-version-specific CSP interaction — report the Tauri version from `src-tauri/Cargo.lock` and the symptom).
- The CSP requires `unsafe-eval` or a remote origin to make any feature work — that would mean a dependency this audit did not find; report it.
- A renderer conversion changes visible output for benign data (spacing/entity differences in the table) — report rather than hand-tuning entities.

## Maintenance notes

- Follow-up candidates recorded for the maintainer: convert the remaining `esc()` renderers (`renderXfers`, `renderHistory`, modals) to `h`; consider dropping `withGlobalTauri` in favor of the npm `@tauri-apps/api` import (requires introducing a JS build step — a real trade-off against the single-file frontend); tighten `script-src` from `'unsafe-inline'` to a hash of the single inline script once the file stabilizes.
- Reviewer: any NEW `innerHTML +=` site added after this plan should use `h`; flag raw concatenation in review.
- Plan 011 (render performance) will restructure `renderXfers`; if it lands first, the conversion list here shrinks accordingly.
