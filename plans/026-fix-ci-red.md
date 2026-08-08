# Plan 026: Get CI green on PR #7 (clippy toolchain drift + audit advisory triage)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. Touch
> only the files listed as in scope. If any STOP condition occurs, stop and
> report. When done, update the status row in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat ac3389e..HEAD -- src-tauri/src/intel/release.rs .github/workflows/ci.yml`

## Status
- **Priority**: P1 (blocks 023/024/025 and PR #7) | **Effort**: S | **Risk**: LOW | **Category**: dx
- **Depends on**: none (branches from the chain tip `ac3389e`)
- **Planned at**: commit `ac3389e`, 2026-08-08

## Why this matters

PR #7's CI is red on both jobs. Neither is a defect in the audited code, but both must be
green before more work stacks on this chain, and a red CI trains people to ignore CI.

1. **`test` job (clippy)**: The workflow uses `dtolnay/rust-toolchain@stable`, which resolved to
   Rust **1.97.0** on the runner. The machine this chain was developed on is **1.96.0**. Clippy
   1.97 tightened `clippy::question_mark`, which now fires on a `match` in the plan-013 strict
   URL parser. With `-D warnings` the build fails. The code is correct; only the lint is new.
   Exact CI error:
   ```
   error: this `match` expression can be replaced with `?`
      --> src/intel/release.rs:146:29
   146 |       let (authority, path) = match rest.find('/') {
   147 |         Some(i) => (&rest[..i], &rest[i + 1..]),
   148 |         None => return None,
   149 |     };
   = note: `-D clippy::question-mark` implied by `-D warnings`
   ```
2. **`audit` job**: `rustsec/audit-check` fails on transitive advisories in the Tauri dependency
   tree, notably **RUSTSEC-2024-0429** (`glib 0.18.5` unsoundness in `VariantStrIter`) and an
   `anyhow` unsoundness advisory. `glib` is a GTK/Linux crate: it is in `Cargo.lock` because Tauri
   declares it for Linux targets, but napm ships macOS-only, so it is not in the shipped binary.
   These are not actionable by this project beyond waiting for upstream Tauri bumps, which is
   exactly what plan 001's maintenance note anticipated ("may produce initial noise from
   unmaintained-crate advisories in the Tauri tree, which can be triaged with `--ignore` entries
   carrying a reason").

## Current state
- `.github/workflows/ci.yml`: `test` job on `macos-latest` running fmt/clippy/test; `audit` job on
  `ubuntu-latest` using `rustsec/audit-check@v2` with `working-directory: src-tauri`. Both added by
  plan 001. No toolchain pin, no audit ignore config.
- `src-tauri/src/intel/release.rs` around line 146: the `github_repo_from_url` authority/path split
  (added by plan 013). The immediately preceding `match rest.find(['?', '#'])` has a `None => rest`
  arm and does NOT trigger the lint; only the `None => return None` one does.
- `src-tauri/Cargo.toml` has `rust-version = "1.77.2"` (MSRV), unrelated to the CI toolchain choice.
- Local toolchain: rustc/clippy 1.96.0. CI stable: 1.97.0.

## Commands you will need
| Purpose | Command | Expected |
|---|---|---|
| Clippy (local, 1.96) | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 |
| Tests | `cd src-tauri && cargo test` | exit 0, 147 |
| fmt | `cd src-tauri && cargo fmt --all -- --check` | exit 0 |
| Audit (if installed) | `cd src-tauri && cargo audit` | reports the advisories; may not be installed locally |

## Scope
**In scope**: `src-tauri/src/intel/release.rs` (the one lint), `.github/workflows/ci.yml` (toolchain
pin + audit ignores), and an audit config file if that is the mechanism you choose
(e.g. `src-tauri/.cargo/audit.toml`).
**Out of scope**: any behavior change; upgrading Tauri or any dependency; the MSRV in Cargo.toml;
suppressing the lint with `#[allow]` (fix it properly — the suggested rewrite is equivalent).

## Git workflow
- Branch: `advisor/026-ci-green` from `advisor/018-disclosure` (ac3389e).
- Commits: `fix(intel): use ? instead of match in the URL authority split` and
  `ci: pin the toolchain and triage transitive advisories`.
- Do NOT push or open a PR unless the operator asks (the operator WILL ask you to push this one;
  wait for the instruction in your dispatch message).

