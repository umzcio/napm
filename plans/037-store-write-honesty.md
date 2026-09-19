# Plan 037: Store writes report failure, create the temp file 0600, and cap history

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- src-tauri/src/store.rs src-tauri/src/lib.rs src-tauri/src/ops.rs frontend/index.html`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug / security
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

Three findings in `store.rs`, one plan because they share one function:

1. **Silent write failures.** `write_json` returns `()` and discards every
   error; on a full disk or a permissions problem a pin toggle, settings
   save, or history append is silently lost while the UI shows the new
   state. This is the one remaining silent-write path in a project whose
   stated rule is never to fake success.
2. **A world-readable window for the GitHub token.** The temp file is
   created with the umask default (typically 0644) and only then chmodded
   0600, so `settings.json.tmp` (which contains the GitHub token) is briefly
   readable by other local users, and a crash in the window leaves it that
   way. Plan 012 hardened the final file; this is the residual gap in the
   writer itself.
3. **Unbounded history.** `add_history` reads, appends, and rewrites the
   whole JSON array per operation with no cap; `get_history` ships the whole
   thing to the WebView on every call. It is the only unbounded user-data
   structure in the app.

## Current state

- `src-tauri/src/store.rs:121-133` - the writer, all errors discarded:

  ```rust
  fn write_json<T: Serialize>(path: &Path, value: &T) {
      if let Ok(s) = serde_json::to_string_pretty(value) {
          let tmp = path.with_extension("json.tmp");
          if std::fs::write(&tmp, s).is_ok() {
              #[cfg(unix)]
              {
                  use std::os::unix::fs::PermissionsExt;
                  let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
              }
              let _ = std::fs::rename(&tmp, path);
          }
      }
  }
  ```

- `src-tauri/src/store.rs:139-148` `set_pin`, `:157-162` `add_history`,
  `:172-175` `set_settings` - all return `()`, all hold `STORE_LOCK`.
- `src-tauri/src/lib.rs:31-34` (`set_pin`), `:124-127` (`set_settings`) -
  Tauri commands return `()`. `src-tauri/src/ops.rs:450-459` calls
  `store.add_history(...)` on op success with no way to know it failed.
- Frontend: `togglePin` (`frontend/index.html:~801`) already passes a revert
  callback as `call(...)`'s `onErr`, and `call` (`:1190-1198`) flashes
  `"<cmd> failed: ..."` in the status bar on invoke rejection - so once the
  backend command returns `Result`, the UI failure path already exists and
  needs no new design. Check the Preferences save path (around `:1589`) uses
  `call("set_settings", ...)`; if it uses a raw invoke, route it through
  `call`.
- `log = "0.4"` and `tauri-plugin-log` are already dependencies
  (`src-tauri/Cargo.toml`), so `log::warn!` is available.
- Existing tests: `store.rs` has ~17 tests (`#[test]` at lines 221-405+)
  including corruption/atomicity/concurrency coverage and a
  settings-mode test. Match their style: tempdir-based, no network.
