# Plan 009: Parallelize the installed scan and memoize repeated discovery subprocesses

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/scan/ src-tauri/src/ops.rs`
> Plans 002/003 legitimately touch these files; reconcile against their diffs.
> Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (concurrency in the app's primary code path; source scanners are independent, which bounds it)
- **Depends on**: none (cleaner after 002/003 land)
- **Category**: perf
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

`scan_all` is the app's primary action: it runs at every launch (under the dial-up splash) and after every install. It scans npm → brew → pip → npx → manual strictly sequentially, and three of those sources block on the network inside a subprocess (`npm outdated -g --json`, `brew outdated --json=v2`, `pip list --outdated`), so their latencies add instead of overlapping. The codebase already knows the right pattern — `search::search_all` fans its three sources out with `std::thread::scope` so total time is the slowest source, not the sum. On top of that, the same discovery subprocesses are spawned repeatedly per scan: `brew --prefix` runs in both `scan/brew.rs` and `scan/manual.rs`, `npm root -g` in both `scan/npm.rs` and `scan/manual.rs`, the python site-dir probe in both `scan/pip.rs` and `scan/manual.rs`, and `pip_bin()` re-probes `pip3 --version` on every scan and again on every pip operation. Package-manager cold starts are hundreds of milliseconds each; this is seconds of launch latency for zero benefit.

## Current state

- `src-tauri/src/scan/mod.rs:62-77` — the sequential aggregation:
  ```rust
  pub fn scan_all(pins: &std::collections::BTreeSet<String>, sources: Sources) -> Vec<InstalledTool> {
      let mut all = Vec::new();
      if sources.npm { all.extend(npm::scan_npm()); }
      if sources.brew { all.extend(brew::scan_brew()); }
      if sources.pip { all.extend(pip::scan_pip()); }
      if sources.npx { all.extend(npx::scan_npx()); }
      if sources.manual {
          let other_names: std::collections::BTreeSet<String> =
              all.iter().map(|t| t.name.clone()).collect();
          all.extend(manual::scan_manual(&other_names));
      }
      for row in all.iter_mut() { row.pinned = pins.contains(&row.pkg); }
      all
  }
  ```
  Note the ordering constraint: `manual` consumes `other_names` built from the other four sources' results, so manual must run AFTER they complete. (The roadmap records this exclusion ordering shipped broken twice in M9 — respect it.)
- The exemplar parallel pattern: `src-tauri/src/search/mod.rs:50-59` (`std::thread::scope` with one spawn per source, results collected after the scope).
- Duplicate discovery spawns (verify each with grep before touching):
  - `brew --prefix`: `scan/brew.rs:108` and `scan/manual.rs:55`
  - `npm root -g`: `scan/npm.rs:78` and `scan/manual.rs:71`
  - `python3 -c "import site..."`: `scan/pip.rs:103` and `scan/manual.rs:82`
  - `pip_bin()` (`scan/pip.rs:76-88`, probes `pip3 --version` then `pip --version`): called per scan and per op (`ops.rs:69`)
- `pip_bin` already returns `Option<&'static str>` — memoizable as-is.
- All scanners are free functions with no shared mutable state; each already degrades to an empty vec on failure (`scan/mod.rs:41-47` `run()` returns `""` on spawn failure).
- Convention: tests in `#[cfg(test)] mod tests` per file.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app (timing) | `npm run tauri dev` | library populates noticeably faster on relaunch |

## Scope

**In scope**:
- `src-tauri/src/scan/mod.rs` (parallel `scan_all`)
- `src-tauri/src/scan/brew.rs`, `scan/npm.rs`, `scan/pip.rs`, `scan/manual.rs` (memoized discovery lookups only)

**Out of scope**:
- `search/` — already parallel.
- The manual scanner's per-binary version probing (plan 014 handles its caching/parallelism and the consent question).
- Registry-document caching and `whats_new` fan-out (plan 010).
- Any change to scan OUTPUT (row contents must be identical).

## Git workflow

