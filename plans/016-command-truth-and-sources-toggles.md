# Plan 016: Show the command that actually ran, and untangle the two "Sources" toggles

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/ops.rs frontend/index.html`
> Plans 003/004/007/011/013 legitimately touch these; reconcile against their
> diffs. Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/004-frontend-transfer-identity.md (transfer records carry pkg+eco)
- **Category**: tech-debt
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Two honesty gaps in an app whose brand is honesty:

1. **The Transfers pane displays a command string rebuilt in JS, not the command Rust ran.** For every pip operation the pane shows `pip install ...` while the backend actually resolved and ran `pip3` (`pip_bin()`); and when `build_command` returns `None` (unsupported eco/action), the backend emits a failed `transfer-done` with NO output line, so the UI shows a fabricated command, an empty log, and a bare failure glyph — three pieces of wrong information about one event. There are three separate JS constructions of this same string (`queueTransfer`'s `cmd`, `installCmd` for the clipboard items, and `removeCmd` for the malicious-package hint).
2. **Two identically-named "Sources" controls do different things.** The View menu's "Source: npm/brew/pip/npx/manual" entries are a client-side DISPLAY filter (localStorage `napm.view`), while Preferences has a "Sources" checkbox set that controls what the backend SCANS (settings.json). Unchecking npm in Preferences empties the library of npm rows while the View menu still shows "Source: npm ✓" — the user has no way to tell which of two same-named switches hid their packages.

## Current state

- `src-tauri/src/ops.rs:72-99` — the spawn path; on `None` it emits only the done event:
  ```rust
  let (prog, args) = match built {
      Some(c) => c,
      None => {
          let _ = app.emit("transfer-done",
              DoneEvent { op_id: op_id.clone(), success: false, code: -1 });
          return;
      }
  };
  ```
  After a successful spawn, lines are emitted as `LineEvent { op_id, stream, line }` (`:107`, `:116`).
- `frontend/index.html:740-743` — the JS reconstruction (always prints `pip`, never `pip3`):
  ```js
  var cmd=(t.eco==="npm"?"npm i -g "+t.pkg+"@"+target
         :t.eco==="pip"?"pip install "+t.pkg+"=="+target
         :t.eco==="brew"?"brew install "+t.pkg
         :"npm i -g "+t.pkg);
  ```
  rendered as `$ <cmd>` in the transfer row (inside `renderXfers`, `:677-693`; the `$ ` prefix markup — locate with `grep -n '\\$ ' frontend/index.html`).
- `frontend/index.html:623-627` — `installCmd(eco,pkg,version)` used by "Copy install command" context-menu items (`:479` search results, `:810` library rows). `removeCmd` at `:612-616` (malicious-card hint). These two are CLIPBOARD/display hints, not records of execution — they stay, with a comment.
- View menu source toggles: `frontend/index.html:968-972` (`{label:"Source: npm", checked:..., run:function(){toggleSource("npm");}}` and the four siblings). `VIEW.sources` persists under localStorage key `napm.view` (`:347-349`).
- Preferences sources checkboxes: `frontend/index.html:1046-1049` (ids `prefSrc_npm` etc.), saved via `set_settings` (`:1085-1093`), consumed by `scan_installed` → `scan_all` gating.
- Get settings into the frontend: `get_settings` command exists (`src-tauri/src/lib.rs:93-96`); check how prefs load them (`grep -n "get_settings" frontend/index.html`) — reuse that path to know which sources are scan-disabled at menu-render time.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test ops::` | pass |
| All tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | checklist below |

## Scope

**In scope**:
- `src-tauri/src/ops.rs` (emit the resolved command line; explanatory line on `None`)
- `frontend/index.html` (drop the display `cmd` reconstruction; View menu relabel + disable logic)

**Out of scope**:
- `installCmd`/`removeCmd` (clipboard/hint helpers; keep, annotate).
- Merging the two persistence layers (localStorage view vs settings) into one — the fix here is clarity, not consolidation; consolidation is a bigger product change.
- `renderXfers` DOM structure (plan 011 owns it).

## Git workflow

- Branch: `advisor/016-command-truth`
- Commits: `fix(ops): transfer rows show the exact command that ran` and `fix(ui): View source filters are labeled as filters and track scan settings`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Backend emits the truth

In `ops.rs`:
- On `None` from `build_command`, before the done event, emit a `transfer-line` (`stream: "stderr"`): `unsupported operation: <eco> <action>` (no em dashes).
- After a successful spawn decision (before the reader threads), emit a first `transfer-line` (`stream: "stdout"`): `$ <prog> <args joined with spaces>`. Compute it from the actual `(prog, args)` pair.

Add/adjust `ops.rs` tests only if you extracted a pure format helper (e.g. `fn display_command(prog,&args)->String` — do extract it, test it with the pip3 case).

**Verify**: `cd src-tauri && cargo test ops::` → pass.

### Step 2: Frontend renders the received command

In `queueTransfer`, drop the `cmd` construction; initialize the record with `cmd:""`. In the `transfer-line` handler, when a line starting with `"$ "` arrives for a transfer whose `cmd` is empty, set `x.cmd = line` (and do not duplicate it in the log body — skip pushing that first line into `lines`, since the row header renders it). The right-click "Copy command" (`:758`) now copies the real command.

Handle the no-backend preview path (`:749`): keep a placeholder `cmd` of `"(not running in napm)"` behavior as-is.

**Verify** (app run): a pip update's row header shows `$ pip3 install ...` (or `pip` if that machine resolves plain pip); an npm row shows the `--` if plan 013 landed; right-click → Copy command copies what the header shows; a forced unsupported op (temporarily invoke `run_op` from the console with `eco:"brew", action:"rollback"`) shows the explanatory line instead of an empty log.

### Step 3: View menu clarity

- Relabel the five entries "Source: X" → "Show: X" (`:968-972`).
- At menu render time, for any source disabled in Preferences (from the settings object the prefs path already fetches — cache it in a `SETTINGS` var on load/save), render the item disabled with label "Show: X (off in Preferences)". The menu engine already supports `disabled:` functions (see `:723-724` for the pattern).

**Verify** (app run): untick brew in Preferences and save → library loses brew rows AND View menu shows "Show: brew (off in Preferences)" greyed; re-enable → normal.

## Test plan

- `ops.rs`: `display_command("pip3", ["install","httpie==3.2.2"])` → `pip3 install httpie==3.2.2`. Existing build_command tests untouched.
- Manual checklist in Steps 2-3.
- Backend regression: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `grep -n 'eco==="npm"?"npm i -g "' frontend/index.html` → no matches (display reconstruction gone)
- [ ] Backend emits `$ ...` first line and the unsupported-operation line (read the diff; app-run confirms)
- [ ] View menu items say "Show: ..." and grey out when the source is scan-disabled
- [ ] `installCmd`/`removeCmd` carry a one-line comment marking them clipboard/hint helpers, not execution records
- [ ] `cd src-tauri && cargo test` exits 0; app-run checklist passes
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond the named plans' expected diffs.
- The `$ `-prefix convention collides with real package-manager output in testing (a tool that prints lines starting `$ `) — switch to carrying the command in a dedicated event payload field instead, and note the change.

## Maintenance notes

- If a dedicated `transfer-start {op_id, cmd}` event ever becomes preferable to the `$ ` line convention, both ends are localized (ops.rs emit + one handler branch).
- Plan 019 (uninstall) adds a destructive verb whose row must show the real command via this same path.
- Reviewer: confirm the first-line skip logic cannot drop a legitimate output line when `cmd` was already set.
