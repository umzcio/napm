# Plan 001: Establish CI and a formatting/lint/audit baseline

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- CONTRIBUTING.md .github/ src-tauri/Cargo.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

The repo has 81 Rust tests (78 unit tests in `#[cfg(test)]` modules plus 3 in `src-tauri/tests/updater_signature.rs`) and nothing runs any of them automatically. The most recent commit (`bb85e05`) added Dependabot, so weekly automated dependency PRs now arrive into a repo with zero automated verification. Worse, the only verification command CONTRIBUTING tells contributors to run is `cargo test --lib`, which excludes the integration test that proves updater signature verification works (valid signature verifies, tampered payload rejected) — the trust boundary of the auto-update system. This plan makes one command the verification story locally and in CI, and gets formatting/lint to a state where CI can gate on them.

## Current state

- `.github/` contains exactly one file: `dependabot.yml` (weekly grouped cargo + npm updates). There is no `workflows/` directory.
- `CONTRIBUTING.md:19-24` says:
  ```
  Run the backend tests before opening a pull request:

  cd src-tauri
  cargo test --lib
  ```
  `--lib` runs only the 78 in-crate unit tests and skips `src-tauri/tests/updater_signature.rs`.
- `src-tauri/Cargo.toml` has no `[lints]` table; there is no `rustfmt.toml` or `clippy.toml` anywhere. Formatting has drifted from rustfmt in places (e.g. `src-tauri/src/lib.rs:236` is one very long `generate_handler!` line, and the `run()` body at `lib.rs:230-262` is 2-space indented while most of the crate is 4-space), so `cargo fmt --check` will fail today.
- Clippy has been run manually at least once (`src-tauri/src/ops.rs:57` has `#[allow(clippy::too_many_arguments)]`) but nothing gates on it.
- There is no JS build step: `src-tauri/tauri.conf.json` sets `"frontendDist": "../frontend"` with no `beforeBuildCommand`. CI therefore needs only a Rust toolchain; it does NOT need to build the Tauri app bundle — `cargo test` compiles the crate (including the `tauri` dependency) and runs the tests.
- The project targets macOS only (README, CONTRIBUTING). Use a `macos-latest` runner so `cfg(target_os = "macos")` dependencies (`tauri-plugin-updater`) compile.
- Repo conventions: commit messages are conventional-commit flavored, e.g. `fix(ui): window control glyphs were white on gray`, `chore: add Dependabot config for cargo (src-tauri) and npm`. Documentation must not contain em dashes (CONTRIBUTING "No em dashes" rule).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests (all) | `cd src-tauri && cargo test` | exit 0, 81 tests pass (78 lib + 3 integration) |
| Format check | `cd src-tauri && cargo fmt --all -- --check` | exit 0 after Step 2 |
| Lint | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 after Step 3 |
| Advisory audit (optional local) | `cargo audit` (run in `src-tauri/`) | exit 0 or a report; tool may not be installed locally |

## Scope

**In scope** (the only files you should modify):
- `CONTRIBUTING.md`
- `src-tauri/**/*.rs` (formatting and clippy fixes ONLY — no behavior changes)
- `.github/workflows/ci.yml` (create)

**Out of scope** (do NOT touch):
- `frontend/index.html` — no JS lint exists yet; that is a separate decision (see plans index).
- `scripts/` — release tooling is covered by plan 015.
- Any functional change to Rust code. If a clippy fix would change behavior (not just style), use a targeted `#[allow(...)]` with a one-line reason comment instead.

## Git workflow

- Branch: `advisor/001-ci-baseline`
- Separate commits per step: the rustfmt baseline MUST be its own commit (`chore: rustfmt baseline`) so it does not bury the CI workflow diff.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Fix the documented verification command

In `CONTRIBUTING.md`, change `cargo test --lib` to `cargo test`, and add one sentence noting this includes the updater signature integration tests.

**Verify**: `grep -n "cargo test" CONTRIBUTING.md` → shows `cargo test` with no `--lib` flag.

### Step 2: Land the rustfmt baseline

Run `cd src-tauri && cargo fmt --all`. Review the diff is formatting-only (no token changes beyond whitespace/line breaks: `git diff --word-diff` should show no semantic edits). Commit as `chore: rustfmt baseline`.

**Verify**: `cd src-tauri && cargo fmt --all -- --check` → exit 0, AND `cargo test` → 81 tests pass.

### Step 3: Get clippy clean

Run `cd src-tauri && cargo clippy --all-targets -- -D warnings`. Fix style-level warnings; for any warning whose fix would change behavior or is disproportionate, add `#[allow(clippy::<lint>)]` at the smallest scope with a one-line reason comment. Commit as `chore: clippy clean pass`.

**Verify**: `cd src-tauri && cargo clippy --all-targets -- -D warnings` → exit 0, AND `cargo test` → 81 tests pass.

### Step 4: Add the CI workflow

Create `.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
  schedule:
    - cron: "17 6 * * 1" # weekly, so new RustSec advisories surface without a push
jobs:
  test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - name: Format
        run: cargo fmt --all -- --check
        working-directory: src-tauri
      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings
        working-directory: src-tauri
      - name: Tests
        run: cargo test
        working-directory: src-tauri
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
          working-directory: src-tauri
```

Note: if `rustsec/audit-check@v2` does not support `working-directory`, replace that job with a plain `cargo install cargo-audit --locked` + `cargo audit` run in `src-tauri/`. Check the action's README before choosing.

**Verify**: the three commands the workflow runs (`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, each in `src-tauri/`) all exit 0 locally. YAML sanity: `npx --yes yaml-lint .github/workflows/ci.yml` if available; otherwise careful visual inspection of indentation.

## Test plan

No new tests: this plan makes the existing 81 tests actually gate changes. The verification is that all three CI commands pass locally on the branch.

## Done criteria

- [ ] `grep -rn "cargo test --lib" CONTRIBUTING.md` → no matches
- [ ] `cd src-tauri && cargo fmt --all -- --check` exits 0
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` exits 0
- [ ] `cd src-tauri && cargo test` exits 0 with 81 passing tests
- [ ] `.github/workflows/ci.yml` exists and mirrors the local commands
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `cargo test` fails BEFORE any of your changes (pre-existing breakage — that is a finding, not something to fix here).
- The clippy pass produces more than ~30 warnings, or any warning whose fix requires changing behavior in `ops.rs`, `store.rs`, or `intel/` — report the list instead of mass-fixing.
- The rustfmt diff touches string literals or comments in a way that changes UI copy.

## Maintenance notes

- Every later plan in this set uses `cargo test` (unqualified) as its gate; this plan is why that gate means something.
- Dependabot PRs will now run this workflow; the `audit` job also runs weekly by schedule.
- Deliberately deferred: JS lint/typecheck for `frontend/index.html` (needs a maintainer decision on tooling for an inline script; see plans/README.md "considered" notes), and `cargo audit` locally as a pre-commit hook.
