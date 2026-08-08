# Plan 014: Cache, parallelize, and disclose the manual scanner's version probing

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/scan/manual.rs src-tauri/src/store.rs frontend/index.html`
> Plans 002/003/009/012 legitimately touch these; reconcile against their
> diffs. Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: MED (changes a visible behavior of the manual source; the consent model is a product decision this plan makes explicit)
- **Depends on**: none (composes with 009)
- **Category**: security / perf
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

To resolve a version for a `$PATH` binary whose filename carries none, the manual scanner EXECUTES the binary: `<tool> --version`, then `<tool> version`, each with a 2-second kill timer — on every single library scan, at every launch and after every install. Two costs:

- **Security**: home-directory `$PATH` entries (`~/.local/bin`, `~/bin`) are exactly where a malicious postinstall or a `curl | sh` script drops payloads. A dormant binary that would only ever run on explicit invocation gets executed automatically by napm. The scanner's own comments show this was thought about (home-only gate, no `-v` probing) — but auto-execution of an open-ended binary set is a different decision than "napm runs npm/brew/pip", and it is currently made silently on the user's behalf.
- **Performance**: probes are serial inside the `$PATH` walk; a binary that answers neither flag costs up to 4 seconds. A dozen such tools is a near-minute scan during which the library is empty.

This plan caches probe results by (path, mtime, size) so each binary is executed at most once per version, bounds the probing with a small worker pool, and surfaces the behavior as a labeled preference so the user can turn it off. The stricter model (default-off, per-binary consent) is documented for the maintainer as an explicit alternative; this plan does not silently change the product's default.

## Current state

- `src-tauri/src/scan/manual.rs:90-115` — `resolve_version`: filename token first (free), else, only for binaries resolving under `$HOME`, run `--version` then `version` via `run_with_timeout` (`:119-141`, spawn + 50ms `try_wait` polling + kill at 2s). The doc comment records the deliberate guards.
- `src-tauri/src/scan/manual.rs:160-209` — the sequential `$PATH` walk calling `resolve_version` (`:192`) per unmanaged binary.
- `src-tauri/src/store.rs:32-37` — `Settings { github_token, sources }`, serde `camelCase` + `default`, so adding a field with a `default = "default_true"` keeps old settings files parsing.
- Preferences UI: `frontend/index.html:1045-1056` (`renderPrefs` builds the checkbox list; the save handler around `:1085-1093` reads the checkboxes into the settings object and invokes `set_settings`).
- `scan_manual` signature: `pub fn scan_manual(&other_names)` called from `scan/mod.rs:68-72`; the scanners have no store access today — `scan_all` receives `Sources` from `lib.rs:26` (`scan::scan_all(&pins, store.settings().sources)`), so a new setting travels the same way (extend `scan_all`'s parameters or pass the whole `Settings`).
- App-data cache file conventions: JSON files in the app-data dir; `clear_caches` (`src-tauri/src/lib.rs:162-187`) removes named files and prefixed families.
- UI copy rule: no em dashes; limits are surfaced honestly (the M9 pattern: "unmanaged" labels rather than fake data).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test scan::manual` | pass |
| All tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | manual rows behave per checklist |

## Scope

**In scope**:
- `src-tauri/src/scan/manual.rs` (probe cache + bounded parallel probing + the setting gate)
- `src-tauri/src/store.rs` (one new `Settings` field)
- `src-tauri/src/scan/mod.rs` + `src-tauri/src/lib.rs` (plumb the setting + cache dir into `scan_manual`)
- `frontend/index.html` (one labeled checkbox in Preferences)

**Out of scope**:
- Changing the DEFAULT (probing stays on by default in this plan; flipping the default or per-binary consent is the documented alternative for the maintainer).
- The managed-roots exclusion list.
- `run_with_timeout` internals.

## Git workflow

- Branch: `advisor/014-manual-probe`
- Commits: `perf(scan): cache and parallelize manual version probes`, `feat(prefs): toggle for executing manual tools to read versions`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Probe result cache

