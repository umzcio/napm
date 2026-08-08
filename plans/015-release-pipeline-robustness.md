# Plan 015: Make the release pipeline robust: jq-built manifest, one version bump, honest update checks, private package.json

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- scripts/ package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/src/lib.rs frontend/index.html`
> If these changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it
> as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

Three failure modes in the release path, one of which has ALREADY happened:

1. **The updater manifest is built by unquoted heredoc.** `make-latest-json.sh` interpolates `$VERSION`, `$NOTES`, and the signature into a JSON heredoc with no escaping. A double quote or backslash in the release notes yields malformed `latest.json`. Because the app's `check_for_update` swallows every failure as "no update" — and the UI then tells a manually-checking user "You are up to date" — a broken manifest silently strands every installed user on the old version with a false all-clear.
2. **The version is typed by hand into three manifests.** `package.json` is at 0.1.0 while `Cargo.toml`/`tauri.conf.json` are at 0.1.4, and git history contains the incident: commit `443ac45` ("chore: bump to v0.1.4") truncated both `Cargo.toml` and `tauri.conf.json`, requiring the recovery commit `3efe538` ("fix: restore Cargo.toml and tauri.conf.json (bad bump truncated them)").
3. **`package.json` looks publishable and ships the wrong thing.** It has no `private: true`, no `repository`, version 0.1.0, and `"bin": {"napm": "reference/scanner.js"}` — the file the README describes as "the original CLI, kept as a logic reference". An accidental `npm publish` would install a stale five-tool demo scanner as the `napm` command globally.

## Current state

- `scripts/make-latest-json.sh` (entire file, 22 lines):
  ```bash
  VERSION="${1:?usage: make-latest-json.sh <version> <tag> [notes]}"
  TAG="${2:?...}"
  NOTES="${3:-Update to v$VERSION}"
  ...
  SIG="$(cat "$SIG_FILE")"
  URL="https://github.com/umzcio/napm/releases/download/$TAG/napm.app.tar.gz"
  cat > "$ROOT/latest.json" <<EOF
  {
    "version": "$VERSION",
    "notes": "$NOTES",
    "platforms": {
      "darwin-aarch64": { "signature": "$SIG", "url": "$URL" }
    }
  }
  EOF
  ```
- `scripts/release.sh` — builds/signs/notarizes; does NOT touch versions and does not validate `latest.json` (verify: `grep -n version scripts/release.sh` → no version handling). It sources gitignored `scripts/.notary-config.local`; do not touch that mechanism.
- Versions today: `package.json:3` `"0.1.0"`; `src-tauri/Cargo.toml:3` `0.1.4`; `src-tauri/tauri.conf.json:4` `"0.1.4"`. The in-app About reads the version live from Tauri config (safe).
- `src-tauri/src/lib.rs:198-209` — the check that conflates failure with up-to-date:
  ```rust
  /// Check the release feed for a newer signed version. Returns None on no update
  /// OR any failure (a failed check never blocks or fabricates an update).
  #[tauri::command(async)]
  async fn check_for_update(app: tauri::AppHandle) -> Option<UpdateMeta> {
      use tauri_plugin_updater::UpdaterExt;
      let update = app.updater().ok()?.check().await.ok()??;
      Some(UpdateMeta { ... })
  }
  ```
- `frontend/index.html:1125-1132` — the frontend treats a rejection as up-to-date for manual checks:
  ```js
  i("check_for_update").then(function(u){
    UPDATE_CHECKING=false;
    if(u) showUpdateModal(u);
    else if(manual) showUpToDate();
  }).catch(function(){ UPDATE_CHECKING=false; if(manual) showUpToDate(); });
  ```
  and `showUpToDate` (`:1118-1124`) renders "You are up to date".
- `jq` availability: not guaranteed; the script must check and fail with a clear message (`command -v jq`).
- UI copy rule: no em dashes.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 |
| Script check | `bash -n scripts/make-latest-json.sh scripts/release.sh scripts/bump-version.sh` | exit 0 (syntax) |
| Manifest validation | `jq . latest.json` | pretty-printed JSON, exit 0 |
| Run the app | `npm run tauri dev` | Help → Check for updates behaves per checklist |

## Scope

**In scope**:
- `scripts/make-latest-json.sh`
- `scripts/release.sh` (add guards only)
- `scripts/bump-version.sh` (create)
- `package.json`
- `src-tauri/src/lib.rs` (`check_for_update` return type)
- `frontend/index.html` (`checkForUpdate` failure branch)

**Out of scope**:
- The signing/notarization flow and `.notary-config.*` handling (works; leave it).
- `latest.json` itself (a release artifact, gitignored).
- `reference/scanner.js` (keep the file and the `demo` script; only the `bin` mapping goes).

## Git workflow

- Branch: `advisor/015-release-pipeline`
- Commits: `fix(release): jq-built manifest and validation guards`, `feat(release): single version bump script`, `chore: package.json is private, not a publishable CLI`, `fix(updater): a failed check is not "up to date"`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: jq-built manifest

Rewrite the manifest assembly in `make-latest-json.sh` (keep the arg parsing and SIG_FILE resolution):

```bash
command -v jq >/dev/null || { echo "jq is required (brew install jq)"; exit 1; }
jq -n --arg version "$VERSION" --arg notes "$NOTES" --arg sig "$SIG" --arg url "$URL" \
  '{version: $version, notes: $notes, platforms: {"darwin-aarch64": {signature: $sig, url: $url}}}' \
  > "$ROOT/latest.json"
