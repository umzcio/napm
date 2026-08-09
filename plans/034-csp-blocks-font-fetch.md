# Plan 034: The production CSP blocks napm's own font, and the fallback cannot fire

> **Executor instructions**: Follow the plan, run every verification, touch only the in-scope files.
> STOP and report if a STOP condition occurs. Update your row in `plans/README.md` when done.

## Status
- **Priority**: P1 (ships in every packaged build; dev builds cannot reproduce it) | **Effort**: S | **Risk**: LOW
- **Planned at**: `main` @ 23559e5 (tag v0.1.6), 2026-08-09
- **Found by**: reproducing `frontend/index.html` in a browser with the exact production CSP applied

## Why this matters

Tauri injects the `csp` from `tauri.conf.json` into **release builds only**. Dev builds do not enforce
it. So every CSP defect in this app is invisible to `npm run tauri dev` and ships to users. This is
the first one found, and it was found by applying the real CSP to the real page, not by reading it.

Two defects compound into one failure.

### Defect 1: `connect-src` does not allow the app's own origin

`src-tauri/tauri.conf.json`:
```
connect-src ipc: http://ipc.localhost
```

`frontend/index.html:331` fetches the bundled font:
```js
fetch("/vt323.ttf").then(function(r){ if(!r.ok) throw 0; return r.arrayBuffer(); })
```

`fetch` is governed by `connect-src`, not `font-src`. The origin is not listed, so in a packaged
build this is blocked outright. Reproduced verbatim:

```
Connecting to 'http://localhost:8765/vt323.ttf' violates the following Content Security Policy
directive: "connect-src ipc: http://ipc.localhost". The action has been blocked.
```

### Defect 2: the fallback regex targets a string that no longer exists

The `.catch()` at `frontend/index.html:333` is supposed to rescue exactly this case:
```js
.catch(function(){ var s=document.querySelector("style");
  var m=s&&s.textContent.match(/(data:font\/woff2;base64,[A-Za-z0-9+\/=]+)/); if(m) add('url("'+m[1]+'")'); });
```

It scans the stylesheet for a `data:font/woff2;base64,...` URI. There is no longer one.
`grep -c 'data:font/woff2;base64,' frontend/index.html` returns **0**.

The `@font-face` `src` at **:12** is malformed, and reading it explains why:
```css
src:url("/vt323.ttf") format("truetype");base64,d09GMgABAAAAAA00AA4...=") format("woff2");
```
The `;` after `format("truetype")` ends the declaration. What follows is 4508 characters of
orphaned base64 that CSS discards as an invalid declaration. This is the wreckage of an edit that
replaced a `url("data:font/woff2;base64,...")` source with a file reference and left the payload
behind. It is dead weight in the shipped HTML, and it is what silently disarmed the fallback.

### The combined effect

In a packaged build, VT323 loads by no JS path at all. The only remaining route is the CSS
`@font-face`, and the comment directly above this code (**:288-291**) asserts that WKWebView does not
apply a CSS `@font-face` for a local font, which is the entire reason the JS loader exists.

VT323 is the app's identity: the wordmark, the version string, the peer handles, the download
counts, the transfer rates, the history timestamps, and the dial-up splash. Its sizes (20px for the
wordmark, 15px for handles) are tuned for VT323's small x-height, so a fallback renders visibly
larger than intended.

## Scope
**In scope**: `src-tauri/tauri.conf.json` (the `csp` string only), `frontend/index.html` (the
`@font-face` `src` at :12 and the font loader at :328-335).
**Out of scope**: every other CSP directive's intent (see Step 1 before touching them); the rest of
the stylesheet; any other JS; the version fields.

## Steps

### Step 1: Let the app fetch its own assets
Add `'self'` to `connect-src`, keeping `ipc:` and `http://ipc.localhost`.

Do **not** widen any other directive, and do **not** add a wildcard. This CSP is a deliberate
security control from plan 005: the app renders package metadata from third-party registries, so
`script-src` and `default-src` staying tight is the mitigation. Adding `'self'` to `connect-src`
restores the app's ability to read its own bundled files and nothing more.

**Verify**: state in your report which directives you changed and confirm the diff is one line.

### Step 2: Repair the `@font-face` and the fallback so they agree
Decide between two consistent designs and implement one, stating which and why:

