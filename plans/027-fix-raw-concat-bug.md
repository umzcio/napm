# Plan 027: Fix the raw() concatenation bug in the uninstall modal, and make the bug class impossible

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. Touch only the files listed as in scope. If any
> STOP condition occurs, stop and report. When done, update the status row in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat advisor/025-import..HEAD -- frontend/index.html`

## Status
- **Priority**: P1 (user-visible defect in a safety dialog) | **Effort**: S | **Risk**: LOW | **Category**: bug
- **Depends on**: branches from `advisor/025-import` (the current chain tip)
- **Planned at**: chain tip after plan 025, 2026-08-08
- **Found by**: the plan-025 executor, during self-review of its own identical mistake. Confirmed by the reviewer.

## Why this matters

Plan 005 introduced an escaping tagged template `h` plus a `raw()` marker for pre-built trusted HTML.
`raw()` returns an object `{__raw: "..."}`, and `h` unwraps it. But `raw()` only works when the value
is **interpolated inside an `h` template**. In plain `+` string concatenation the object stringifies
via the default `Object.prototype.toString`, producing the literal text `[object Object]`.

`renderUninstallModal` (added by plan 023) concatenates three `raw()` values:

```js
raw(pinLine)+raw(depsLine)+raw(reasonLine)+checkingLine+
```

Reproduced (node):
```
inside h template:    <b><i>ok</i></b>
plain concatenation:  <b>[object Object]</b>
```

So today, in the uninstall confirm dialog, three lines render as `[object Object]`:
- the **pin note** ("This tool is pinned. Uninstalling will also unpin it.")
- the **brew dependents warning** (the safety-critical explanation of why Uninstall is blocked)
- the **self-toolchain exclusion reason** (why removing npm/pip itself is refused)

These are exactly the safety affordances design decisions 2 and 6 of the uninstall work exist to
provide. The dialog still functions, but its warnings are unreadable garbage.

This slipped through review because the JS syntax gate cannot catch it (the code is valid JS),
there are no frontend tests, and GUI verification is deferred to human QA.

## Current state
- `frontend/index.html`, `renderUninstallModal`: the concatenation line above. The three inputs are
  each built with `h` templates (so their interpolated values, including brew's dependent names from
  `brew uses --installed`, are ALREADY escaped) and are plain strings. `checkingLine` is a plain
  string literal and is correct as-is.
- `frontend/index.html`, the helper (near `esc`):
  ```js
  function raw(s){ return {__raw:String(s)}; }
  function h(strings){ var out=strings[0]; for(var i=1;i<arguments.length;i++){ var v=arguments[i]; out+=(v&&v.__raw!==undefined)?v.__raw:esc(v==null?"":v); out+=strings[i]; } return out; }
  ```
- A repo-wide grep for the pattern found exactly ONE occurrence:
  `grep -nE '(\+\s*raw\(|raw\([^)]*\)\s*\+)' frontend/index.html` → line ~1290 only.

## Commands you will need
| Purpose | Command | Expected |
|---|---|---|
| JS gate | `awk '/<script>/{f=1;next}/<\/script>/{f=0}f' frontend/index.html > /tmp/s.js && node --check /tmp/s.js` | exit 0 |
| Bug-pattern grep | `grep -nE '(\+\s*raw\(\|raw\([^)]*\)\s*\+)' frontend/index.html` | no matches after the fix |
| Backend regression | `cd src-tauri && cargo test` | exit 0, 201 |

## Scope
**In scope**: `frontend/index.html` only (the `raw` helper and the one concatenation site).
**Out of scope**: any other renderer; any behavior change to the uninstall flow itself; adding a
frontend test harness (a real gap, but a separate decision).

## Git workflow
- Branch: `advisor/027-raw-concat-fix` from `advisor/025-import`.
- Commit: `fix(ui): raw() survives string concatenation, fixing uninstall modal warnings`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Make the bug class impossible
Give the `raw()` marker a `toString` so it renders correctly in BOTH contexts:
```js
// Marks pre-built trusted HTML. `h` unwraps __raw when interpolated; toString
// keeps it correct if the value is ever used in plain string concatenation
// instead (which otherwise yields "[object Object]").
function raw(s){ var v=String(s); return {__raw:v, toString:function(){ return v; }}; }
```
`h`'s existing `v.__raw!==undefined` check runs first, so interpolation behavior is unchanged.
**Verify**: with node, confirm BOTH forms now produce the same HTML:
```
node -e '<paste esc/h/raw>; console.log(h`<b>${raw("<i>x</i>")}</b>`); console.log("<b>"+raw("<i>x</i>")+"</b>");'
```
→ both print `<b><i>x</i></b>`.

### Step 2: Drop the unnecessary wrappers at the call site
In `renderUninstallModal`, change `raw(pinLine)+raw(depsLine)+raw(reasonLine)+checkingLine+` to
`pinLine+depsLine+reasonLine+checkingLine+`. These are already `h`-built escaped strings; wrapping
them in `raw()` for a concatenation context was the mistake. (Step 1 would make the old form work
too, but the direct form is what the surrounding code does and is clearer.)
**Verify**: `grep -nE '(\+\s*raw\(|raw\([^)]*\)\s*\+)' frontend/index.html` → no matches.
JS gate → exit 0.

### Step 3: Confirm no other instance exists
Re-run the pattern grep across the whole file and also check for a `raw(` value assigned to a
variable that is later concatenated (e.g. `var x = raw(...)` then `"..."+x`).
**Verify**: report the grep output. If you find another instance, fix it the same way and say so.

## Test plan
No automated frontend tests exist (the gap that let this through). Verification is the node
two-context check in Step 1, the pattern grep, the JS syntax gate, and the backend regression run.
**HUMAN VERIFICATION** (reproduce in your report): with the app running, open the uninstall confirm
modal for (a) a pinned tool → the pin note reads as English, not `[object Object]`; (b) a brew
formula with an installed dependent → the "in use: X still depends on this formula" warning reads
correctly and Uninstall is disabled; (c) `npm` itself → the self-toolchain reason reads correctly.

## Done criteria
- [ ] `grep -nE '(\+\s*raw\(|raw\([^)]*\)\s*\+)' frontend/index.html` → no matches
- [ ] The node two-context check prints identical correct HTML for both forms
- [ ] JS gate passes; `cd src-tauri && cargo test` exits 0 (201)
- [ ] Only `frontend/index.html` changed (`git diff --stat advisor/025-import..HEAD`)
- [ ] Human checklist reproduced in the report
- [ ] `plans/README.md` status row updated

## STOP conditions
- The `h`/`raw` helpers differ from the excerpt above (drift).
- Adding `toString` changes any existing `h` interpolation output (it must not — the `__raw` check
  short-circuits first; if you observe a difference, report it).

## Maintenance notes
- Root cause worth remembering: a marker object that only works in one syntactic context is a trap.
  The `toString` makes both contexts correct, so the next person cannot reintroduce it.
- The deeper gap is that the frontend has no automated tests, so this class of defect is only
  caught by reading or by GUI QA. Extracting the inline script into a testable module was
  considered in the original audit and deferred as a maintainer decision; this bug is evidence for
  revisiting it.
- Reviewer: confirm the three warning lines are `h`-built (escaped) before accepting the unwrapped
  concatenation, so the fix does not open an injection path for brew's dependent names.
