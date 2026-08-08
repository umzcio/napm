# Plan 003: Make store writes atomic and prevent duplicate concurrent package operations (backend)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/store.rs src-tauri/src/ops.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED (changes persistence and op-spawn behavior; mitigated by keeping shapes identical)
- **Depends on**: none (pairs with plan 004, the frontend half)
- **Category**: bug
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Every `run_op` spawns a detached thread with no registry of in-flight work, and the frontend's Update All fires one `run_op` per outdated tool in a loop. So a single Update All with 8 outdated tools runs 8 package-manager processes concurrently (Homebrew fails on its own lock; concurrent `npm i -g` contend on the same global prefix), and nothing stops a double-click from running the same install twice at once. Meanwhile the store's `add_history` and `set_pin` are read-whole-file → modify → write-whole-file with no lock, so concurrent op completions lose history entries; and `fs::write` truncates in place, so the app dying mid-write leaves a corrupt file that `read_json` silently reads back as *empty* — the user's entire history or pin set vanishes with no error. History is the feature that answers "what changed and when"; losing entries silently defeats it.

## Current state

- `src-tauri/src/store.rs:53-64` — non-atomic write, silent-empty read:
  ```rust
  fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
      std::fs::read_to_string(path)
          .ok()
          .and_then(|s| serde_json::from_str(&s).ok())
          .unwrap_or_default()
  }

  fn write_json<T: Serialize>(path: &Path, value: &T) {
      if let Ok(s) = serde_json::to_string_pretty(value) {
          let _ = std::fs::write(path, s);
      }
  }
  ```
- `src-tauri/src/store.rs:70-91` — the unlocked read-modify-write pairs (`set_pin` on `pins.json`, `add_history` on `history.json`). `set_settings` at `store.rs:99+` has the same shape on `settings.json`.
- `src-tauri/src/store.rs:40-48` — `Store` is just `{ dir: PathBuf }` and is constructed fresh per command call (`lib.rs:14-20` `open_store`), so any lock must be process-global, not per-instance.
- `src-tauri/src/ops.rs:57-72` — `run_op` builds the command then `std::thread::spawn`s with no in-flight tracking:
  ```rust
  pub fn run_op(app: AppHandle, store: Store, op_id: String, eco: String, pkg: String, ...) {
      let pip = crate::scan::pip::pip_bin().unwrap_or("pip3");
      let built = build_command(&eco, &pkg, &to, &action, pip);
      std::thread::spawn(move || { ... });
  }
  ```
- `src-tauri/src/ops.rs:129-131` — each op thread calls `store.add_history(...)` on success.
- The events the frontend listens for (`frontend/index.html:903-915`): `transfer-line` `{op_id, stream, line}` and `transfer-done` `{op_id, success, code}`. Do not change these payload shapes; plan 004 relies on them as-is.
- Convention: tests in `#[cfg(test)] mod tests` in the same file (see `ops.rs:136-172`, `store.rs` bottom).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0, all pass |
| Targeted | `cd src-tauri && cargo test store::` / `cargo test ops::` | pass |
| Manual run | `npm run tauri dev` (repo root) | app launches; installs stream output |

## Scope

**In scope**:
- `src-tauri/src/store.rs`
- `src-tauri/src/ops.rs`

**Out of scope**:
- `frontend/index.html` — the frontend in-flight guard and identity keying is plan 004. This plan's backend rejection is the safety net under it.
- `src-tauri/src/lib.rs` — command signatures must not change.
- File permissions (0600 on settings.json) — that is plan 012; keep the diffs separable.
- Serializing ops *globally* (a queue that runs one op at a time). Deliberately NOT in scope: different packages may still run concurrently; only same-package duplicates are rejected. A global queue is a UX/product change the maintainer has not asked for.

## Git workflow

- Branch: `advisor/003-store-atomicity`
- Commits: `fix(store): atomic writes and a process-wide store lock` and `fix(ops): reject duplicate in-flight operations per package`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Atomic `write_json`

In `store.rs`, change `write_json` to write to a sibling temp file then rename (rename is atomic on the same filesystem):

```rust
fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Ok(s) = serde_json::to_string_pretty(value) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
```

**Verify**: `cd src-tauri && cargo test store::` → existing store tests pass; new roundtrip test (Test plan) passes; `ls` of the test temp dir shows no `.json.tmp` leftovers after a write.

