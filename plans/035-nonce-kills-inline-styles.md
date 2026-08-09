# Plan 035: Tauri's style nonce disables every inline style attribute in packaged builds

> **Executor instructions**: Follow the plan, run every verification, touch only the in-scope files.
> STOP and report if a STOP condition occurs. Update your row in `plans/README.md` when done.

## Status
- **Priority**: P1 (visible in every shipped build since the CSP landed; two stray artifacts on screen right now) | **Effort**: M | **Risk**: MEDIUM (touches 51 sites across the UI)
- **Planned at**: `main` @ 23559e5 (tag v0.1.6), 2026-08-09
- **Found by**: reproducing the packaged CSP exactly, after a maintainer report of a gray bar visible in the release build but not in dev

## The bug, and the proof

A small gray bar sits at the top-left of the window in packaged builds. In v0.1.5 screenshots a
second thin dark bar sat in the middle of the window. Neither appears in `npm run tauri dev`.

**Root cause.** Tauri appends a nonce to the `style-src` directive at runtime. The token is visible
in the built binary:
```
$ strings -a src-tauri/target/debug/bundle/macos/napm.app/Contents/MacOS/napm | grep style-src
style-src__TAURI_STYLE_NONCE__
```
Per CSP Level 3, **when a nonce-source or hash-source is present in a directive, `'unsafe-inline'`
is ignored.** Tauri adds the matching `nonce` attribute to the `<style>` element, so the stylesheet
keeps working. But **a `style` attribute cannot carry a nonce**, so every inline `style="..."`
attribute in `frontend/index.html` is blocked. There are 51 of them
(`grep -c 'style="' frontend/index.html`).

Dev builds never enforce the CSP, which is why this was invisible locally and shipped.

**Reproduced exactly.** Serving `frontend/index.html` with the production CSP plus
`'nonce-abc123'` in `style-src` and `nonce="abc123"` on the `<style>` tag, at a 1200px viewport:

| Element | Measured | Renders as |
|---|---|---|
| `#menuPop` | `3,3 180x6`, `display=block` | the gray bar, at the window's content-box top-left, over the titlebar |
| `#modalBox` | `440,522 320x2` | the thin centered bar seen in v0.1.5 |

Those match the release build. Measuring the real v0.1.6 window from a screen capture put the gray
bar at roughly 178 x 6 logical px at the window's top-left corner, and the v0.1.5 centre bar at
about 325 px wide, horizontally centred.

**Why those two specifically.** `#menuPop` and `#modalBack` are hidden *only* by an inline
`style="display:none"` in the markup. With the attribute blocked they fall back to the stylesheet,
where `.menu-pop` is `position:absolute; min-width:180px; padding:2px; border:1px` and `.modal` is
`min-width:320px`. `.menu-pop` has no `top`/`left` until JS sets them, so it lands at its static
position, which for an absolutely-positioned child of a flex container (`.window` is
`display:flex`) is the container's content-box start corner: the top-left, inside the 3px padding.
Empty, it is 180 x 6. That is the bar.

`#xferBadge` also carries `style="display:none"` yet is correctly hidden, which is not a
counterexample: `frontend/index.html:804` sets it through the CSSOM (`badge.style.display=...`), and
CSSOM mutations are not subject to CSP. That is also why the app is otherwise fully functional and
why the splash still dismisses. **Only the attributes written in the HTML are dead.**

## What else is broken (do not assume it is only the two bars)

All 51 inline style attributes are inert in production. Spot-check these before and after:
- **:275** `#appetiteLabel` loses `display:inline-block; min-width:88px`, so the status bar text
  reflows as the label changes.
- **:1248** the Preferences GitHub-token input loses `width:100%; margin-top:4px`.
- **:1369** the import textarea loses its entire sizing block.
- Row-level `style="font-size:11px"` on the npx drift note.

## Scope
**In scope**: `frontend/index.html` (inline `style` attributes and the CSS rules that replace them).
**Out of scope**: `src-tauri/tauri.conf.json`'s `csp` (see STOP conditions; do **not** try to fix
this by weakening the CSP); the nonce mechanism itself; plan 034's `connect-src` change; any Rust.

## Approach

### Step 1: Fix the two visible artifacts first, and verify
Replace the `style="display:none"` on `#menuPop` (**:244**) and `#modalBack` (**:317**) with a
stylesheet-driven hidden state, for example a `.hidden{display:none}` class or a default
`display:none` on `.menu-pop` and `.modal-back` that the existing JS overrides through the CSSOM.