- **(a) File only.** Fix `src` to reference `/vt323.ttf` cleanly, delete the 4508 orphaned base64
  characters, and change the `.catch()` fallback to something that can actually work (for example
  `add('url("/vt323.ttf")')`, since `FontFace` with a URL is governed by `font-src`, which already
  allows `'self'`, so it survives even if `connect-src` is ever tightened again).
- **(b) Data URI only.** Restore a real `src:url("data:font/woff2;base64,...")` so the existing
  regex matches, and drop the file fetch.

Prefer (a): the `.ttf` is already bundled and 153KB of base64 inflates every page load.

Whichever you choose, the invariant is that **the fallback must be able to fire**. Add a comment
stating that invariant so the next edit does not silently break it again.

**Verify**: `grep -c 'data:font/woff2;base64,' frontend/index.html` and the regex in the code must
be consistent with each other. If you choose (a), there should be no base64 blob left at all.

### Step 3: Prove it under the real CSP, not in dev
Dev builds do not enforce the CSP, so `npm run tauri dev` **cannot** verify this. Reproduce the
production condition:

1. Copy `frontend/` to the scratchpad.
2. Inject the exact `csp` string from `tauri.conf.json` as
   `<meta http-equiv="Content-Security-Policy" content="...">` in `<head>`, **with `'nonce-abc123'`
   added to `style-src` and `nonce="abc123"` on the `<style>` tag**. Tauri appends a style nonce at
   runtime (see plan 035), so a harness without it does not match production.
3. Serve it over http (the `file:` protocol is blocked by the browser tooling) and open it.
4. Assert **zero** CSP violations in the console, and assert the font actually loaded:
   `document.fonts.check('16px VT323')` is `true`, and `[...document.fonts].map(f => f.family)`
   includes VT323.
5. Repeat with `connect-src` reverted to its current value, and confirm the fallback path now loads
   the font anyway. That second run is the important one: it proves the fallback is armed.

Browser tools are available but deferred; load them in ONE `ToolSearch` call.

Paste both console outputs into your report.

### Step 4: Sweep for other CSP violations while you are set up
You now have the only harness in the project that can see production CSP behaviour. Click through
all four tabs and open the Preferences and About dialogs, and report **every** violation, not just
font ones. Do not fix anything beyond Steps 1 and 2; anything else you find becomes its own plan.

## Commands
| Purpose | Command | Expected |
|---|---|---|
| Config parses | `python3 -c "import json;json.load(open('src-tauri/tauri.conf.json'))"` | no error |
| No orphaned base64 | `grep -c 'format("truetype");base64,' frontend/index.html` | `0` |
| Rust unaffected | `cd src-tauri && cargo test` | 203 passing |

## Git workflow
- Branch: `advisor/034-csp-font` from `main`.
- Commit: `fix(csp): allow the app to fetch its own font, and repair the fallback`.
- Push the branch. Do NOT open a PR and do NOT merge.

## Done criteria
- [ ] `connect-src` includes `'self'`; no other directive widened
- [ ] The `@font-face` `src` is valid and the orphaned base64 is gone (design a), or the data URI is
      restored and the regex matches (design b)
- [ ] Under the real CSP: zero console violations, and `document.fonts.check('16px VT323')` is true
- [ ] With `connect-src` reverted, the fallback still loads VT323 (proving it is armed)
- [ ] Every other CSP violation found in Step 4 is reported, none silently fixed
- [ ] Only the two in-scope files changed (`git diff --stat main..HEAD`)

## STOP conditions
- Adding `'self'` to `connect-src` is not sufficient because the fetch resolves to a different
  origin under `tauri://` or `asset://`. Report the actual origin the packaged app uses rather than
  guessing at a scheme.
- Step 4 turns up a violation that breaks a security-relevant behaviour (anything touching
  `script-src`, or a network call to a registry). Report it immediately and do not fix it here.

## Maintenance notes
- **Dev builds do not enforce the CSP.** Any future change to `tauri.conf.json`'s `csp`, or any new
  `fetch` / `XHR` / font / image source, must be checked with the Step 3 harness. Consider promoting
  that harness into the repo as a checked-in smoke test; that is a separate decision, not this plan.
- Reviewer: confirm no directive other than `connect-src` moved, and that the fallback was proven by
  the reverted-CSP run rather than by inspection.
