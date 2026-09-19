# Plan 036: Check the production CSP invariants in CI so a third CSP defect cannot ship

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- src-tauri/tauri.conf.json frontend/index.html .github/workflows/ci.yml package.json`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests / dx
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

`npm run tauri dev` does NOT enforce the `csp` from `tauri.conf.json`, and in
packaged builds Tauri appends a nonce to `style-src`, which per CSP Level 3
voids `'unsafe-inline'`. Two P1 defects shipped through this gap in v0.1.5 –
v0.1.7: plan 034 (the CSP blocked the app's own font fetch) and plan 035
(inline style attributes silently dropped, leaving the released app dimmed
and unclickable). The repo's own plans index records that promoting the
ad-hoc verification harness into a checked-in test "is unplanned and worth
doing". This plan adds a zero-dependency static checker that fails CI on the
exact invariant classes that caused both defects, so the next CSP-adjacent
change is caught before release instead of after.

## Current state

- `src-tauri/tauri.conf.json:25` - the production CSP:

  ```json
  "csp": "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost"
  ```

- `frontend/index.html` (head stylesheet comment, ~lines 22-37) documents the
  hazard and the existing grep guard:

  ```
  /* NO INLINE style ATTRIBUTES IN THIS FILE. Tauri appends a nonce to the
     style-src directive at runtime, and per CSP Level 3 a nonce makes
     'unsafe-inline' be ignored. ...
     Hiding, sizing and spacing must therefore live in this stylesheet, or be
     set from JS through the CSSOM (el.style.foo = ...), which the CSP does
     not block. The guard is a plain grep. Run it before you commit and expect
     zero hits (the character class keeps this comment from matching itself):
         grep -n 'style=[\"]' frontend/index.html
  ```

- `frontend/index.html` currently has: exactly one `<style>` tag (the head
  stylesheet Tauri nonces), zero inline `style="` attributes, zero inline
  event-handler attributes (all wiring is `addEventListener`), and all
  dynamic styling via CSSOM property sets (`el.style.display = ...`), which
  CSP does not block. The app fetches `/vt323.ttf` from JS (FontFace
  fallback), which needs `connect-src 'self'` - that was the 034 fix.
- `package.json` scripts today: only `demo` and `tauri`. Node 18+ is already
  a prerequisite (CONTRIBUTING.md), and CI's test job runs on `macos-latest`
  where node is present.
- `.github/workflows/ci.yml` test job steps: checkout → rust toolchain →
  rust-cache → Format → Clippy → Tests.
- Commit style (from `git log`): `fix(ui): ...`, `ci: ...`, lowercase,
  conventional-prefix. Branch convention: `advisor/NNN-slug`.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Checker   | `node scripts/check-csp.js`               | exit 0, "all CSP parity checks passed" (or similar) |
| npm script| `npm run check:csp`                       | exit 0              |
| Tests     | `cd src-tauri && cargo test`              | all pass (202 at planning time) |
| Lint      | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `scripts/check-csp.js` (create)
- `package.json` (add one script)
- `.github/workflows/ci.yml` (add one step to the test job)

**Out of scope** (do NOT touch, even though they look related):
- `src-tauri/tauri.conf.json` - this plan checks the CSP, it does not change
  it. Tightening `script-src` (dropping `'unsafe-inline'`) is a separate,
  riskier follow-up that needs packaged-build verification.
- `frontend/index.html` - it currently passes every check; if you find a
  violation, that is a STOP condition (drift), not something to fix here.
- Any browser-based harness (playwright/puppeteer): deliberately not used;
  zero new dependencies is a hard constraint of this plan.

## Git workflow

- Branch: `advisor/036-csp-parity-check`
- Commit per step; message style: `ci: ...` / `test: ...` (see `git log`).

## Steps

### Step 1: Write `scripts/check-csp.js`

A plain Node script (no dependencies, `require("fs")` only) that:

1. Reads `src-tauri/tauri.conf.json`, parses it, and extracts
   `app.security.csp`. Split on `;` into a directive map
   (`directive -> Set of sources`). Fail loudly if the key is missing.
2. **Emulates the packaged build**: append `'nonce-x'` to the `style-src`
   source set. Per CSP Level 3, any nonce in `style-src` makes
   `'unsafe-inline'` be ignored - this is the condition that killed every
   inline style attribute in v0.1.5/v0.1.6. (Evidence from plan 035: only
   styles broke, so script-src nonces are not emulated here.)
3. Runs these checks against `frontend/index.html` (read as text), each with
   a clear failure message naming the line:
   - **No inline style attributes**: scan line by line for
     /style\s*=\s*["']/, EXCLUDING matches inside the head stylesheet's
     comment block (the comment itself documents the guard; simplest correct
     approach: strip `/* ... */` comment blocks before scanning). Expect zero.
   - **Exactly one `<style` occurrence** in the file (the head stylesheet -
     the tag Tauri rewrites with its nonce). A second `<style>` tag created
     in markup would not carry the nonce and would be dead in production.
   - **No inline event-handler attributes**: after stripping comments, scan
     for /\son[a-z]+\s*=\s*["']/ (onclick=, onerror=, ...). Expect zero.
   - **No `javascript:` URLs**: scan for /javascript:/i outside comments.
     Expect zero.
   - **Font fetch covered**: the file references `vt323.ttf` via CSS
     `@font-face` AND a JS `fetch`/FontFace load. Assert `font-src` contains
     `'self'` (covers the CSS load) AND `connect-src` contains `'self'`
     (covers the JS fetch). This is the plan-034 regression class: if anyone
     removes `'self'` from `connect-src`, the packaged app loses its font
     again.
   - **CSP parse sanity**: assert `default-src`, `script-src`, `style-src`,
     `font-src`, and `connect-src` directives all exist.
4. Prints one line per passing check and exits 0, or prints each failure and
   exits 1.

Keep the script self-documenting: a header comment explaining WHY (the two
shipped defects, the dev-mode enforcement gap) so the next maintainer does
not delete it as noise. No em dashes anywhere in the file (repo convention,
see CONTRIBUTING.md).

**Verify**: `node scripts/check-csp.js` → exit 0.

### Step 2: Prove the checker catches both historical defect classes

Temporarily (do not commit these):

1. Add `style="color:red"` to any element in `frontend/index.html` →
   `node scripts/check-csp.js` must exit 1 and name the line. Revert.
2. In a scratch copy of the CSP logic, simulate removing `'self'` from
   `connect-src` (e.g. set an env var or temporarily edit tauri.conf.json) →
   the font-fetch check must exit 1. Revert.

**Verify**: after reverting, `node scripts/check-csp.js` → exit 0 and
`git status` shows no changes to `frontend/index.html` or
`src-tauri/tauri.conf.json`.

### Step 3: Wire into npm scripts and CI

- `package.json`: add `"check:csp": "node scripts/check-csp.js"` to
  `scripts`.
- `.github/workflows/ci.yml`: in the `test` job, add a step BEFORE the Rust
  steps (it is fast and needs no Rust build):

  ```yaml
      - name: CSP parity
        run: npm run check:csp
  ```

**Verify**: `npm run check:csp` → exit 0. Confirm the workflow file parses
(`git diff` review; YAML indentation matches the existing steps).

### Step 4: Full gate

**Verify**: `cd src-tauri && cargo test` → all pass (nothing Rust changed,
this is a sanity gate), and `node scripts/check-csp.js` → exit 0.

## Test plan

The checker is the test. Step 2 is its adversarial verification: both
historical defect classes (inline style attribute, font-fetch connect-src
hole) must be caught in the failing direction. If you want a permanent
self-test, add a `--selftest` flag to the script that runs the checks against
two embedded fixture strings (one violating, one clean) - optional, keep it
small.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `node scripts/check-csp.js` exits 0 on the clean tree
- [ ] Adding a `style="` attribute to `frontend/index.html` makes it exit 1 (verified, then reverted)
- [ ] Removing `'self'` from `connect-src` makes it exit 1 (verified, then reverted)
- [ ] `npm run check:csp` exits 0
- [ ] `.github/workflows/ci.yml` test job contains the CSP parity step
- [ ] `cd src-tauri && cargo test` exits 0
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The current `frontend/index.html` fails any check on the clean tree (the
  codebase has drifted; the fix belongs in its own change, not this one).
- `tauri.conf.json` no longer has `app.security.csp` at the expected shape.
- You find yourself wanting to add a dependency to make the check work. The
  zero-dependency constraint is deliberate; report instead.

## Maintenance notes

- Any future change to the CSP, to inline styles, or to a frontend network
  call must keep this checker green. If a new resource type is added (e.g.
  an `<img>` from a remote URL, a new fetch), extend the checker's coverage
  assertions in the same PR.
- The checker is static; it does not replace a packaged-build smoke test for
  changes to Tauri itself (upgrades of the `tauri` crate can change nonce
  behavior). The release checklist in `scripts/release.sh` remains the last
  line of defense.
- Deliberately deferred: dropping `'unsafe-inline'` from `script-src`
  (security hardening). That requires proving in a packaged build that
  Tauri's runtime script injection does not depend on it; do it as its own
  change with a real `.app` boot test, and let this checker guard the result.
