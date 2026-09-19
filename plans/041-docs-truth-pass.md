# Plan 041: Documentation truth pass - CLAUDE.md, ROADMAP, README structure

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report - do not improvise. When done, update the status row for this plan
> in `plans/README.md` - unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3df72f5..HEAD -- CLAUDE.md docs/ROADMAP.md README.md`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: docs
- **Planned at**: commit `3df72f5`, 2026-09-16

## Why this matters

`CLAUDE.md` is the file coding agents read first in this repo, and it is the
pre-implementation brief: it recommends a Node backend agent (`:20-27`),
frames Tauri as an unmade choice (`:25`), defines the canonical data model
as three ecosystems (`:34`, shipped is six), and recommends SQLite (`:124`)
while the README records the settled opposite ("Why flat JSON instead of
SQLite", `README.md:274`). An agent following it would propose changes that
contradict the shipped architecture. Separately, `docs/ROADMAP.md`
contradicts itself: its "Deferred on purpose" list (`:353-357`) defers the
issue-velocity hold and the npx latest-drift hint, both recorded as shipped
in v0.1.3 (`:309-320`), and the Branding entry says the repo is "not yet
pushed" (`:29-30`) though it is public with published releases. Stale docs
that are actively wrong are worse than missing docs. The content of
CLAUDE.md is valuable history (the original product brief) and should be
archived, not deleted.

## Current state

**Revision note (2026-09-16, after first dispatch):** `CLAUDE.md` is NOT
tracked by git. It is ignored by `.gitignore` (`/CLAUDE.md`) and exists only
as a local file in the maintainer's working tree. That gitignore decision is
respected, not changed: the archived brief becomes a tracked file under
`docs/design/`, and the new thin CLAUDE.md is written to the maintainer's
local tree directly (the plan's single out-of-worktree step), since a
gitignored file cannot travel through a branch.

- `CLAUDE.md` (local, untracked) - the original build brief. Notable stale claims:
  `:5` "the brief for turning the prototype into a real, working
  application" (done); `:22-25` Node-agent recommendation and undecided
  packaging; `:34` `Ecosystem = "npm" | "brew" | "pip"`; `:124` "SQLite is
  recommended"; `:132-134` lists the throttle slider as intentionally doing
  nothing (it is now the real Update Appetite dial). Durable content worth
  keeping in the replacement: the naming/legal rules (`:11-16`), the "what
  not to do" honesty rules (`:147-152`), and the no-em-dash style rule
  (`:154-156`).
- `docs/ROADMAP.md:353-357` - "Deferred on purpose" includes "`hold`
  issue-velocity scoring ... - v1.5" and "**npx latest-drift hint** ... -
  v1.5"; both shipped (`:311-320`). `:354` also defers "npx usage-frequency
  intelligence (rank by how often you npx a tool)" - the M11 spike
  (`docs/design/m11-spike.md`) found the only local signal is the npx cache,
  which records that a spec ran at least once, not how often, so the feature
  as described is not observable; recency-of-last-run via cache mtimes is
  the feasible weaker version.
- `docs/ROADMAP.md:29-30` - "scrubbed git history (public repo at
  github.com/umzcio, not yet pushed)". The repo is public with releases
  through v0.1.7.
- `README.md:230-252` - the Project Structure tree lists every
  `src-tauri/src` module except `importer.rs` (358 lines, the import-manifest
  feature the README itself documents at `:82`).
- `docs/design/` already holds design records (e.g. the M11 spike), so an
  archived brief has a natural home.
- House rule: no em dashes in any documentation or UI copy.

## Commands you will need

| Purpose   | Command                                   | Expected on success |
|-----------|-------------------------------------------|---------------------|
| Em-dash check | `grep -n "—" CLAUDE.md docs/ROADMAP.md README.md` | no NEW hits introduced by you (README/ROADMAP may have none; CLAUDE.md replacement must have none) |
| Link check | `grep -n "original-brief" CLAUDE.md`      | matches            |
| Sanity    | `cd src-tauri && cargo test`              | all pass (nothing code changed) |

## Scope

**In scope** (the only files you should modify):
- `docs/design/original-brief.md` (create, tracked, from the local CLAUDE.md content)
- `docs/ROADMAP.md` (three targeted edits)
- `README.md` (one line in Project Structure)
- `/Users/zach/GitHub/napm/CLAUDE.md` (rewrite, thin) - SINGLE EXCEPTION:
  this file is gitignored and untracked, so it is edited directly in the
  maintainer's working tree, not in the worktree, and is not committed.

**Out of scope** (do NOT touch):
- Any source file.
- `.gitignore` - the maintainer's choice to keep CLAUDE.md local stands.
- `plans/README.md` rows other than this plan's own status.
- Do not rewrite the ROADMAP's milestone history; only the three targeted
  edits below.
- Do not editorialize the archived brief; copy it verbatim.

## Git workflow

- Branch: `advisor/041-docs-truth-pass`
- One commit is fine; style: `docs: ...`.

## Steps

### Step 1: Archive the brief (tracked copy)

Read `/Users/zach/GitHub/napm/CLAUDE.md` (the local, untracked original) and
create `docs/design/original-brief.md` in the worktree whose content is that
file VERBATIM, with this header prepended:

```markdown
# Original product brief (archived)

