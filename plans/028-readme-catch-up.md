# Plan 028: Bring the README in line with what the app actually does

> **Executor instructions**: Follow this plan step by step. Touch only `README.md`. If any STOP
> condition occurs, stop and report. When done, update the status row in `plans/README.md`.

## Status
- **Priority**: P2 | **Effort**: S | **Risk**: LOW | **Category**: docs
- **Depends on**: plans 023/024/025 (merged to `main` in PR #8)
- **Planned at**: `main` @ 8d6e74e, 2026-08-08

## Why this matters

Three shipped features are absent or contradicted in the README, so the front page of a public repo
now misdescribes the product. The project's own rule is that nothing in the interface is faked or
stale; the README should hold to the same standard.

1. **cargo is a sixth scanned source** but the README says "Five sources" and lists five everywhere.
2. **Uninstall** now exists as a real verb; the README's Transfers section lists only install,
   update, and rollback.
3. **Library import** now exists; the README advertises only "Export library".

## Current state (verified against `main` @ 8d6e74e; line numbers approximate, locate by content)

Stale five-source claims:
- **:11** header tagline: "across npm, Homebrew, pip, npx, and manual installs"
- **:27** the sources badge URL: `sources-npm%20%7C%20brew%20%7C%20pip%20%7C%20npx%20%7C%20manual`
- **:32** the Shared Library screenshot `alt` text
- **:39** the "Why" paragraph's list of what accumulates
- **:50** `- **Five sources, one view**: npm, Homebrew, pip, and npx scanned with one batch command each, plus ... manual / unmanaged`
- **:84** the login-shell PATH capture bullet ("finds `npm`, `brew`, `pip`, and your manual tools")
- **:142** Tech Stack table row: `| **Package sources** | npm, Homebrew, pip, npx via std::process::Command; manual installs via a $PATH sweep |`
- **:170** the ASCII architecture diagram's source boxes (`| npm | brew | pip | npx|`)
- **:240** Project Structure: `scan/ # one module per source (npm/brew/pip/npx/manual)`
- **:257** Design Decisions "Why Tauri and native Rust?" ("run npm, brew, and pip")

Missing features:
- **:70-73** Transfers section: no uninstall. Current rollback bullet says "Rollback for npm
  (`npm i -g pkg@ver`) and pip (`pip install pkg==ver`). Homebrew is gated honestly".
  cargo ALSO supports real rollback (`cargo install --version`), unlike brew.
- **:79** `- **Export library** to JSON or Markdown` with no import counterpart.
- **:64-67** Search section: "Federated across npm and the Homebrew catalog by default" and the
  "Honest about pip" bullet. cargo search is exact-name lookup only (crates.io has no free-text
  search API), the same honest limitation pip has, and it is labeled that way in the UI.
- **:93** the "What napm sends" section ALREADY names crates.io (added with the cargo work) — verify
  and leave it alone.

Facts to describe accurately (do not overstate):
- cargo source reads `.crates2.json` (fallback `cargo install --list`), resolving the install root
  via `CARGO_INSTALL_ROOT` / cargo config / `CARGO_HOME`, not a hardcoded path.
- cargo crates installed from git or a local path have no registry "latest" and honestly show no
  update path.
- Uninstall: `npm rm -g`, `pip uninstall -y`, `brew uninstall`, `cargo uninstall`. Brew dependents
  are pre-checked (`brew uses --installed`) and the action is disabled with the dependent list
  rather than offered and failed. napm refuses to uninstall its own toolchain (npm/corepack,
  pip/setuptools/wheel). Removal is logged to history and can be rolled back (reinstalls the prior
  version) for the ecosystems that support it.
- Import: a versioned manifest export flavor, a preview that classifies every row into will install
  / already present / cannot install with a reason each, and strictly sequential execution with a
  summary that names failures. Version pinning is deliberately NOT in v1 (brew cannot honor a pin),
  and manual/npx rows are excluded from the manifest because they have no install path.

## House style (enforced)
- **No em dashes** anywhere. Use commas, colons, or parentheses.
- Match the existing voice: plain, specific, honest about limits. Do not add marketing language.
- Keep the existing structure; this is an update, not a rewrite.

## Scope
**In scope**: `README.md` only.
**Out of scope**: every other file; the screenshots themselves (only their `alt` text); CONTRIBUTING;
docs/ROADMAP.md; adding new sections beyond the ones named above.

## Git workflow
- Branch: `advisor/028-readme` from `main`.
- Commit: `docs: README covers cargo, uninstall, and import`.
- Do NOT push or open a PR unless the operator asks (the operator WILL instruct you to push).

## Steps

### Step 1: Six sources everywhere
Update each stale location listed above so cargo is included. For the badge at :27, add cargo to the
URL-encoded list (`%20%7C%20` is the separator). For the ASCII diagram at :170, add cargo to the
source boxes, keeping the box borders aligned (count the characters; a misaligned diagram is worse
than none). Change "Five sources" to "Six sources".
**Verify**: `grep -n "Five sources" README.md` → no matches. `grep -ci "cargo" README.md` → several.

### Step 2: Uninstall in the Transfers section
Add a bullet describing uninstall per the facts above, and extend the rollback bullet to note cargo
supports real rollback while brew does not.
**Verify**: `grep -n "uninstall" README.md` → present in the Transfers section.

### Step 3: Import alongside Export
Replace the lone Export bullet with Export plus Import, describing the preview buckets and the
honest exclusions per the facts above.
**Verify**: `grep -n "Import" README.md` → present.

### Step 4: Search honesty for cargo
Extend the search section so cargo's exact-name limitation is stated the same way pip's already is.
**Verify**: read the section; both limitations described in the same voice.

### Step 5: Style pass
**Verify**: `grep -c '—' README.md` → **0**. Read the diff start to finish and confirm no
marketing language crept in and the diagram still aligns.

## Done criteria
- [ ] `grep -c '—' README.md` → 0
- [ ] `grep -n "Five sources" README.md` → no matches
- [ ] cargo, uninstall, and import each appear in their proper sections
- [ ] The ASCII architecture diagram is still visually aligned (paste it into your report)
- [ ] The "What napm sends" section still names crates.io and was not otherwise altered
- [ ] Only README.md changed (`git diff --stat main..HEAD`)

## STOP conditions
- A claim in this plan does not match the code on `main` (report rather than documenting something
  false; this README's whole point is that it is accurate).
- The diagram cannot be kept aligned while adding a sixth source (report your best attempt and say
  so, rather than shipping a broken diagram).

## Maintenance notes
- Every future ecosystem touches all of the same locations; consider whether the source list is
  worth centralizing in one place the rest reference.
- Reviewer: check the badge URL renders (the encoded pipes) and the diagram alignment.