Check the JS that shows and hides them still works: `closePopup` (**:1221**), `openMenu` (**:1238**),
and every `modalBack.style.display="flex"` site (**:1299, :1339, :1433, :1481, :1493, :1595, :1602,
:1609**) plus `closeModal` (**:1274**). If you make `display:none` a stylesheet default, an inline
`display:flex` set by JS still wins, but confirm rather than assume, and confirm the popup still
positions correctly once `left`/`top` are set.

**Verify with the harness in Step 3 before going further.** The two bars must be gone.

### Step 2: Sweep the remaining inline styles
Move the other 49 into the stylesheet as classes. Prefer named classes over utility soup, and match
the existing CSS conventions in this file.

Where an inline style is genuinely dynamic (a computed width, a percentage that changes at runtime),
it must be set through the CSSOM from JS instead, since that is not blocked. Note in your report
which ones you converted to classes and which had to become CSSOM assignments.

Do **not** change any visual result. This is a mechanism change, not a restyle.

### Step 3: The harness (this is the only way to see the bug)
`npm run tauri dev` **cannot** reproduce this. Build a repro in the scratchpad, not the repo:

1. Copy `frontend/` to the scratchpad.
2. Inject the `csp` string from `tauri.conf.json` as a `<meta http-equiv="Content-Security-Policy">`,
   **with `'nonce-abc123'` added to `style-src`**.
3. Add `nonce="abc123"` to the `<style>` tag, so the stylesheet survives exactly as Tauri arranges it.
   Both halves are required: without the nonce in the CSP the bug does not reproduce, and without the
   nonce on the tag the whole stylesheet is blocked and everything looks broken for the wrong reason.
4. Serve over http (`file:` is blocked by the browser tooling) and open it.
5. Assert `#menuPop` and `#modalBack` compute to `display:none`, and that the spot-checks above have
   their intended geometry.

Browser tools are deferred; load them in ONE `ToolSearch` call.

### Step 4: Make the bug class impossible to reintroduce
Add a comment at the top of the `<style>` block stating that inline `style` attributes do not work in
packaged builds because Tauri's style nonce voids `'unsafe-inline'`, and that hiding or sizing must
live in the stylesheet or be set through the CSSOM.

Then add a grep-able guard and report what you chose: the cheapest is a documented one-liner in
CONTRIBUTING (`grep -c 'style="' frontend/index.html` should stay at 0). Do not add a build step.

## Commands
| Purpose | Command | Expected |
|---|---|---|
| No inline styles left | `grep -c 'style="' frontend/index.html` | `0` (or a justified, documented count) |
| Rust unaffected | `cd src-tauri && cargo test` | 203 passing |

## Git workflow
- Branch: `advisor/035-inline-styles` from `main`.
- Commit: `fix(ui): move inline styles into the stylesheet, which the CSP nonce disables`.
- Push the branch. Do NOT open a PR and do NOT merge.

## Done criteria
- [ ] Under the nonce harness, `#menuPop` and `#modalBack` compute to `display:none`; neither bar renders
- [ ] The menu popup and every modal still open, position, and close correctly
- [ ] `grep -c 'style="' frontend/index.html` is 0, or every remaining instance is justified in your report
- [ ] The spot-checked elements keep their intended geometry under the harness
- [ ] A comment records why inline styles do not work here
- [ ] Only `frontend/index.html` changed (`git diff --stat main..HEAD`)

## STOP conditions
- You are tempted to remove `'unsafe-inline'` handling by weakening the CSP, or to strip Tauri's
  nonce. Do not. The CSP is a deliberate control from plan 005 and this app renders third-party
  registry metadata. Report instead.
- Converting an inline style to a class changes the rendered result anywhere. Report the difference
  rather than accepting it.

## Maintenance notes
- **Every CSP-affected defect is invisible in dev.** Plan 034 covers a second one found the same way
  (`connect-src` blocking the app's own font fetch). Anything touching the CSP, inline styles, or a
  new network call needs the Step 3 harness. Promoting that harness into the repo as a checked-in
  smoke test is worth its own plan.
- Reviewer: verify with the harness, not with `tauri dev`, and confirm the CSP itself was not touched.