- Branch: `advisor/009-parallel-scan`
- Commits: `perf(scan): run source scans concurrently` and `perf(scan): memoize discovery subprocess results`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Memoize the discovery lookups

Create small `OnceLock`-backed accessors, each in the file that owns the underlying probe, and route both call sites through them:

```rust
// scan/brew.rs
pub(crate) fn brew_prefix() -> &'static Option<PathBuf> {
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(|| { /* existing `brew --prefix` invocation */ })
}
```

Same shape for `npm_root()` in `scan/npm.rs`, the python site dirs in `scan/pip.rs`, and wrap `pip_bin()`'s body in a `OnceLock<Option<&'static str>>`. Update `scan/manual.rs` (and `ops.rs:69` for `pip_bin`) to call the shared accessors. Semantics note to put in each doc comment: the value is now per-process; a user who installs Homebrew while napm is running sees it after an app restart. That trade is acceptable (PATH itself is captured once at startup already — `pathenv.rs`).

**Verify**: `cd src-tauri && cargo test` → exit 0; `grep -rn "brew --prefix\|npm root -g" src-tauri/src/scan/` → each probe string appears in exactly one function.

### Step 2: Parallelize `scan_all`

Mirror `search::search_all`'s `thread::scope` shape: spawn the four independent scanners (each behind its `sources` flag; a disabled source contributes an empty vec without spawning), join, build `other_names`, then run `manual` (still on the scoped threads' parent — manual stays sequential after the join). Keep the pins stamping loop unchanged.

```rust
let (npm_rows, brew_rows, pip_rows, npx_rows) = std::thread::scope(|s| {
    let n = s.spawn(|| if sources.npm { npm::scan_npm() } else { Vec::new() });
    let b = s.spawn(|| if sources.brew { brew::scan_brew() } else { Vec::new() });
    let p = s.spawn(|| if sources.pip { pip::scan_pip() } else { Vec::new() });
    let x = s.spawn(|| if sources.npx { npx::scan_npx() } else { Vec::new() });
    (n.join().unwrap_or_default(), b.join().unwrap_or_default(),
     p.join().unwrap_or_default(), x.join().unwrap_or_default())
});
```

Preserve the output ORDER of the final vec (npm, brew, pip, npx, manual) so downstream display sorting sees identical input.

**Verify**: `cd src-tauri && cargo test` → exit 0 (including the `scan_all` gating test if plan 006 added it; add it here if not — all-sources-off returns empty).

### Step 3: Timing sanity

Run `npm run tauri dev`, let the first scan finish, then File → Rescan now and observe wall time (log lines appear in the dev console; or just observe the splash/refresh). Expected: rescan completes in roughly the time of the slowest source rather than the sum. No formal benchmark required; note observed before/after in your report if you measured.

## Test plan

- `scan_all` with all sources disabled → empty (pure, no subprocesses spawned).
- Memoized accessors: call twice, same value (trivially true; the test's value is compile-time wiring, keep it minimal or skip where it would need process spawning in tests).
- The real coverage is the existing per-source parser tests, which must remain green: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0
- [ ] `scan_all` uses `std::thread::scope`; manual still runs after the join with `other_names` from all four sources
- [ ] Each discovery probe (`brew --prefix`, `npm root -g`, python site dirs, pip probe) exists in exactly one function, `OnceLock`-cached
- [ ] App-run: library contents identical to before (spot-check row count and a few rows)
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live code beyond plans 002/003's expected diffs.
- Any scanner turns out to share mutable state with another (none should; if the compiler fights the scope closure borrows over something shared, report rather than wrapping in locks).
- Manual rows change (appear/disappear) versus the sequential version — that means `other_names` ordering broke; this is the twice-shipped M9 regression, treat as STOP.

## Maintenance notes

- Plan 022 (cargo ecosystem) adds a sixth source: it joins the parallel scope and contributes to `other_names`.
- The per-process memoization interacts with plan 014's probe cache; they are complementary (different data).
- Reviewer: confirm no spawned closure captures `sources` by reference in a way that outlives the scope (it is `Copy`; capture by value).