- Convention note: error handling at the Tauri boundary is
  `Result<T, String>` (see `export_library`, `lib.rs:129-149`).

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Tests     | `cd src-tauri && cargo test`              | all pass            |
| Lint      | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | exit 0 |
| Format    | `cd src-tauri && cargo fmt --all -- --check` | exit 0 (run `cargo fmt` to apply) |
| App check | `npm run tauri dev`                       | pin/unpin and Preferences save still work |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/store.rs`
- `src-tauri/src/lib.rs` (the `set_pin` / `set_settings` command signatures only)
- `src-tauri/src/ops.rs` (the `add_history` call site only)
- `frontend/index.html` (only if the Preferences save path bypasses `call`)

**Out of scope** (do NOT touch, even though they look related):
- `src-tauri/src/intel/registry.rs`, `src-tauri/src/search/brew.rs`,
  `src-tauri/src/intel/wire.rs`, `src-tauri/src/intel/release.rs` - the other
  cache writers are plan 042's scope. Do not "reuse this opportunity" to
  touch them.
- Any change to the JSON file formats or locations.
- `scan/manual.rs`'s probe-cache writer (same write-then-chmod pattern, but
  it caches no secrets; recorded, deliberately not fixed here).

## Git workflow

- Branch: `advisor/037-store-write-honesty`
- Commit per step; style: `fix(store): ...` (see `git log`).

## Steps

### Step 1: `write_json` returns `io::Result<()>` and creates the temp file 0600

Rewrite `write_json` so the temp file is created with owner-only permissions
at creation time (no window), and every failure propagates:

```rust
fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("json.tmp");
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            std::io::Write::write_all(&mut f, s.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&tmp, &s)?;
        }
    }
    std::fs::rename(&tmp, path)
}
```

(Keep the existing doc comment about tmp+rename atomicity; extend it with
one line noting the temp file is created 0600 so a secret-bearing
`settings.json` is never world-readable, even mid-write or after a crash.
No em dashes in comments.)

**Verify**: `cd src-tauri && cargo build` → compiles (call sites will be
updated next; use `let _ = ...` temporarily nowhere, fix callers for real in
step 2).

### Step 2: Propagate through the mutators and commands

- `Store::set_pin`, `Store::set_settings`, `Store::add_history` return
  `std::io::Result<()>` (the `STORE_LOCK` guard stays).
- `add_history` also gains the cap: after `h.push(entry)`, sort newest-first
  and truncate:

  ```rust
  h.push(entry);
  h.sort_by_key(|e| std::cmp::Reverse(e.ts));
  h.truncate(500);
  Self::write_json(&self.history_path(), &h)
  ```

  Add a `const HISTORY_CAP: usize = 500;` with a one-line comment (bounds
  file size and the get_history payload; 500 is far beyond any real session
  count).
- `lib.rs`: `set_pin` and `set_settings` return `Result<(), String>`:

  ```rust
  #[tauri::command(async)]
  fn set_pin(app: tauri::AppHandle, pkg: String, pinned: bool) -> Result<(), String> {
      open_store(&app).set_pin(&pkg, pinned).map_err(|e| e.to_string())
  }
  ```

- `ops.rs` (`:450-459`): the op itself succeeded, so history failure must not
  flip the reported outcome, but it must not be silent either:

  ```rust
  if success {
      if let Err(e) = store.add_history(HistoryEntry { ts, pkg, eco, action, from, to }) {
          log::warn!("op succeeded but history write failed: {e}");
      }
  }
  ```

- Update all existing `store.rs` tests that call these methods (they now
  return `Result`; `.unwrap()` in tests is the house style if present).

**Verify**: `cd src-tauri && cargo test` → all existing tests pass after
mechanical updates.

### Step 3: New tests

In `store.rs`'s test module, following the existing tempdir pattern:

1. `history_is_capped_at_500_newest` - add 505 entries with increasing `ts`;
   assert `history().len() == 500` and the oldest 5 are gone.
2. `settings_tmp_file_is_owner_only_from_creation` (unix only) - save
   settings; assert the final file is 0600 (the existing mode test covers
   this; extend or add: after a write, no `settings.json.tmp` remains, and
   if one is staged manually mid-test its mode is 0600). At minimum assert
   the final artifact mode.
3. `write_failure_propagates` - create the store dir, chmod it 0555
   (read-only), attempt `set_pin`, assert `Err`. Restore 0755 in a guard or
   at test end so the tempdir cleans up. (macOS CI honors directory write
   permission for file creation.)
4. Frontend behavior has no test harness; verification is manual (step 5).

**Verify**: `cd src-tauri && cargo test store` → all store tests pass,
including the new ones.

### Step 4: Frontend error path check

Read the Preferences save handler (`frontend/index.html`, around `:1589`)
and `togglePin` (`:801`). Confirm both go through `call(...)` so a backend
`Err` flashes `"set_settings failed: ..."` / reverts the pin. If Preferences
uses a raw `invoke`, switch it to `call` (one-line change, matching every
other call site). Do not add new UI; the status-bar flash is the established
honest-error pattern from plan 007.

**Verify**: `grep -n 'set_settings' frontend/index.html` shows the save goes
through `call(`.

### Step 5: Manual smoke + full gate

Run `npm run tauri dev`: toggle a pin (survives restart), save Preferences
(survives restart), run any op and confirm it appears in History. Then:

**Verify**: `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check` → all exit 0.

## Test plan

Covered in step 3. Structural pattern: the existing `store.rs` tests
(tempdir + direct `Store` use). New tests: cap, mode-at-creation, error
propagation. `cargo test store` must show 3 new passing tests.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cd src-tauri && cargo test` exits 0, including the 3 new tests
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings` exits 0
- [ ] `cd src-tauri && cargo fmt --all -- --check` exits 0
- [ ] `grep -n "fn write_json" -A3 src-tauri/src/store.rs` shows an `io::Result` return
- [ ] `grep -n "truncate(500)\|HISTORY_CAP" src-tauri/src/store.rs` matches
- [ ] `grep -n "fn set_pin\|fn set_settings" src-tauri/src/lib.rs` shows `Result<(), String>` returns
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts above do not match the live code (drift since `3df72f5`).
- The read-only-dir test cannot fail the write on macOS (e.g. running as
  root or a filesystem that ignores modes); report instead of weakening the
  test.
- The frontend Preferences path turns out to need more than the one-line
  `call` switch (e.g. it depends on invoke never rejecting).
- Making the commands fallible breaks the importer (`src-tauri/src/importer.rs`)
  or another caller you did not expect; that means the call graph drifted.

## Maintenance notes

- The 500-entry cap means "what changed and when" is bounded; if a future
  feature needs deep history (e.g. per-tool timelines), page the file or
  archive instead of raising the cap silently.
- A reviewer should scrutinize: the unix/non-unix cfg split in `write_json`,
  the error-mapping at the command boundary, and that `ops.rs` still reports
  op success truthfully when only the history write failed.
- Follow-up explicitly deferred: the same 0600-at-creation fix for
  `scan/manual.rs`'s probe cache (no secrets in it, so cosmetic), and
  surfacing history-write failure in the UI (logged only, by design here).