In `manual.rs`, add a JSON cache `manual_probe.json` in the app-data dir: a map `resolved_path -> { mtime: i64, size: u64, version: String }`. `resolve_version` consults it first (hit requires mtime AND size match); on a completed probe (including a failed one — cache `""` so a version-less binary is not re-executed every scan), record the entry. Load once per scan, save once per scan (atomic write; reuse plan 003's temp+rename pattern). `scan_manual` gains a `cache_dir: &Path` parameter plumbed from `lib.rs` through `scan_all`.

Add `manual_probe.json` to `clear_caches`' named-file list in `lib.rs` so Refresh registry caches also re-probes.

**Verify**: `cd src-tauri && cargo test scan::manual` → new cache tests pass (Test plan). App run: second Rescan now is visibly faster when version-less binaries exist on `$PATH`.

### Step 2: Bounded parallel probing

Restructure the walk: collect all unmanaged candidates first (cheap, filesystem-only), then resolve versions for the cache-miss subset on a `thread::scope` pool of at most 4 workers, then assemble rows in the original order. The probe count is usually small after Step 1, so 4 is plenty; the cap exists so a cold cache never executes dozens of unknown binaries simultaneously.

**Verify**: `cd src-tauri && cargo test` → exit 0; row output identical to the sequential version for the same inputs (assert in a test on the pure assembly path if separable, else via the app run).

### Step 3: The preference

- `store.rs`: add `pub probe_manual: bool` to `Settings` with a serde default of `true` (`#[serde(default = "default_probe_manual")]` + `fn default_probe_manual() -> bool { true }` — the struct-level `default` derives from `Default`; implement `Default` accordingly).
- Plumb into `scan_manual`; when false, `resolve_version` stops after the filename check (rows show an empty version — which the UI already renders as an em-dash-free placeholder for manual rows; confirm rendering and leave it).
- `renderPrefs`: under the Sources block add one checkbox, checked from `s.probeManual!==false`, labeled: "Run manual tools with --version to read their version (executes binaries found on your PATH)". Save path writes `probeManual` into the settings object.

**Verify**: app run — untick the box, save (a rescan fires per the existing save path), manual rows without filename versions show no version; re-tick, save, versions return (from cache or probe).

## Test plan

- `manual.rs` tests (temp dir; model after existing tests in the file):
  - cache roundtrip: record → load → hit on matching mtime+size; miss on changed mtime
  - failed-probe caching: an entry with `version: ""` is a hit (no re-probe decision)
  - a corrupt `manual_probe.json` degrades to an empty cache (no panic)
  - probing disabled: `resolve_version`-equivalent path returns filename-derived or empty without spawning (structure the gate so this is testable without executing anything — e.g. pass an enum `Probe::Allowed(cache)/Off`)
- Settings: deserialize an OLD settings.json (no `probeManual` key) → `probe_manual == true` (add to `store.rs` tests).

**Verification**: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; cache + settings-default tests exist and pass
- [ ] `manual_probe.json` written to app-data and listed in `clear_caches`
- [ ] Probe fan-out bounded at 4; candidates collected before probing
- [ ] Preferences shows the labeled toggle; off = no binary execution beyond filename parsing
- [ ] Old settings files still parse with probing defaulted on
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond the named plans' expected diffs.
- Plumbing `cache_dir`/settings into `scan_manual` forces a change to the `scan_installed` command signature visible to the frontend (it must not — the frontend passes nothing).
- The maintainer has meanwhile decided on the stricter consent model (check `plans/README.md` notes) — execute that decision instead of this default.

## Maintenance notes

- **Documented alternative for the maintainer**: default `probe_manual` to OFF and add a per-binary "resolve version" action (row context menu) persisting consent per path+mtime. Stronger security posture, visible product regression for existing users. This plan's toggle is the infrastructure either way; flipping the default later is a one-line change plus release notes.
- The (path, mtime, size) key means a replaced binary at the same path with identical mtime+size serves a stale version — accepted; hash-keying would cost a full file read per scan.
- Reviewer: confirm the probe pool never executes a binary NOT under `$HOME` (the existing `under_home` gate must sit before the pool dispatch).