### Step 2: Process-wide lock around read-modify-write

Add at module scope in `store.rs`:

```rust
use std::sync::Mutex;
static STORE_LOCK: Mutex<()> = Mutex::new(());
```

Take `let _g = STORE_LOCK.lock().unwrap();` as the first line of `set_pin`, `add_history`, and `set_settings` (the three read-modify-write methods). Reads (`pins`, `history`, `settings`) stay lock-free — rename-based writes mean a reader always sees a complete old or new file.

**Verify**: `cd src-tauri && cargo test store::` → pass, including the new concurrent-append test (Test plan).

### Step 3: In-flight op registry in `ops.rs`

Add to `ops.rs`:

```rust
use std::collections::HashSet;
use std::sync::Mutex;

static IN_FLIGHT: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

/// Try to claim (eco, pkg) as in flight. False when an op for it is already running.
fn try_begin(eco: &str, pkg: &str) -> bool { ... insert into the set, false if present ... }
fn finish(eco: &str, pkg: &str) { ... remove from the set ... }
```

In `run_op`, before spawning: if `!try_begin(&eco, &pkg)`, emit a `transfer-line` (`stream: "stderr"`, line: `another operation for <pkg> is already running`) followed by `transfer-done { success: false, code: -1 }` for this `op_id`, and return without spawning. Inside the spawned thread, ensure `finish` runs on every exit path — use a small guard struct whose `Drop` calls `finish`, declared right at the top of the closure, so a panic in the streaming code cannot leak the claim.

Wording note: this line is user-visible UI copy; no em dashes (repo rule).

**Verify**: `cd src-tauri && cargo test ops::` → existing 5 tests plus new `try_begin`/`finish` tests pass.

### Step 4: Manual smoke test

Run `npm run tauri dev`. In the app: trigger an update from the library, and while it streams, click Get on the same row again (the frontend guard from plan 004 may not exist yet — that is the point). Expected: the second transfer row appears, immediately shows the "another operation ... is already running" line, and fails honestly with no second package-manager process. Then confirm a normal install still succeeds and lands one history entry.

**Verify**: behavior as described; `history.json` in the app-data dir (File menu → Open data folder) contains exactly one new entry.

## Test plan

- `store.rs` tests (model after existing ones in the file, using a temp dir):
  - Roundtrip: `add_history` then `history()` returns the entry; no `.tmp` file remains.
  - Concurrent appends: spawn 8 threads each calling `add_history` once on the same `Store` dir; after joining, `history()` has exactly 8 entries. (This fails on the old code, passes with the lock.)
  - Corrupt-file behavior unchanged: a file containing `not json` still reads as default (document with a comment that plan 007/008 may later surface this instead of hiding it).
- `ops.rs` tests:
  - `try_begin("npm","x")` → true; second `try_begin("npm","x")` → false; after `finish("npm","x")`, true again.
  - `try_begin("npm","x")` and `try_begin("pip","x")` are independent (both true).

**Verification**: `cd src-tauri && cargo test` → exit 0, all pass.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; the concurrent-append and in-flight tests exist and pass
- [ ] `grep -n "fs::write(path" src-tauri/src/store.rs` → no direct truncate-write of the final path remains in `write_json`
- [ ] `grep -n "try_begin" src-tauri/src/ops.rs` → present and called in `run_op`
- [ ] `transfer-line`/`transfer-done` payload field names unchanged (`grep -n "op_id" src-tauri/src/ops.rs`)
- [ ] Manual smoke test in Step 4 done and behaves as described
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live code.
- You find any other writer of `pins.json`/`history.json`/`settings.json` outside `store.rs` (there should be none; `intel/mod.rs:81-87` only READS settings.json — reading is fine).
- The duplicate-rejection breaks Update All for DIFFERENT packages (it must not — the key is `(eco, pkg)`).
- Changing `run_op` requires touching its signature in `lib.rs`.

## Maintenance notes

- Plan 004 adds the frontend guard so users normally never see the backend rejection; keep the backend line anyway (defense in depth, and other invoke paths exist).
- Plan 012 will add permission bits to these writes; it builds on this `write_json`.
- If a global op queue (strictly sequential transfers) is ever wanted, `IN_FLIGHT` is the place it grows from.
- Reviewer: scrutinize the Drop-guard in Step 3 (claims must never leak) and the temp-file extension choice (must not collide with real store files).