## Steps

### Step 1: Fix the clippy lint properly
Apply clippy's own suggestion in `src-tauri/src/intel/release.rs`:
```rust
let (authority, path) = {
    let i = rest.find('/')?;
    (&rest[..i], &rest[i + 1..])
};
```
This is behavior-identical (the function already returns `Option`). Do NOT use `#[allow]`.
**Verify**: `cd src-tauri && cargo clippy --all-targets -- -D warnings` → exit 0 on the LOCAL 1.96
toolchain, AND `cargo test` → 147 pass (the `github_repo_from_url` tests from plan 013 must all
still pass, including the rejection cases).

### Step 2: Make CI's linter version deterministic
The root cause is a floating `stable` that silently changes the lint set under `-D warnings`.
Pin the toolchain in `.github/workflows/ci.yml`'s `test` job to an explicit version so a new
clippy release cannot turn the build red without a deliberate bump:
```yaml
      - uses: dtolnay/rust-toolchain@1.97.0
        with:
          components: rustfmt, clippy
```
Add a short comment above it explaining that the pin is deliberate (a floating stable makes
`-D warnings` non-deterministic) and that bumping it is a normal maintenance PR.
**Verify**: the YAML parses (`python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/ci.yml'))"`).

### Step 3: Triage the audit advisories with reasons
Configure the audit job to ignore ONLY the specific advisories that are transitive and
not-applicable, each with a written reason, so a NEW advisory still fails the build.
Determine the correct mechanism for `rustsec/audit-check@v2` before choosing (read its README via
WebFetch: https://github.com/rustsec/audit-check). Two viable shapes:
- the action's `ignore` input listing advisory ids, or
- a `src-tauri/.cargo/audit.toml` with `[advisories] ignore = [...]`.
Whichever you use, record for each id a one-line reason in a comment or alongside, e.g.:
- `RUSTSEC-2024-0429` (`glib` unsoundness): GTK/Linux-only transitive dep of Tauri; napm ships
  macOS-only, so this code is not in the shipped binary. Revisit when Tauri bumps glib.
- the `anyhow` advisory: transitive; unreachable from napm's own code paths. Revisit on the next
  Tauri dependency bump.
Include ONLY advisories that are actually firing today. Do not add blanket ignores, do not disable
the job, and do not ignore an advisory that affects a crate napm calls directly (`ureq`, `serde`,
`serde_json`, `tauri`, `minisign-verify`) — if one of those is in the failing set, STOP and report
instead of ignoring it.
**Verify**: list the exact advisory ids you ignored and the reason for each in your report.

## Test plan
No new unit tests (Step 1 is a mechanical rewrite covered by plan 013's existing
`github_repo_from_url` tests; Steps 2-3 are CI config). The verification is that the three local
gates stay green and the CI config is valid.

## Done criteria
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` exits 0 locally
- [ ] `cd src-tauri && cargo test` exits 0 (147 tests) and `cargo fmt --all -- --check` is clean
- [ ] `grep -n "question_mark\|#\[allow" src-tauri/src/intel/release.rs` → no allow was added for this lint
- [ ] The workflow pins an explicit toolchain version with an explanatory comment
- [ ] Audit ignores are advisory-id-specific, each with a written reason; no blanket ignore; the job is still present and still fails on NEW advisories
- [ ] Only the in-scope files changed (`git diff --stat advisor/018-disclosure..HEAD`)
- [ ] `plans/README.md` status row updated

## STOP conditions
- A failing advisory affects a crate napm depends on DIRECTLY (`ureq`, `serde`, `serde_json`,
  `tauri`, `tauri-plugin-updater`, `tauri-plugin-log`, `minisign-verify`, `log`) — report it as a
  real finding instead of ignoring it.
- Fixing the clippy lint changes any `github_repo_from_url` test result.
- Clippy 1.96 (local) reports a DIFFERENT lint after your edit — report rather than chasing lints
  across two toolchain versions.

## Maintenance notes
- Bumping the pinned toolchain is a deliberate maintenance PR; expect a small clippy-fix diff with
  each bump. That is the tradeoff for a deterministic `-D warnings` gate.
- The audit ignores are a snapshot; each should be removed when the upstream Tauri bump lands.
- Reviewer: confirm the ignore list is id-specific and reasoned, not a blanket suppression.
