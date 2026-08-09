# Plan 033: The version bump must cover package-lock.json

> **Executor instructions**: Follow the plan, run every verification, touch only the in-scope files.
> STOP and report if a STOP condition occurs. Skip updating `plans/README.md`.

## Status
- **Priority**: P2 (blocks a clean 0.1.6 release) | **Effort**: S | **Risk**: LOW
- **Planned at**: `main` @ 8423350, 2026-08-08

## Why this matters

`scripts/bump-version.sh` syncs the three manifests that must agree, and `scripts/release.sh`
refuses to build unless they do. Neither touches `package-lock.json`, so it still records `0.1.4`
while everything else says `0.1.5`. Any `npm install` or `npm ci` rewrites it and dirties the tree,
and the released lockfile misstates the version of the thing it locks.

The script already solves exactly this problem for the Rust side: it runs `cargo update -p napm
--precise "$VERSION"` with the comment "so Cargo.lock's recorded version does not drift from
Cargo.toml". The npm lockfile needs the same treatment. This is a gap in the fix, not a new idea.

The header comment on `bump-version.sh` explains that this script exists *because* the manifests
drifted apart when hand-edited. The lockfile is the one file still drifting.

## Current state

`scripts/bump-version.sh`:
- **:26-30** sets `.version` in `package.json` via `jq`
- **:44-50** refreshes the Cargo lockfile pin
- **:52-62** the final guard: re-reads all three manifests and fails the bump if any disagrees

`scripts/release.sh`:
- **:12-20** the same three-way cross-check, which refuses to build on a mismatch

`package-lock.json` carries the version in **two** places, both of which must move:
```json
{
  "name": "napm",
  "version": "0.1.4",          <- top level
  "packages": {
    "": { "name": "napm", "version": "0.1.4", ... }   <- the root package entry
  }
}
```

## Steps

### Step 1: Bump the lockfile in `bump-version.sh`
Add a step alongside the existing Cargo lockfile refresh that sets both version fields in
`package-lock.json`.

Prefer a `jq` edit of the two fields over `npm install --package-lock-only`: `jq` is already a hard
dependency of this script, it is deterministic, it works offline, and it cannot touch the dependency
tree. `npm install` can rewrite unrelated parts of the lockfile and needs the network.

Guard for the file being absent (do not fail the bump if someone has no lockfile checked out), and
preserve the file's existing formatting conventions: check whether it is 2-space indented and ensure
your write matches, including the trailing newline. A whole-file reformat would bury the real change.

### Step 2: Extend both guards
Add `package-lock.json` to the final verification block in `bump-version.sh` (**:52-62**) and to the
cross-check in `release.sh` (**:12-20**), so a future drift fails loudly instead of shipping. Update
the error messages and the surrounding comments to name four files rather than three, including the
`echo "==> Building v$TAURI_VER"` path's assumptions if they change.

**Verify**: with the tree at `0.1.5` and the lockfile at `0.1.4`, `scripts/release.sh` must now fail
its version check. Test this without building: the check is at the top of the script and exits
before `npm run tauri build`. Confirm you see the mismatch error, then confirm it passes once the
lockfile is correct.

### Step 3: Prove the bump works
Run `scripts/bump-version.sh 0.1.5` (the version the tree is already at, so this is a no-op for the
three manifests and a repair for the lockfile). Confirm `git diff` shows only the two version fields
in `package-lock.json` changing, and nothing else.

Do **not** bump to 0.1.6 and do **not** commit a version change; the operator runs the real bump.
Leave the tree at `0.1.5` with a corrected lockfile.

## Commands
| Purpose | Command | Expected |
|---|---|---|
| Lockfile parses | `python3 -c "import json;json.load(open('package-lock.json'))"` | no error |
| Both fields agree | `jq -r '.version, .packages[""].version' package-lock.json` | `0.1.5` twice |
| Scripts are valid bash | `bash -n scripts/bump-version.sh && bash -n scripts/release.sh` | no output |
| Shellcheck, if present | `shellcheck scripts/bump-version.sh scripts/release.sh` | no new warnings |

## Scope
**In scope**: `scripts/bump-version.sh`, `scripts/release.sh`, `package-lock.json` (its two version
fields only).
**Out of scope**: the dependency tree in the lockfile; `package.json`; `Cargo.toml`;
`tauri.conf.json`; `Cargo.lock`; the notarization and upload steps of `release.sh`; CI.

## Git workflow
- Branch: `advisor/033-bump-lockfile` from `main`.
- Commit: `fix(release): bump script covers package-lock.json`.
- Push the branch. Do NOT open a PR and do NOT merge.

## Done criteria
- [ ] `bump-version.sh` updates both version fields in `package-lock.json`
- [ ] Both guards check four files; a deliberate mismatch fails `release.sh` before it builds
- [ ] `package-lock.json` reads `0.1.5` in both places, with no other change to the file
      (`git diff --stat package-lock.json` shows 2 insertions, 2 deletions)
- [ ] `bash -n` clean on both scripts
- [ ] Only the three in-scope files changed (`git diff --stat main..HEAD`)

## STOP conditions
- The lockfile turns out to encode the version somewhere beyond those two fields. Report what you
  found rather than guessing at a third edit.
- Correcting the lockfile changes anything in the dependency tree. It must not: report and stop.

## Maintenance notes
- Any future manifest that records the app version must be added to both guards at the same time as
  it is added to the bump. The guards are the only thing keeping these files honest.
- Reviewer: confirm the lockfile diff is exactly two lines and that no dependency entry moved.
