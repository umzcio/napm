# Plan 006: Move version comparison and status classification into Rust, making the README's architecture claim true

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/scan/ frontend/index.html README.md CONTRIBUTING.md`
> Note: plan 002 intentionally created `src-tauri/src/scan/version.rs` — that
> change is expected, and this plan builds on it. Any OTHER drift in the
> excerpts below is a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (changes what drives every status glyph and what Update All selects; mitigated by characterization tests written first)
- **Depends on**: plans/002-npx-version-compare-and-manual-size.md (creates `scan/version.rs`)
- **Category**: tech-debt
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

README ("A native Rust backend owns every shell call and all version logic and is the single source of truth") and CONTRIBUTING ("All shell access and version logic live in the Rust backend") both state a rule the code does not follow: the entire semver engine — `verParts`, `verCmp`, `bumpKind`, and `statusOf` — lives as ~6 lines of inline JS in `frontend/index.html`, with zero tests of any kind. These functions decide every status glyph, the "N safe to take" count, what Update All touches, and which packages get a What's New verdict. `verParts` silently truncates to 3 segments and coerces non-numeric parts to 0, so `1.0.0-rc1` compares equal to `1.0.0` and 4-segment pip versions compare equal on their first three segments — and `statusOf` then flags any string-unequal pair as an "update", including downgrades within those blind spots (the README separately promises it "never flags a downgrade as an update"). Moving classification into tested Rust makes the docs true and puts the app's core decision under `cargo test`.

## Current state

- `frontend/index.html:332` — status derivation (JS):
  ```js
  function statusOf(t){ if(t.eco==="manual") return "unmanaged"; if(!t.installed) return "offline"; if(t.installed===t.latest) return "current"; return verCmp(t.latest,t.installed)<0?"current":"update"; }
  ```
- `frontend/index.html:360-365` — the JS version engine:
  ```js
  function verParts(v){ var p=String(v||"").split(".").slice(0,3).map(function(x){ var m=/^\d+/.exec(x); return m?parseInt(m[0],10):0; }); while(p.length<3) p.push(0); return p; }
  function verCmp(a,b){ var x=verParts(a),y=verParts(b); for(var k=0;k<3;k++){ if(x[k]!==y[k]) return x[k]-y[k]; } return 0; }
  function bumpKind(a,b){ if(!a||!b||a===b) return "none"; var x=verParts(a),y=verParts(b);
    if(y[0]!==x[0]) return "major"; if(y[1]!==x[1]) return "minor"; return "patch"; }
  function isSafe(kind){ if(kind==="none") return false; if(APPETITE>=2) return true; if(APPETITE>=1) return kind!=="major"; return kind==="patch"; }
  function safeCount(){ return TOOLS.filter(function(t){ return statusOf(t)==="update" && t.eco!=="npx" && !t.pinned && isSafe(bumpKind(t.installed,t.latest)); }).length; }
  ```
  JS callers of `statusOf`/`bumpKind`: `renderRows` (`:373`, `:388-393`), Update All (`:775`), `verdictScope` (`:514-518`), `installPackage` (`:505`), `safeCount` (status bar).
- `src-tauri/src/scan/mod.rs:14-36` — `InstalledTool` (serialized to the frontend); no status/bump fields today.
- `src-tauri/src/scan/mod.rs:62-77` — `scan_all` aggregates sources and applies pins; the natural place to stamp derived fields.
- After plan 002, `src-tauri/src/scan/version.rs` exists with `pub fn cmp(a, b) -> Ordering` (numeric segments, prerelease sorts below release) and `cmp_opt`.
- `README.md:146` and `README.md:249` state the "all version logic in Rust" claim; CONTRIBUTING "Project conventions" repeats it.
- The appetite dial value (`APPETITE`, 0/1/2) is client UI state persisted in localStorage (`:891-896`); `isSafe` is a 3-line policy mapping over the bump kind. It STAYS in JS (the dial re-classifies live without a backend round trip); the docs get a precise wording instead (Step 5).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0, all pass |
| Targeted | `cd src-tauri && cargo test scan::version` | pass |
| Run the app | `npm run tauri dev` | library renders with correct glyphs |

## Scope

**In scope**:
- `src-tauri/src/scan/version.rs` (extend: `bump_kind`, `status_of` + characterization tests)
- `src-tauri/src/scan/mod.rs` (two new serialized fields on `InstalledTool`, stamped in `scan_all`)
- `frontend/index.html` (thin out `statusOf`/`bumpKind`/`verCmp`/`verParts` to read the backend fields)
- `README.md`, `CONTRIBUTING.md` (wording fix)

**Out of scope**:
- `isSafe`/`safeCount`/the appetite dial — stays in JS by design (see Current state).
- The npx drift comparison (`frontend/index.html:881`, `r.latest!==t.installed`) — a string-inequality display hint, correct as is.
- `reference/scanner.js` — legacy reference, not a product surface.

## Git workflow

- Branch: `advisor/006-version-logic-to-rust`
- Commits: `feat(scan): status and bump classification in Rust` then `refactor(ui): read backend status/bump fields` then `docs: version-logic claim now precise`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Characterization tests first

In `scan/version.rs` tests, encode the CURRENT JS behavior for the cases that must not change (normal semver triples), and the intended fixes for the blind spots. Table-driven over `(installed, latest, expected_status, expected_bump)` — cases in the Test plan. Write `status_of` and `bump_kind` signatures now so tests compile, with `todo!()` bodies if needed.

### Step 2: Implement `status_of` and `bump_kind` in Rust

In `scan/version.rs`:

```rust
/// Status for a library row: "unmanaged" (manual), "offline" (not installed),
/// "current", or "update". Uses cmp(); never flags latest <= installed as an update.
pub fn status_of(eco: &str, installed: Option<&str>, latest: &str) -> &'static str { ... }

