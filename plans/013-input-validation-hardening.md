# Plan 013: Validate package names and versions before argv, and parse GitHub repo URLs strictly

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/ops.rs src-tauri/src/intel/release.rs`
> Plan 003 legitimately touches ops.rs; reconcile against its diff. Any other
> mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW (validation can only reject; the risk is rejecting legitimate names — bounded by tests)
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Two hardening gaps, both bounded but both one refactor away from worse:

1. **Argument injection into package managers.** `build_command` interpolates the caller-supplied `pkg` and `version` into argv tokens with no validation. There is no shell anywhere (`Command::new(...).args(...)` throughout — good), but package names arrive from registry search results, i.e. from whoever published the package. A name starting with `-` lands in argv where npm parses flags. Because name and version are fused into one token (`pkg@version`), this is bounded to option-confusion rather than arbitrary flag injection — but nothing enforces even that boundary, and a future code path that passes `pkg` alone (the brew arm already does: `pkg.to_string()` as its own token) inherits the exposure.
2. **Registry-controlled path segments in authenticated GitHub requests.** `github_repo_from_url` finds the substring `github.com/` ANYWHERE in a registry-supplied URL and takes the next two segments. The `repo` segment is stripped of `#`/`?`; the `owner` segment is NOT. Both are then interpolated unencoded into `https://api.github.com/...` URLs that carry the user's PAT in an Authorization header. The host is hardcoded so the token cannot leave api.github.com, and ureq 2.x drops auth headers on cross-host redirects — but a package author still controls the path/query shape of an authenticated request, and the loose `contains("github.com")` matching means `https://evil.example/github.com/x/y` parses as a repo.

## Current state

- `src-tauri/src/ops.rs:4-31` — `build_command` (no validation):
  ```rust
  pub fn build_command(eco: &str, pkg: &str, version: &str, action: &str, pip_bin: &str) -> Option<(String, Vec<String>)> {
      match (eco, action) {
          ("npm", _) => Some(("npm".to_string(),
              vec!["i".to_string(), "-g".to_string(), format!("{}@{}", pkg, version)])),
          ("pip", _) => Some((pip_bin.to_string(),
              vec!["install".to_string(), format!("{}=={}", pkg, version)])),
          ("brew", "install") | ("brew", "update") =>
              Some(("brew".to_string(), vec!["install".to_string(), pkg.to_string()])),
          ("npx", "promote") => Some(("npm".to_string(),
              vec!["i".to_string(), "-g".to_string(), pkg.to_string()])),
          _ => None,
      }
  }
  ```
  Existing tests at `ops.rs:136-172` pin the happy-path shapes (including scoped npm names implicitly via `typescript`; add a scoped case).
- `src-tauri/src/intel/release.rs:106-118` — the loose URL parse:
  ```rust
  pub fn github_repo_from_url(url: &str) -> Option<(String, String)> {
      let i = url.find("github.com/")? + "github.com/".len();
      let rest = &url[i..];
      let mut parts = rest.split('/');
      let owner = parts.next()?.trim();
      let repo = parts.next()?.trim().trim_end_matches(".git");
      let repo = repo.split(['#', '?']).next().unwrap_or(repo);
      if owner.is_empty() || repo.is_empty() { return None; }
      Some((owner.to_string(), repo.to_string()))
  }
  ```
  Consumers that interpolate the results into authenticated URLs: `release.rs:242` (`api.github.com/search/issues?q=repo:{owner}/{repo} ...` via the `count` closure) and the releases fetch around `release.rs:326-341` (changelog path). Feeding URLs come from npm `repository.url`, PyPI `project_urls`/`home_page`, brew `homepage`, matched with `contains("github.com")` (`release.rs:296-317`).
  Existing tests for this function are in `release.rs`'s test module (verify with `grep -n "github_repo_from_url" src-tauri/src/intel/release.rs`) — extend, do not replace.