This is the pre-implementation brief napm was built from. It is kept as a
design record; the shipped architecture differs where noted (Tauri v2 + Rust
backend, six ecosystems, flat-JSON persistence). Current contributor and
agent guidance lives in CLAUDE.md, README.md, CONTRIBUTING.md, and
docs/ROADMAP.md.

---
```

**Verify**: `docs/design/original-brief.md` exists in the worktree and
`diff <(tail -n +11 docs/design/original-brief.md) /Users/zach/GitHub/napm/CLAUDE.md`
shows no differences (adjust the tail offset to the actual header length).

### Step 2: Write the new thin CLAUDE.md (local file, out of worktree)

Overwrite `/Users/zach/GitHub/napm/CLAUDE.md` directly (it is gitignored;
this cannot go through the branch). A short file (aim ~40 lines) whose job
is routing, not restating:

- One paragraph: what napm is (point at README.md) and that the prototype
  brief is archived at `docs/design/original-brief.md`.
- Settled architecture rules, stated as rules: Tauri v2 + Rust backend owns
  all shell access, version comparison, and status classification; the
  frontend is a single vanilla-JS file (`frontend/index.html`) with no build
  step and only calls `invoke()`; persistence is flat JSON in the app-data
  dir by deliberate decision; six ecosystems (npm, brew, pip, npx, cargo,
  manual).
- Carry forward verbatim from the old file: the naming/legal rules (no
  original-brand name or trade dress, napm's own artwork only), the honesty
  rules (never fake what is not technically possible; surface limits in the
  UI), and the no-em-dash rule.
- Pointers: build/test via CONTRIBUTING.md (`npm install`,
  `npm run tauri dev`, `cd src-tauri && cargo test`); direction via
  `docs/ROADMAP.md`; executed change history via `plans/README.md`.

**Verify**: `grep -c "—" /Users/zach/GitHub/napm/CLAUDE.md` → 0; the file
names referenced in it all resolve.

### Step 3: ROADMAP targeted edits

1. In "Deferred on purpose", delete the `hold` issue-velocity line and the
   npx latest-drift hint line (both shipped in v0.1.3, already recorded at
   `:309-320`).
2. Rewrite the npx usage-frequency line to: "npx usage-frequency
   intelligence - not observable: the npx cache records that a spec ran at
   least once, not how often. Recency-of-last-run from cache mtimes is the
   feasible weaker version. v1.5."
3. Branding entry (`:29-30`): drop "not yet pushed" so it reads as the
   public repo it is.

**Verify**: `grep -n "latest-drift" docs/ROADMAP.md` → only the shipped
record; `grep -n "not yet pushed" docs/ROADMAP.md` → zero hits.

### Step 4: README structure line

Add to the Project Structure tree, in alphabetical position within the
`src/` listing:

```
│   │   ├── importer.rs         # library import manifest (preview + sequential install)
```

**Verify**: `grep -n "importer.rs" README.md` matches.

### Step 5: Gate

`cd src-tauri && cargo test` → all pass (sanity). Re-read each edited file
start to finish for em dashes and for claims you did not intend to make.

## Test plan

Docs-only; the checks above (greps + reading the final files) are the
tests. No code changes means `cargo test` is a formality but run it anyway.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `docs/design/original-brief.md` exists in the worktree branch and contains the old brief verbatim plus the archive header
- [ ] `/Users/zach/GitHub/napm/CLAUDE.md` (local, untracked) exists, is thin (≤ ~60 lines), contains the naming/legal, honesty, and no-em-dash rules, and contains zero em dashes
- [ ] `grep -rn "SQLite is recommended" /Users/zach/GitHub/napm/CLAUDE.md docs/design/original-brief.md` matches only inside the archived brief
- [ ] `grep -n "not yet pushed" docs/ROADMAP.md` → zero hits
- [ ] `grep -n "importer.rs" README.md` → one hit in Project Structure
- [ ] `git status` in the worktree shows only the three tracked in-scope files modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- CLAUDE.md has gained new, still-accurate content since `3df72f5` that the
  archive-or-carry-forward decision should consider (e.g. someone already
  started updating it).
- The ROADMAP lines cited have moved or been reworded (drift; re-locate the
  same claims rather than editing by line number).
- You believe any claim in the new CLAUDE.md is itself unverifiable from the
  repo; thin and true beats comprehensive and stale.

## Maintenance notes

- The new CLAUDE.md is deliberately a router: rules that change belong in
  README/CONTRIBUTING/ROADMAP, with CLAUDE.md pointing at them, so it does
  not go stale again. Reviewers should reject PRs that grow it.
- If the project later adopts AGENTS.md as the cross-tool convention, make
  it a symlink or one-line pointer to CLAUDE.md rather than a second source
  of truth.