/// "major" | "minor" | "patch" | "none" from the first differing numeric segment.
/// "none" when either side is empty or the versions are equal under cmp().
pub fn bump_kind(installed: &str, latest: &str) -> &'static str { ... }
```

Semantics: `status_of` mirrors the JS ordering of checks (manual → unmanaged; no/empty installed → offline; `cmp(latest, installed) <= 0` → current; else update). With plan 002's `cmp`, prerelease and 4-segment versions now compare correctly, which is the intended behavioral fix. npx is NOT special-cased here (the frontend renders npx rows specially, but their status_of result is "current" naturally because npx scan sets `latest` = `installed`).

**Verify**: `cd src-tauri && cargo test scan::version` → all characterization tests pass.

### Step 3: Stamp derived fields in `scan_all`

Add to `InstalledTool` (`scan/mod.rs`):

```rust
/// Derived: "current" | "update" | "offline" | "unmanaged". Computed in scan_all.
pub status: String,
/// Derived: "major" | "minor" | "patch" | "none". Computed in scan_all.
pub bump: String,
```

Give every scanner-constructed `InstalledTool` default empty strings (compiler will point at each construction site — `scan/npm.rs`, `brew.rs`, `pip.rs`, `npx.rs`, `manual.rs`; fill with `String::new()`), then in `scan_all` after the pins loop:

```rust
for row in all.iter_mut() {
    row.status = version::status_of(&row.eco, row.installed.as_deref(), &row.latest).to_string();
    row.bump = version::bump_kind(row.installed.as_deref().unwrap_or(""), &row.latest).to_string();
}
```

Add a `scan_all`-level test: with all sources disabled it returns empty; construct rows manually and assert status/bump stamping (see Test plan).

**Verify**: `cd src-tauri && cargo test` → exit 0.

### Step 4: Thin the frontend

In `frontend/index.html`:
- `statusOf(t)` becomes `function statusOf(t){ return t.status; }` (keep the function so ~10 call sites don't change).
- `bumpKind(a,b)` call sites that operate on a tool's own `(installed, latest)` pair — `:388`, `:517`, `:775`, `safeCount` — switch to `t.bump` (keep a `bumpOf(t){return t.bump;}` helper if cleaner). Delete `verParts`/`verCmp`/`bumpKind` once no caller remains. `isSafe` stays, now taking `t.bump`.
- `installPackage` (`:508`) pushes a synthetic row for a not-yet-installed package: set `status:"offline"`, `bump:"none"` on it explicitly.

**Verify**: `npm run tauri dev` — glyphs/labels match pre-change behavior for normal versions; a pip tool with a 4-segment version and any prerelease-version tool now classify correctly (spot-check whatever is present in the local library). `grep -n "function verParts" frontend/index.html` → no matches.

### Step 5: Fix the docs to be precise

In `README.md` (both `:146` area and the Design Decisions paragraph) and CONTRIBUTING "Project conventions": the claim becomes "all shell access, version comparison, and status classification live in the Rust backend; the frontend's only version-adjacent logic is the appetite dial's safe/held mapping over the backend-computed bump kind." Keep the no-em-dash rule in whatever sentence you write.

**Verify**: `grep -n "all version logic" README.md CONTRIBUTING.md` → no stale absolute claim remains.

## Test plan

Characterization table in `scan/version.rs` (minimum):
- `("npm", Some("1.0.0"), "1.0.0")` → current, none
- `("npm", Some("1.0.0"), "1.0.1")` → update, patch
- `("npm", Some("1.0.0"), "1.2.0")` → update, minor
- `("npm", Some("1.0.0"), "2.0.0")` → update, major
- `("npm", None, "1.0.0")` → offline
- `("manual", Some("1.0.0"), "1.0.0")` → unmanaged
- `("npm", Some("2.0.0"), "1.9.0")` → current (downgrade never an update)
- `("pip", Some("1.2.3.5"), "1.2.3.4")` → current (4-segment blind spot fixed)
- `("pip", Some("1.2.3.4"), "1.2.3.5")` → update, patch (4th-segment bumps count as patch — document this choice in the code)
- `("npm", Some("2.0.0"), "2.0.0-rc.1")` → current (prerelease of same version is not an upgrade)
- `("npm", Some("2.0.0-rc.1"), "2.0.0")` → update, patch (same numeric triple: define as patch; document)
- `scan_all` test: all-sources-off returns empty (also covers the source gating from the audit's TEST-02).

**Verification**: `cd src-tauri && cargo test` → exit 0, all pass.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; the characterization table above exists and passes
- [ ] `grep -n "verParts\|verCmp(" frontend/index.html` → no matches
- [ ] `InstalledTool` serializes `status` and `bump`; app run confirms glyphs unchanged for normal versions
- [ ] README and CONTRIBUTING no longer claim ALL version logic is in Rust while `isSafe` remains in JS; new wording is accurate
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- Plan 002 has not landed (no `scan/version.rs`) — execute 002 first, do not inline a second comparator.
- The frontend uses `verCmp` somewhere not listed in Current state (`grep -n "verCmp(" frontend/index.html` first — the call-site list must be complete).
- App-run shows different glyphs for NORMAL semver triples than before the change (that is a regression, not a blind-spot fix).

## Maintenance notes

- Any future ecosystem (cargo, plan 022) gets status/bump for free via `scan_all`.
- Reviewer: the two "document this choice" cases (4th-segment bump, prerelease-to-release bump) are policy decisions this plan makes explicit — confirm the maintainer agrees.
- Deferred: making `verdict_scope` computation fully backend-side (the frontend still filters by appetite before calling `get_whats_new`); revisit if the appetite dial ever moves server-side.