- Percent-encoding helper available: `crate::http::encode` (`src-tauri/src/http.rs:51-62`).
- Real-world name grammars to respect (do not over-tighten):
  - npm: lowercase URL-safe names, may be scoped `@scope/name`; must not begin with `.` or `_`; length ≤ 214. Legacy packages may contain uppercase.
  - pip (PEP 503): letters, digits, `-`, `_`, `.`; must start and end alphanumeric.
  - brew: formula names like `ripgrep`, `gcc@13`, `python@3.12` (note `@` is legal in brew names).
  - GitHub owner: alphanumerics and `-` (plus legacy `_`); repo: alphanumerics, `-`, `_`, `.`.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test ops:: && cd - && cd src-tauri && cargo test intel::release` | pass |
| All tests | `cd src-tauri && cargo test` | exit 0 |

## Scope

**In scope**:
- `src-tauri/src/ops.rs` (validation gate + tests)
- `src-tauri/src/intel/release.rs` (`github_repo_from_url` + URL construction sites + tests)

**Out of scope**:
- `lib.rs` command signatures.
- The frontend (it may still SEND bad names; the backend gate is the enforcement point).
- `search/` fetch paths (they already `http::encode` query values; verify, do not modify).

## Git workflow

- Branch: `advisor/013-input-validation`
- Commits: `fix(ops): validate package names and versions before building argv` and `fix(intel): strict GitHub repo URL parsing and encoded API paths`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Validation gate in `build_command`

Add two functions with doc comments in `ops.rs`, called at the top of `build_command` (return `None` on failure — `run_op` already emits a failed `transfer-done` for `None`; if plan 016 landed, it also emits an explanatory line):

```rust
/// Package-name gate before argv. Rejects: empty, leading '-', any whitespace
/// or control char, and shapes outside the ecosystem's grammar (npm allows one
/// leading @scope/ segment; brew allows '@' for versioned formulae). Length <= 214.
fn valid_pkg(eco: &str, pkg: &str) -> bool { ... }

/// Version gate: non-empty, no leading '-', chars limited to [A-Za-z0-9._+-].
fn valid_version(v: &str) -> bool { ... }
```

Implementation guidance: reject-list first (leading `-`, whitespace/control anywhere, `..`), then a light per-eco allow-shape: npm `@scope/name` = at most one `/`, and only when starting with `@`; pip/brew = no `/` at all. Do NOT attempt full grammar validation — the goal is excluding argv-dangerous shapes, not registry-perfect validation.

Where the tool supports it, add the end-of-options marker so even a pathological name cannot be read as a flag: npm arms become `["i", "-g", "--", spec]`, pip becomes `["install", "--", spec]`. brew does not reliably support `--` before formula names — rely on the gate there (note this in a comment).

**Verify**: `cd src-tauri && cargo test ops::` → existing 5 tests updated for the `--` insertion + new validation tests pass.

### Step 2: Strict GitHub URL parsing

Rewrite `github_repo_from_url` to parse rather than substring-match: strip a scheme prefix (`https://`, `http://`, `git+https://`, `git://`, `ssh://git@`), then require the authority to EQUAL `github.com` or `www.github.com` (case-insensitive), then split the path. Sanitize BOTH segments: strip `#?` fragments from owner as well, then validate `owner` against `[A-Za-z0-9-_]+` and `repo` against `[A-Za-z0-9._-]+` (after `.git` trim); return None otherwise. Keep the function signature.

At the URL construction sites (`release.rs:242` count closure, and the releases URL near `:326-341`), run `owner`/`repo` through `crate::http::encode` when formatting (with the charset gate this is belt-and-braces, which is the point).

**Verify**: `cd src-tauri && cargo test intel::release` → existing URL tests (the `release.rs:420-427` area cases: `git+https://github.com/owner/repo.git` etc.) still pass, plus the new rejections.

## Test plan

- `ops.rs`:
  - existing shapes still build (npm plain + `@scope/name`, pip, brew `gcc@13`, npx promote) — with `--` present for npm/pip
  - rejected: leading `-` name, name with a space, name with a newline, empty version, version with a space, `..` in name
- `release.rs` `github_repo_from_url`:
  - accepted (existing): `https://github.com/owner/repo`, `git+https://github.com/owner/repo.git`
  - accepted new: `ssh://git@github.com/owner/repo`, `https://www.github.com/owner/repo`
  - rejected: `https://evil.example/github.com/owner/repo`, `https://github.com.evil.example/owner/repo`, owner containing `#` or `?` or `%`, owner `..`, empty segments

**Verification**: `cd src-tauri && cargo test` → exit 0, all pass.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; the rejection tests above exist and pass
- [ ] `grep -n "fn valid_pkg" src-tauri/src/ops.rs` → present, called in `build_command`
- [ ] npm and pip argv include `--` before the package spec
- [ ] `github_repo_from_url` requires an exact github.com host and validates both segments; `grep -n "find(\"github.com/\")" src-tauri/src/intel/release.rs` → no matches
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond plan 003's expected ops.rs diff.
- `--` breaks a real install in manual testing (npm and pip both support it; if a version in the wild fails, drop the marker for that tool and keep the gate, reporting which).
- Tightening the URL parser drops changelogs for a REAL package in your test run (check one known package per ecosystem: e.g. npm `typescript`, pip `httpie`) — loosen only with a named test case.

## Maintenance notes

- Plan 022 (cargo ecosystem) must route its `cargo install` arm through `valid_pkg`/`valid_version` and add its name grammar.
- The `contains("github.com")` matchers that FEED this function (`release.rs:296-317`) still select candidate URLs loosely; that is fine — the strict parse now decides. Do not "fix" the matchers separately.
- Reviewer: the npm legacy-uppercase allowance and brew `@` are the two spots over-tightening would bite users.
