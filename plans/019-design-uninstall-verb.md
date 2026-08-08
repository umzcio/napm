# Plan 019: Design the one-click uninstall verb (design plan, then a small build)

> **Executor instructions**: This is a DESIGN plan: the deliverable is a written
> design (committed as `docs/design/uninstall.md`) answering the questions below,
> plus a maintainer sign-off, before any build. A build outline is included so
> the design stays grounded, but do not implement until the design is approved.
> If anything in the "STOP conditions" section occurs, stop and report. When
> done, update the status row in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/ops.rs frontend/index.html docs/ROADMAP.md`

## Status

- **Priority**: P2 (highest-leverage direction item)
- **Effort**: M (coarse: S design + S-M build)
- **Risk**: MED (first genuinely destructive operation the app runs)
- **Depends on**: plans/004-frontend-transfer-identity.md and plans/003-store-atomicity-and-op-serialization.md (the execution path it reuses); plans/016 (real-command display) is a natural companion
- **Category**: direction
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Uninstall is the only missing verb. Install, update, rollback, and promote all run through the real Transfers path with streamed output and history; remove does not exist. The gap is sharpest at the product's most urgent moment: when the OSV scan flags an installed package as MALICIOUS with no fixed version, the What's New card computes the exact removal command and then tells the user to copy it into a terminal — the app's own security feature dead-ends outside the app. The roadmap explicitly parked "a one-click uninstall op" in the M5 deferral list; of that batch, it is the piece still missing.

## Evidence (inline, verified)

- `docs/ROADMAP.md` (M5 "Deferred to v1.5"): "Deferred to v1.5: issue-velocity `hold`, brew/system-tool CVE mapping (Debian/Alpine), **a one-click uninstall op**, and the appetite dial's 'security-only' far-left notch." (issue-velocity has since shipped, per the "Deferred polish" section.)
- `frontend/index.html:588-589` — the malicious card renders the copy-paste workaround: `'No safe version published. Remove it: <code>'+esc(removeCmd(it))+'</code>'`; `removeCmd` (`:612-616`) builds `npm rm -g` / `pip uninstall` / `brew uninstall` strings.
- `src-tauri/src/ops.rs:4-31` — `build_command` has npm/pip/brew/npx arms for install/update/rollback/promote; the `_ => None` arm swallows any "remove" action today.
- The streaming/history machinery to reuse: `ops::run_op` (spawn + `transfer-line`/`transfer-done` + `add_history`), history store (`store.rs:87-91`, entries `{ts,pkg,eco,action,from,to}` — `action` is currently `"install" | "update" | "rollback"`).

## Design questions the document must answer

1. **Commands per ecosystem**: `npm rm -g <pkg>`; `pip uninstall -y <pkg>` (the `-y` is required for a non-interactive run — is auto-confirming pip acceptable, given napm shows its own confirmation first?); `brew uninstall <pkg>` — which FAILS when another formula depends on it. Decide: surface brew's dependency failure as the honest streamed error (recommended, matches the house style), or pre-check `brew uses --installed` and disable with a reason?
2. **Confirmation UX**: the first destructive verb needs a confirm step. Win98-style modal with the exact command shown, package name typed? A simple Yes/No with the command visible is probably right for the era styling; define copy (no em dashes).
3. **History semantics**: new `action: "remove"` with `from: <installed>, to: null`? The `to` field is currently `String` (non-null) — decide the encoding (`to: ""`?) and what Roll back means on a remove entry (reinstall `from` — which npm/pip support; brew supports plain reinstall of current). This is the natural "undo".
4. **Row afterlife**: after a successful remove, does the row disappear on the next scan (it will) and is that enough, or should the transfer completion optimistically mark it `offline` (npm/pip rows with `installed: null` render as offline with an Install button — a nice symmetry)?
5. **Where the verb surfaces**: the malicious-card button (primary motivation — replaces the copy-paste hint when removal is chosen); the library row context menu ("Uninstall..."); anywhere else? npx rows: clearing an npx cache dir is a different operation — in or out of scope (recommend out, labeled)?
6. **Safety rails**: pinned packages — does a pin block uninstall (recommend: no, but the confirm modal notes the pin)? Should napm refuse to uninstall itself or its own toolchain (npm removing npm)? List the exclusions.

## Build outline (after design sign-off)

- `ops.rs`: `("npm","remove")`, `("pip","remove")`, `("brew","remove")` arms + tests (model on `ops.rs:136-172`); route through the plan 013 validation gate if landed.
- History: extend the accepted `action` values; frontend `renderHistory` label ("removed") and rollback-as-reinstall wiring.
- Frontend: confirm modal, malicious-card button branch (keep the copyable command as the fallback when the user declines), context-menu item, transfer row via the standard path.
- Verification: `cd src-tauri && cargo test`; manual run installing and removing a throwaway package (e.g. npm `cowsay`) end to end, plus a brew dependency-failure case observed streaming honestly.

## Done criteria (design phase)

- [ ] `docs/design/uninstall.md` exists answering all six questions with a chosen option and rationale each
- [ ] Maintainer has approved the design (recorded at the top of the doc)
- [ ] `plans/README.md` status row updated (design DONE; build tracked as a follow-up row)

## STOP conditions

- The maintainer rejects auto-confirm for pip (`-y`) — the verb may need a different pip flow (streamed interactive is not supported by the current ops path); report options rather than improvising.
- Any decision here conflicts with a roadmap entry newer than `bb85e05`.

## Maintenance notes

- The confirm-modal pattern built here is reusable for any future destructive verb (cache purges, config resets).
- The malicious-card flow should keep working with zero clicks removed even when the user declines the in-app uninstall (the copyable command stays as the fallback).
