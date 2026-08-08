# Plan 012: Owner-only permissions on the settings file and a password-style token field

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/store.rs frontend/index.html`
> Plan 003 legitimately rewrites `write_json`; reconcile against its diff.
> Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/003-store-atomicity-and-op-serialization.md (soft: both edit `write_json`; land 003 first)
- **Category**: security
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Storing the GitHub personal access token in `settings.json` is a recorded product decision for this local desktop app, and the audit confirmed the token never leaks into logs, caches, or exports. Two implementation details widen that decision beyond what was decided, and this plan closes exactly those: (1) the file is written with `std::fs::write`, which creates mode 0644 under the default umask — world-readable, so any other local account or unsandboxed process can read the credential; (2) the Preferences dialog renders the token into a plain text input, visible on screen, in screenshots, and on screen shares.

## Current state

- `src-tauri/src/store.rs:60-64` — the write path (no `set_permissions` call exists anywhere in the crate; verify with `grep -rn "set_permissions" src-tauri/src` → no matches):
  ```rust
  fn write_json<T: Serialize>(path: &Path, value: &T) {
      if let Ok(s) = serde_json::to_string_pretty(value) {
          let _ = std::fs::write(path, s);
      }
  }
  ```
  (If plan 003 landed, this is now a temp-write + rename — the permission fix applies to the temp file before rename, or to the final path after; either way the effective mode must end 0600.)
- `src-tauri/src/store.rs:93-101` — `settings.json` path and `set_settings` route through `write_json`. The token field is `Settings.github_token` (`store.rs:32-37`).
- `frontend/index.html:1054` — the token input in `renderPrefs`:
  ```js
  '<input id="prefToken" class="search" style="width:100%;margin-top:4px" value="'+esc(s.githubToken||"")+'"></div>'
  ```
- The token is also read directly from `settings.json` by `intel::github_token` (`src-tauri/src/intel/mod.rs:81-87`) — read-only, unaffected.
- Platform: macOS only; `#[cfg(unix)]` is always true in practice, but keep the cfg so a future non-unix target compiles.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test store::` | pass |
| All tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | Preferences behaves as described |
| Inspect mode | `ls -l "$HOME/Library/Application Support/com.napm.app/settings.json"` | `-rw-------` after saving prefs |

## Scope

**In scope**:
- `src-tauri/src/store.rs`
- `frontend/index.html` (the `renderPrefs` token input and its save-path read only)

**Out of scope**:
- Moving the token to the macOS Keychain — a bigger decision recorded as follow-up, not this plan.
- `pins.json` / `history.json` modes (not sensitive; but the simple implementation below tightens them too, which is harmless — note it in the commit message).
- Token validation or scopes UI.

## Git workflow

- Branch: `advisor/012-settings-hardening`
- Commits: `fix(store): owner-only mode on store files` and `fix(ui): token field is a password input with a reveal toggle`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Owner-only file mode

In `store.rs` `write_json`, after the successful write (or on the temp file before the rename, if plan 003's shape is in place), set the mode:

```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(<final-or-temp path>, std::fs::Permissions::from_mode(0o600));
}
```

Also handle the pre-existing file: in `Store::new` (`store.rs:44-48`), best-effort re-mode `settings.json` if it exists, so users upgrading get the fix without re-saving preferences.

**Verify**: `cd src-tauri && cargo test store::` → pass (add the mode assertion test from the Test plan); after an app run that saves Preferences, `ls -l` on `settings.json` shows `-rw-------`.

### Step 2: Password-style token input

In `renderPrefs` (`frontend/index.html:1050-1056`):
- Change the input to `type="password"` (keep id, class, value binding).
- Add a reveal control next to it: a small `<label><input type="checkbox" id="prefTokenShow"> show</label>`; a delegated listener (the prefs modal already has one for its buttons — put this in the same handler that processes `data-*` clicks, or a direct listener attached right after `box.innerHTML` is set) toggles `prefToken.type` between `"password"` and `"text"`.
- The save path reads `document.getElementById("prefToken").value` — unchanged by the type switch; confirm and leave it.

UI copy rule: no em dashes.

**Verify** (app run): Preferences shows dots for a stored token; "show" reveals it; saving still persists (re-open Preferences → token retained); What's New changelogs still load with the token active.

## Test plan

- `store.rs` test (unix): write settings via `set_settings` into a temp dir, `assert_eq!(perm.mode() & 0o777, 0o600)` on the resulting file. Model after existing temp-dir store tests in the file.
- Manual app checks in Step 2's Verify.
- Rotation note for the operator (not a code step): any token that has lived in the 0644 file should be treated as exposed to local readers and rotated at github.com; mention this in the PR/commit description so the maintainer acts on it.

**Verification**: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; the mode test exists and passes
- [ ] `grep -rn "set_permissions" src-tauri/src/store.rs` → present in `write_json` (and `Store::new` migration)
- [ ] `grep -n 'id="prefToken"' frontend/index.html` → the input is `type="password"` with a working reveal toggle
- [ ] `ls -l` on a freshly saved `settings.json` shows `-rw-------`
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated
- [ ] Commit/PR description includes the rotation recommendation

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond plan 003's expected `write_json` shape.
- Setting permissions breaks the store on some path (e.g. the app-data dir itself is unwritable) — the calls are best-effort `let _ =`; if tests show writes failing BECAUSE of the mode change, report.

## Maintenance notes

- Follow-up recorded for the maintainer: macOS Keychain via the `security` CLI or a keychain crate would remove the at-rest plaintext entirely; revisit if the token ever gains scopes beyond public-API rate limiting.
- Reviewer: confirm the reveal toggle does not log or copy the token anywhere; it only flips the input type.
