# Plan 023: Build the one-click uninstall verb

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. Touch
> only the files listed as in scope. If any STOP condition occurs, stop and
> report. When done, update the status row in `plans/README.md`.
>
> **Gating**: This plan targets the codebase AFTER the 18-plan audit chain lands
> (PR #7, branch `advisor/018-disclosure`). Do NOT execute against `main` until
> that merges. Branch from the merge commit. The design this implements was
> approved by the maintainer (greenlit 2026-08-08).
>
> **Drift check (run first)**: `git diff --stat ac3389e..HEAD -- src-tauri/src/ops.rs src-tauri/src/lib.rs frontend/index.html`

## Status
- **Priority**: P2 | **Effort**: M | **Risk**: MED (first destructive op) | **Category**: direction (build)
- **Depends on**: PR #7 (the 18-plan chain) merged to main. Conceptually builds on 003/004/016.
- **Planned at**: commit `ac3389e` (chain tip), 2026-08-08

## Why this matters

Uninstall is the only missing verb. The malicious-package card in What's New computes the exact removal command and then tells the user to paste it into a terminal, so the app's own security feature dead-ends outside the app. This adds a real remove op through the existing Transfers path (streamed output, honest exit code, history).

## Design decisions (approved — inlined so you need no other doc)

1. **Commands**: `npm rm -g <pkg>`; `pip uninstall -y <pkg>` (the `-y` is required: `run_op` pipes no stdin, so an interactive `Proceed (y/n)?` prompt would hang the transfer forever); `brew uninstall <pkg>`.
2. **Brew dependents**: hybrid. When the confirm modal opens for a brew row, call a new `brew_uninstall_check` that runs `brew uses --installed <pkg>`; if it returns dependents, list them and disable the Uninstall button with a reason. If the check is empty OR fails to run, do not block — let the real `brew uninstall` be the source of truth (its stderr streams honestly on failure).
3. **Confirm modal**: reuse the existing `#modalBack`/`#modalBox` Win98 chrome (as `showUpdateModal` does). Title `Uninstall <name>?`, body shows the exact command in `<code>`, the eco badge + installed version, a pin note if pinned, the dependents list if brew-blocked. Buttons: `Cancel` and a destructive `Uninstall`. Add one `.btn.danger` CSS rule (`color:var(--red);font-weight:bold`). No em dashes.
4. **History**: new `action: "remove"`; keep `to: String`, encode as `to: String::new()`. Frontend renders the version column as `1.2.3 → (removed)` and labels it "removed". Roll back on a remove entry reinstalls `from` — this needs NO backend change (the existing `canRoll` condition already admits it for npm/pip; brew stays excluded, which is honest).
5. **Row afterlife**: on a successful remove, optimistically set the tool's `installed` to null (offline row with an Install button), mirroring the existing optimistic-update pattern in the `transfer-done` handler.
6. **Surfaces**: the malicious card (primary — button replaces the dead-end, keep the copyable command as a fallback line); the library row context menu (`Uninstall...`, disabled when `!t.installed`; manual rows already branch out; npx rows show it disabled with a reason). **Safety rails**: a pin does NOT block uninstall (surface it in the modal; a successful remove also clears the pin via `set_pin(pkg,false)`); a hard-coded self-toolchain exclusion refuses to remove npm/corepack (npm), pip/setuptools/wheel (pip) — button greyed with a reason, checked before the modal opens.

## POST-CHAIN reconciliation (the design was written against bb85e05; these changed underneath it — verify against live code)

- `ops.rs::build_command` now has `valid_pkg(eco,pkg)` / `valid_version(v)` gates at the top and `--` end-of-options markers (plan 013). The `("npm","remove")` and `("pip","remove")` arms must pass `pkg` through `valid_pkg` too, and use `--` (`["rm","-g","--",pkg]`, `["uninstall","-y","--",pkg]`). brew has no reliable `--`; rely on the gate.
- The new remove arms MUST be placed BEFORE the existing `("npm", _)` / `("pip", _)` catch-alls (Rust matches top-to-bottom).
- `run_op` now emits the resolved command as the first `transfer-line` (`$ prog args`, plan 016) and rejects duplicate in-flight ops (plan 003). Remove ops get both for free. Do not rebuild a display command in JS — the header comes from the backend line (plan 016 removed the JS `cmd` reconstruction).
- `queueTransfer(t, target, action)` now takes a TOOL OBJECT (not an index) and has an in-flight dup guard (plan 004). Call it `queueTransfer(t, "", "remove")`. Because plan 016 made the row header come from the backend `$ ...` line, an empty `target` is fine for display.
- `transfer-done` handler resolves the tool via `findToolIdx(x.pkg, x.eco)` (plan 004), not an index. The remove branch sets that tool's `installed=null`.
- Settings gained `probe_manual` (014) and `advisory_checks` (018) — irrelevant here, but if you add a field, follow that serde-default pattern.

## Commands you will need
| Purpose | Command | Expected |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 (147+ on the merged chain) |
| Targeted | `cd src-tauri && cargo test ops:: && cargo test store::` | pass |
| JS gate | `awk '/<script>/{f=1;next}/<\/script>/{f=0}f' frontend/index.html > /tmp/s.js && node --check /tmp/s.js` | exit 0 |
| fmt/clippy | `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings` | clean |

## Scope
**In scope**: `src-tauri/src/ops.rs` (remove arms + `brew_dependents` + tests), `src-tauri/src/lib.rs` (`brew_uninstall_check` command + registration), `src-tauri/src/store.rs` (doc comment + one history test), `frontend/index.html` (`.btn.danger` CSS, `showUninstallConfirm`, malicious-card button, libMenu item, transfer-done remove branch, history render for remove).
**Out of scope**: npx cache clearing (show a disabled item with a reason); any change to install/update/rollback behavior; the appetite dial.

## Git workflow
- Branch: `advisor/023-uninstall` from the merged chain tip.
- Commits: `feat(ops): remove verb and brew dependents check`, `feat(ui): uninstall confirm modal and surfaces`, `docs: history action includes remove`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Backend remove arms + dependents check
Add the three `remove` arms to `build_command` (before the catch-alls, through `valid_pkg`, with `--` for npm/pip). Add `fn brew_dependents(pkg: &str) -> Vec<String>` running `brew uses --installed <pkg>`, returning `Vec::new()` on any spawn/exit/parse failure. Add `#[tauri::command(async)] fn brew_uninstall_check(pkg: String) -> Vec<String>` and register it in `generate_handler!`.
**Verify**: `cargo test ops::` → new tests pass: `npm_remove_builds_rm_g`, `pip_remove_uses_yes_flag_and_given_binary`, `brew_remove_builds_uninstall`, plus `brew_dependents` parse tests (empty stdout → empty, multi-line → N entries).

### Step 2: History encoding
Update the `HistoryEntry.action` doc comment to include `"remove"`. Add a store test round-tripping a `remove` entry with `to: String::new()`.
**Verify**: `cargo test store::` → pass.

### Step 3: Self-toolchain exclusion + confirm modal
In `frontend/index.html`: add `.btn.danger`. Add `showUninstallConfirm(t)` (takes the tool object): if the pkg is in the exclusion list (npm: npm/corepack; pip: pip/setuptools/wheel), open the modal with Uninstall disabled and the reason; for brew rows, call `brew_uninstall_check` and disable with the dependents list if non-empty; otherwise show the enabled destructive button. On confirm: `queueTransfer(t, "", "remove")`, and after a successful remove clear the pin if set.
**Verify**: node --check.

### Step 4: Surfaces + afterlife + history render
Malicious card: replace the plain-text remove line with an Uninstall button wired to `showUninstallConfirm`, keeping the `<code>` command as a fallback line. `libMenu`: add `Uninstall...` (disabled when `!t.installed`; npx disabled-with-reason). `transfer-done`: branch on `x.action==="remove"` to set the resolved tool's `installed=null` (via `findToolIdx`). History render: label `"remove"` as "removed" and render `from → (removed)`.
**Verify**: node --check; `cd src-tauri && cargo test` still green.

### Step 5: Manual verification (HUMAN — reproduce in report)
Install a throwaway npm package (e.g. `cowsay`), then Uninstall it via the library context menu → confirm modal shows `$ npm rm -g -- cowsay`, runs, streams, row goes offline, history shows "removed", Roll back reinstalls it. Then a brew formula with a dependent → Uninstall disabled with the dependents reason. Then verify `npm` itself shows Uninstall greyed with the self-toolchain reason.

## Done criteria
- [ ] `cd src-tauri && cargo test` exits 0; the remove-arm + dependents + history tests exist and pass
- [ ] `brew_uninstall_check` registered in `generate_handler!`
- [ ] `grep -n '"remove"' src-tauri/src/ops.rs` → the three arms present, before the catch-alls
- [ ] Malicious card has a real Uninstall button; libMenu has the item; self-toolchain exclusion works
- [ ] node --check passes; fmt+clippy clean
- [ ] Manual checklist reproduced
- [ ] `plans/README.md` status row updated

## STOP conditions
- PR #7 has not merged (this plan targets the post-chain codebase).
- `build_command`'s `valid_pkg`/`--`/catch-all structure differs from plan 013's (drift) — reconcile before adding arms.
- `queueTransfer` is not the tool-object form from plan 004 (drift).

## Maintenance notes
- The `.btn.danger` + confirm-modal pattern is reusable for any future destructive verb (cache purge, config reset).
- Reviewer: confirm the remove arms sit before the catch-alls, and that a failed brew precheck falls through to a real streamed uninstall rather than silently blocking.
