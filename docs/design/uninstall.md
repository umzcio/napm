Sign-off: PENDING maintainer review

# Design: the one-click uninstall verb

## Why this matters

Install, update, rollback, and promote all run through the real Transfers
path (`ops::run_op`, streamed output, history). Remove does not exist. The
gap is sharpest at the app's most urgent moment: when the OSV scan flags an
installed package as `malicious` with no fixed version, the What's New card
computes the exact removal command and then asks the user to copy it into a
terminal. The app's own security feature dead-ends outside the app. M5's
roadmap entry explicitly parked "a one-click uninstall op" for v1.5
(`docs/ROADMAP.md`, line 99, "Deferred to v1.5" list).

## Evidence base (read and verified this session)

- `frontend/index.html:588-589` (the malicious card's action line): when a
  package is flagged malicious with no fix, it renders `No safe version
  published. Remove it: <code>` + `removeCmd(it)` + `</code>`, a plain string
  the user has to copy by hand.
- `frontend/index.html:612-616`: `removeCmd(it)` already builds the correct
  per-ecosystem command string (`npm rm -g`, `pip uninstall`, `brew
  uninstall`), but only for display. Nothing executes it.
- `src-tauri/src/ops.rs:4-31`: `build_command` matches `(eco, action)`.
  `("npm", _)` and `("pip", _)` match **any** action and always build the
  install/rollback shape (`npm i -g pkg@version`, `pip install pkg==version`).
  Brew only matches `("brew", "install") | ("brew", "update")`. Everything
  else, including any future `"remove"` action, falls through to `_ => None`,
  which `run_op` (below) treats as a failed op with no command run.
- `src-tauri/src/ops.rs:58-134` (`run_op`): spawns the built command with
  `stdout(Stdio::piped())` and `stderr(Stdio::piped())` but **no stdin
  redirection**, streams `transfer-line` events per output line, emits one
  `transfer-done` with the exit code, and on success calls
  `store.add_history(...)`. This is directly reusable for remove with no
  structural change, and the missing stdin pipe is load-bearing for Q1 below:
  an interactive child process reading a stdin prompt would hang forever with
  no way to answer it.
- `src-tauri/src/ops.rs:136-172`: one `#[test]` per command shape
  (`npm_install_pins_version`, `pip_uses_double_equals_and_given_binary`,
  `brew_installs_without_a_version`, `npx_promote_installs_globally_via_npm`,
  `brew_rollback_is_unsupported`). New remove arms need matching tests in the
  same style.
- `src-tauri/src/store.rs:7-15`: `HistoryEntry { ts, pkg, eco, action, from,
  to }`, `action: String` documented inline as `"install" | "update" |
  "rollback"`, `to: String` (not `Option<String>`).
- `src-tauri/src/store.rs:87-91`: `add_history` appends and rewrites the
  JSON file; no validation on `action` or `to`, so a new action value or a
  new `to` convention needs no store-layer change.
- `frontend/index.html:699-711` and `:718-731`: the History pane already
  computes `canRoll = h.action!=="rollback" && h.from && h.eco!=="brew"` in
  two places (render + context menu) and reuses `queueTransfer(ti, h.from,
  "rollback")` to roll back any history entry that qualifies. This condition
  already covers a hypothetical `action:"remove"` entry with no code change,
  which matters for Q3 below.
- `frontend/index.html:907-915`: the `transfer-done` handler is the existing
  "optimistic UI" precedent: on success it sets `TOOLS[x.ti].installed =
  x.to` directly, then re-renders, without waiting for a rescan. This is the
  precedent Q4 follows.
- `frontend/index.html:783-821` (`libMenu`): the existing per-row context
  menu, built from an `openPopup` item array, already special-cases
  `eco==="manual"` into its own filesystem-only branch and already disables
  brew rollback in place with a reason string, both direct precedent for how
  uninstall should slot in.
- `frontend/index.html:286-287`, `:1044-1075`: the existing Win98 modal
  chrome (`#modalBack` / `#modalBox`, `.m-h` / `.m-b`, `.btn` / `.btn.primary`
  / `data-close`), used unmodified by `renderPrefs`, `showAbout`, and
  `showUpdateModal`. No destructive/danger button style exists yet.
- `frontend/index.html:14-17`: CSS custom properties include `--red:#aa0000`
  already used for `.signal.danger` and `.act-rollback`-adjacent states, the
  natural color for a new destructive button/history variant.
- Confirmed locally: `brew` is installed (`Homebrew 6.0.15-146-g8ea475e`),
  so `brew uses --installed <formula>` is a real, invocable command for the
  Q1 precheck design, not a hypothetical.

No evidence citation in the plan failed to match the file it referenced.
Nothing below required a STOP.

## The six questions

### 1. Commands per ecosystem

**npm:** `npm rm -g <pkg>`. Matches `removeCmd` today; no debate.

**pip:** `pip uninstall -y <pkg>`.

Recommendation: yes, `-y` is not just acceptable, it is close to mandatory.
`run_op` does not pipe stdin to the child process (`ops.rs:84-99`, only
stdout/stderr are piped). Bare `pip uninstall` prompts `Proceed (y/n)?` on
an interactive terminal; with no stdin attached, that prompt reads EOF or
hangs, and the transfer row would sit "running" forever with no exit code.
napm's own confirm modal (Q2) is the real confirmation gate; a second,
un-answerable shell-level prompt would be a bug, not a safety feature.

**brew:** `brew uninstall <pkg>`, which fails with a nonzero exit when
another installed formula depends on it.

The plan poses two options: let it fail and stream the honest error
(described in the plan as "recommended, matches house style"), or pre-check
`brew uses --installed <pkg>` and disable with a reason.

Recommendation: a hybrid, weighted toward the precheck as the primary UX,
with honest streaming kept as the backstop. Rationale: CLAUDE.md is explicit
for the parallel brew-rollback case ("Do not offer an action that will fail.
Surface the limitation in the UI"), and the shipped app already lives by
that rule (`canRoll` disables brew rollback in three places rather than
letting `npm i -g` / equivalent fail live). Dependents are just as knowable
in advance as "brew keeps no old bottles" is, via one cheap `brew uses
--installed` call. Concretely:

- When the confirm modal opens for a brew row, run `brew uses --installed
  <pkg>` (new backend command, see Build outline). If it returns one or more
  dependents, list them in the modal and disable the Uninstall button with a
  reason ("`ripgrep` is required by `fd`. Remove `fd` first, or use a
  terminal.").
- If the check returns empty, or the check itself fails to run (brew
  missing, timeout, parse error), do not block. Proceed to a normal
  `run_op`, and let `brew uninstall` be the actual source of truth. A failed
  uninstall (including a race where a dependent appeared between the
  precheck and the run) streams its real stderr and a nonzero exit, exactly
  like any other failed transfer.

This keeps the common case honest and non-doomed (matches the rollback
precedent) while never fabricating a guarantee the precheck can't make
(matches the streaming-honesty principle). A pure "let it fail" design was
considered and rejected only because it repeats a mistake CLAUDE.md already
flagged and the app already fixed once for brew rollback.

### 2. Confirmation UX

Reuse the existing modal exactly (`#modalBack`/`#modalBox`, `.m-h`/`.m-b`,
`data-close`), matching `renderPrefs` / `showUpdateModal` structurally so it
reads as the same app, not a bolted-on dialog.

Content:
- Title: `Uninstall <name>?`
- Body: the exact command that will run, in `<code>` (reuse `removeCmd`
  verbatim, it already exists and is already correct per ecosystem).
- The eco badge and installed version, so the user isn't guessing what will
  be removed.
- If the tool is pinned: a note, "This tool is pinned. Uninstalling will
  also remove the pin." (see Q6).
- If brew and the precheck (Q1) found dependents: the dependent list,
  Uninstall disabled, with the reason inline.
- Buttons: `Cancel` (default, `data-close`, mirrors every existing modal)
  and a destructive button labeled `Uninstall`. No em dashes anywhere in
  this copy, per house style.

New CSS: a `.btn.danger` variant (`color:var(--red); font-weight:bold;`),
since no destructive button style exists in the current sheet (`.btn.primary`
only bolds text; Win98 chrome keeps every button silver). This is the
smallest possible visual distinction that still reads as "the dangerous one"
without breaking the bevel look.

### 3. History semantics

New `action: "remove"`, added to the existing informal enum (currently
`"install" | "update" | "rollback"`, `store.rs:12`).

`to` encoding: keep `to: String` as-is (no store schema change, no migration
of old history.json files needed) and use `to: String::new()` (empty
string) to mean "nothing installed after this action." This is the smallest
change that fits the existing non-nullable type. The frontend already
special-cases the label per action (`h.action==="update"?"updated":...`,
`index.html:702`); add `"remove"` there ("removed") and special-case the
version column so a remove entry renders `1.2.3 → (removed)` instead of a
bare trailing arrow: `esc(h.from||"\u2014")+' → (removed)'` instead of
blindly appending `esc(h.to)`.

Roll back on a remove entry: reinstall `from`. This needs **no backend
change**. `canRoll = h.action!=="rollback" && h.from && h.eco!=="brew"`
(`index.html:703`, `:720`) already admits any non-rollback action with a
populated `from` on a non-brew ecosystem, so a `"remove"` entry with
`from:"1.2.3"` already qualifies today, and `queueTransfer(ti, h.from,
"rollback")` already builds `npm i -g pkg@1.2.3` / `pip install
pkg==1.2.3` via the existing `build_command` npm/pip arms, which ignore the
action string entirely (`ops.rs:12-19`, matched on `("npm", _)` / `("pip",
_)`). Brew stays excluded by the same `eco!=="brew"` guard, which is
correct here too: `brew install <pkg>` after a removal reinstalls whatever
is currently latest, not necessarily the exact removed version, so treating
it as unavailable is honest, not just consistent.

### 4. Row afterlife

Recommendation: optimistically mark the row offline immediately on a
successful remove, matching the exact pattern already shipped for
install/update/rollback. `transfer-done`'s success handler already does
`TOOLS[x.ti].installed = x.to` with no rescan (`index.html:911`). Extend it:
`if (x.action==="remove") TOOLS[x.ti].installed = null; else
TOOLS[x.ti].installed = x.to;`. Since `installed: null` already means
"not installed" throughout the data model and rendering (offline row, shows
an Install button per the CLAUDE.md data model), this requires no new
rendering logic, only the one branch in the success handler. A background
rescan will eventually confirm it; there is no reason to make the user wait
for one, since every other transfer already trusts its own exit code over a
fresh scan.

### 5. Where the verb surfaces

**Malicious card (primary).** Replace the plain text-plus-`<code>` line at
`index.html:588-589` with an actual `Uninstall` button that opens the Q2
confirm modal for that tool. Keep the copyable command visible too (a small
line under or beside the button), for the brew-blocked case and for users
who want to run it themselves regardless. This matches the plan's framing:
the button replaces the dead-end, the copy-paste path survives as a
fallback rather than being deleted.

**Library row context menu.** Add an `Uninstall...` item to `libMenu`
(`index.html:783-821`), positioned after Pin/Unpin and the copy actions,
before "Open ... page" or "What's New for this" (grouped with the other
destructive/state-changing actions, near Roll back). Disabled when
`!t.installed`. For `eco==="manual"` rows, no change: they already branch
into their own filesystem-only menu (Reveal in Finder / Copy path, etc.)
before reaching this code, and manual/unmanaged tools have no package
manager to uninstall through, so Uninstall does not apply and should not
appear.

**npx cache-clearing: out of scope**, labeled. `npx` rows are a run cache,
not a persistent global install; there is no single canonical "clear an npx
package's cache" command across npm versions (it is typically a manual
`rm -rf` under `~/.npm/_npx`), and CLAUDE.md's own honesty rule ("do not
fake what is not technically possible") argues against inventing one for
v1. Recommendation: the library context menu shows no Uninstall item for
npx rows (or shows one, disabled, with the reason "npx cache clearing not
supported yet"); either is acceptable, disabled-with-reason is slightly
more consistent with the brew-rollback precedent of never silently omitting
an action the user might expect. Left as an implementer's choice, called
out in Notes below.

### 6. Safety rails

**Does a pin block uninstall?** Recommendation: no. A hard block would be
one more thing standing between the user and removing a package OSV just
flagged as malicious, which is precisely the case this whole feature exists
for. Instead, the confirm modal surfaces the pin (Q2), and a successful
remove also clears the pin (`set_pin(pkg, false)`) as part of the same
flow, so a pinned-but-now-uninstalled package doesn't leave a stale pin
haunting a row that no longer exists. This is a deliberate product choice,
not something already implied by existing code, flagged in Notes.

**Should napm refuse to uninstall its own toolchain?** Recommendation: yes,
a small hard-coded exclusion list, checked before the confirm modal even
opens (button greyed with a reason, same pattern as brew rollback):

- npm ecosystem: `npm`, `corepack` (ships bundled with npm/node; removing it
  out from under npm is a self-inflicted footgun with no upside).
- pip ecosystem: `pip`, `setuptools`, `wheel` (pip's own foundation;
  `pip uninstall pip` is technically possible and technically breaks `pip`
  itself for no benefit napm can offer here).
- brew ecosystem: no equivalent needed structurally. `brew` itself is not a
  formula you `brew uninstall`, so there is no direct self-removal case to
  guard against the way there is for npm/pip.
- napm itself: not applicable today. napm ships as a signed, notarized
  `.app` (`docs/ROADMAP.md` M10a/M10b), not as an npm/brew/pip package, so
  it never appears as a row in its own Shared Library and this guard has
  nothing to catch. Worth re-checking if that packaging story ever changes.

A known residual gap, out of scope for v1: this guard only stops napm
uninstalling the package manager binaries directly. It cannot see that, say,
a brew-installed `node` is load-bearing for an npm the user relies on,
because npm packages aren't nodes in brew's dependency graph. The Q1
`brew uses --installed` precheck only protects brew-to-brew dependencies. Not
fixing this now; flagged for the maintainer.

## Build outline

**`src-tauri/src/ops.rs`**
- Add explicit `("npm", "remove")`, `("pip", "remove")`, `("brew", "remove")`
  match arms to `build_command`, placed *before* the existing `("npm", _)`
  and `("pip", _)` catch-alls (Rust matches top to bottom, so order matters
  here since those catch-alls would otherwise shadow the new arms):
  - `("npm", "remove") => Some(("npm", vec!["rm", "-g", pkg]))`
  - `("pip", "remove") => Some((pip_bin, vec!["uninstall", "-y", pkg]))`
  - `("brew", "remove") => Some(("brew", vec!["uninstall", pkg]))`
- New tests in the existing `#[cfg(test)] mod tests` style
  (`ops.rs:136-172`): `npm_remove_builds_rm_g`,
  `pip_remove_uses_yes_flag_and_given_binary`,
  `brew_remove_builds_uninstall`. One per new arm, mirroring the existing
  one-behavior-per-test convention.
- New function (same file or a small `ops::brew` submodule): a
  `brew_dependents(pkg: &str) -> Vec<String>` that runs `brew uses
  --installed <pkg>`, splits stdout on newlines, and returns an empty `Vec`
  on any spawn/exit/parse failure (never fabricate an empty-means-safe
  result from a broken check; the frontend should treat a failed check the
  same as "let the real uninstall be the judge," per Q1). One or two tests
  around the parsing (empty stdout, multi-line stdout), command construction
  can't be meaningfully unit-tested without shelling out for real.

**`src-tauri/src/lib.rs`**
- New `#[tauri::command(async)] fn brew_uninstall_check(pkg: String) ->
  Vec<String>` wrapping `ops::brew_dependents`, registered in the
  `generate_handler!` list alongside the existing commands (line 236).
  `run_op` itself needs no signature change; `"remove"` is just a new valid
  value for the existing `action: String` parameter.

**`src-tauri/src/store.rs`**
- Update the inline doc comment on `HistoryEntry.action` (line 12) to
  `"install" | "update" | "rollback" | "remove"`. No type or schema change.
- One new test mirroring `history_appends_newest_first` that round-trips a
  `remove` entry with `to: String::new()`, to lock in the encoding decided
  in Q3.

**`frontend/index.html`**
- New `.btn.danger` CSS rule near the existing `.btn` / `.btn.primary` rules
  (around line 68-75).
- New `.act-remove{color:var(--red);}` alongside the existing
  `.act-update`/`.act-rollback`/`.act-install` line (156).
- New `showUninstallConfirm(ti)` function, modeled on `renderPrefs` /
  `showUpdateModal` (`:1045-1075`, `:1108-1116`): builds the modal body per
  Q2, calls `brew_uninstall_check` for brew rows before enabling the
  button, and on confirm calls `queueTransfer(ti, "", "remove")` (or a
  small variant of `queueTransfer` that tolerates an empty target for
  remove, since the current signature assumes `target` is a version string
  used to build a display command (`removeCmd` should be used for the
  display command when `action==="remove"` instead of the version-based
  `cmd` construction at `index.html:740-743`).
- Malicious card: replace `index.html:588-589`'s plain text with a button
  wired to `showUninstallConfirm`, keeping the `<code>` command as a
  fallback line.
- `libMenu` (`:783-821`): add an `Uninstall...` item wired to
  `showUninstallConfirm(i)`, disabled when `!t.installed`, following the
  existing exclusions for `eco==="manual"` (already branches out earlier)
  and the Q5 decision for `eco==="npx"`.
- `transfer-done` handler (`:907-915`): branch on `x.action==="remove"` to
  set `TOOLS[x.ti].installed = null` instead of `= x.to`, and, when the row
  was pinned, call `set_pin(pkg, false)` and clear `TOOLS[x.ti].pinned`
  locally (Q6).
- History rendering (`:699-711`): add the `"remove"` → `"removed"` label
  case and the `(removed)` version-column special case (Q3).

**Verification**
- `cargo test` for the new `ops.rs` and `store.rs` tests above.
- Manual end-to-end with a throwaway npm package (e.g. `cowsay`, already
  used elsewhere in this codebase's conventions for a disposable global
  tool): install it, confirm it appears in the library, Uninstall it from
  napm, confirm the streamed output and exit code, confirm the row flips to
  offline with an Install button with no rescan needed, confirm a `remove`
  history entry appears with the correct `from` and `to:""`, confirm
  History's Roll back reinstalls the exact prior version.
- Manual brew dependency-failure case: install a brew formula with a known
  installed dependent, open the confirm modal on the depended-upon formula,
  confirm the precheck lists the dependent and disables Uninstall with the
  reason shown; separately, verify a formula with no dependents proceeds
  normally through `run_op` and streams a real success or failure.

## Done criteria (from the plan)

- This document exists at `docs/design/uninstall.md`, answers all six
  questions with a chosen option and rationale each, `Sign-off: PENDING
  maintainer review` at the top. Done.
- No source code modified. `git status` in this worktree shows only this
  new file.

## Notes for the reviewer

- Q1 (brew precheck vs. let-it-fail) is the one place I deliberately did not
  take the plan's parenthetical hint at face value. The plan text next to
  option (a) reads "(recommended, matches house style)"; I chose the hybrid
  instead, weighted toward the precheck, because CLAUDE.md's explicit
  instruction for the structurally-identical brew-rollback case ("do not
  offer an action that will fail") and the app's own shipped behavior for
  that case both point the other way. Flagging this explicitly since it is
  a real disagreement with the plan's suggested framing, not just a filled-in
  blank.
- Q5's npx question (disabled-with-reason vs. simply absent from the menu)
  is left as a coin-flip for the implementer; both are consistent with
  house style and neither changes any other part of the design.
- Q6's "pin is cleared on remove" is a product decision I made, not
  something forced by existing code. The alternative (leave the pin, let it
  dangle until the user notices) is defensible too, but a lingering pin on
  a package that no longer exists in the library seemed like clutter with
  no upside; worth the maintainer's explicit sign-off either way.
- The `queueTransfer` signature currently assumes its `target` argument is a
  version string used both to drive the operation and to build the
  human-readable command shown in the Transfers row (`index.html:740-746`).
  Remove has no target version, so the implementer will need to either pass
  an empty string and special-case the display-command construction (as
  sketched above), or thread a small `label`/`cmd` override through. This is
  a real but small refactor, not a design question, called out here so it
  isn't a surprise during implementation.