jq -e .version "$ROOT/latest.json" >/dev/null
```

**Verify**: `bash -n scripts/make-latest-json.sh` → exit 0. Then a dry run with a fake sig file: create a temp `.sig`, point the script's glob at it or run the jq pipeline manually with `--arg notes 'quote " and $dollar and \backslash'` → `jq . latest.json` parses and the notes round-trip intact.

### Step 2: `bump-version.sh`

Create `scripts/bump-version.sh <version>`:
- Validate the arg matches `^[0-9]+\.[0-9]+\.[0-9]+$`.
- Edit exactly three anchored fields: `"version": "..."` in `package.json` (top-level only — anchor to the line following `"name": "napm"` or use `jq` for this file too, which is safest: `jq --arg v "$1" '.version=$v' package.json > tmp && mv tmp package.json` — note jq reformats; confirm the resulting diff is acceptable or use a targeted `sed` with a line-anchored pattern), `version = "..."` under `[package]` in `src-tauri/Cargo.toml` (anchor: first `^version = ` line), `"version": "..."` in `src-tauri/tauri.conf.json` (anchor: the top-level line, e.g. match `^  "version": `).
- Refresh the lockfile pin: `(cd src-tauri && cargo update -p napm --precise "$1" 2>/dev/null || cargo check -q)` so `Cargo.lock` follows.
- Final guard, the whole point: extract all three versions and `exit 1` with a message if they disagree; print the agreed version on success.

In `release.sh`, add the same three-way agreement check near the top and refuse to build on mismatch.

**Verify**: on a scratch branch, run `scripts/bump-version.sh 0.1.5` → all three files show 0.1.5, script prints agreement, `git diff` shows ONLY version lines (plus Cargo.lock) changed; then `git checkout -- .` to discard. Run `scripts/bump-version.sh banana` → exits non-zero with a clear message.

### Step 3: `package.json` hygiene

Set `"private": true`, add `"repository": "github:umzcio/napm"`, remove the `"bin"` block, keep `"demo"` and `"tauri"` scripts, and set `"version"` to match the current app version (0.1.4 at planning time — use whatever `tauri.conf.json` says when you execute).

**Verify**: `node -e "const p=require('./package.json'); if(!p.private||p.bin) process.exit(1)"` → exit 0; `npm run demo` still runs the reference scanner.

### Step 4: Honest update checks

- Backend: change `check_for_update` to `Result<Option<UpdateMeta>, String>`: `Ok(None)` = genuinely up to date, `Ok(Some(meta))` = update available, `Err(msg)` = the check could not run (updater init or network/manifest failure — map with `.map_err(|e| e.to_string())`, and the inner `.check().await` failure likewise; only a successful check that returns no update is `Ok(None)`). Update the doc comment.
- Frontend `checkForUpdate` (`:1125-1132`): `.then` unchanged for the two Ok shapes (Tauri resolves `Ok` values; `Err` rejects the promise); the `.catch` for a MANUAL check shows a new modal (clone `showUpToDate`'s structure) titled "Check for updates" with body "Could not check for updates. This does not mean you are up to date. Check your connection and try again." — comma phrasing, no em dashes. Silent (launch-time) checks stay silent on failure.

**Verify**: `cd src-tauri && cargo test` → exit 0. App run online: Help → Check for updates → "You are up to date" (or the update modal). App run with Wi-Fi off: Help → Check for updates → the new failure modal, NOT "up to date".

## Test plan

Scripted verifications inline in the steps (bash -n, jq round-trip, bump dry-run, node package.json assertion) plus the two-state manual updater check. No new Rust unit tests (the changed command is glue over the updater plugin; its failure paths are exercised by the offline manual check).

## Done criteria

- [ ] `bash -n` passes on all three scripts; manifest builds via `jq -n` and survives quotes/backslashes in notes
- [ ] `scripts/bump-version.sh` exists; dry-run syncs all three manifests + Cargo.lock and self-verifies; `release.sh` refuses mismatched versions
- [ ] `package.json`: `private: true`, no `bin`, version matches the app, `npm run demo` still works
- [ ] `check_for_update` returns `Result<Option<UpdateMeta>, String>`; offline manual check shows the failure modal, not "up to date"
- [ ] `cd src-tauri && cargo test` exits 0
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match the live code.
- Tauri's serialization of `Result<Option<T>, String>` does not produce resolve/reject as described on this Tauri version (test with a temporary forced `Err` in dev; if the frontend sees something else, report the observed shape).
- `cargo update -p napm --precise` misbehaves against this lockfile — fall back to `cargo check` for the lock refresh and note it.

## Maintenance notes

- The release checklist in README ("Releasing (maintainers)") should mention `bump-version.sh` — one sentence; update it in the same PR.
- If a second platform target ever ships (Linux), `make-latest-json.sh` grows a second `--arg` platform entry; the jq structure makes that additive.
- Reviewer: diff of `bump-version.sh`'s sed/jq anchors against all three files — the 443ac45 incident was an anchoring bug; the three-way agreement check is the backstop.
