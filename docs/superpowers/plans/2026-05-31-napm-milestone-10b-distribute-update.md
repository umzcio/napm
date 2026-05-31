# napm M10b - Distributable & self-updating Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship napm as a signed + notarized `.dmg` that opens cleanly on any Mac and updates itself in place, published from a public GitHub repo.

**Architecture:** Tauri signs/notarizes/staples during `tauri build` when the Apple env vars are set. The updater is the Tauri updater plugin wrapped in two Rust commands (`check_for_update`, `install_update`) that the vanilla-JS frontend invokes, with a Win98-styled update modal and a Help-menu manual check. One local `scripts/release.sh` produces all artifacts; GitHub Releases hosts the `.dmg`, the updater tarball, and `latest.json`.

**Tech Stack:** Tauri v2 (updater plugin), Rust, vanilla JS, bash, Apple `notarytool` (via Tauri), GitHub Releases.

**Spec:** `docs/superpowers/specs/2026-05-31-napm-milestone-10b-distribute-update.md`

---

## CRITICAL conventions

- Run `source "$HOME/.cargo/env"` before any cargo/tauri command.
- After EVERY edit to `frontend/index.html`: `cp frontend/index.html prototype/napm-prototype.html`.
- No em dashes in copy. Commit author: `git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit ...`. Do NOT push until the go-public gate.
- `#[tauri::command(async)]` for the updater commands (they do network IO).
- SECRETS: the Apple `.p8`, `scripts/.notary-config.local`, and the updater private key NEVER get committed (already gitignored: `scripts/.notary-config.local`, `*.p8`, `*.key`).

## Confirmed decisions (owner approved)

- Reuse zMeet's App Store Connect `.p8` key. First public version: `0.1.0`. Default Tauri DMG. The three gated steps wait for the owner.

## Prerequisite

M10a must be verified working (font, responsive startup, library) before executing this milestone.

## File structure

- Modify: `src-tauri/Cargo.toml` - add `tauri-plugin-updater`.
- Modify: `src-tauri/src/lib.rs` - init the updater plugin; add `check_for_update` + `install_update` commands; register them.
- Modify: `src-tauri/tauri.conf.json` - `bundle.createUpdaterArtifacts`, `plugins.updater` (pubkey + endpoints) [pubkey filled at the gated key-gen step].
- Modify: `src-tauri/capabilities/default.json` - allow the updater + process permissions if required.
- Modify: `frontend/index.html` (+ mirror) - update modal, on-launch check, Help-menu item.
- Create: `scripts/.notary-config.example`, `scripts/release.sh`, `scripts/make-latest-json.sh`.
- Modify: `README.md` - install + update + build/release notes (house style, no em dashes).

---

# PHASE A - Non-gated (implement + test anytime)

## Task A1: Add the updater plugin dependency and initialize it

**Files:** `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the crate**

Run: `cd src-tauri && cargo add tauri-plugin-updater --target 'cfg(target_os = "macos")'`
Expected: `tauri-plugin-updater` (2.x) added under a macOS target table in `Cargo.toml`.

- [ ] **Step 2: Initialize the plugin in `.setup()`**

In `src-tauri/src/lib.rs`, inside the `.setup()` closure (after the debug log plugin block), register the updater plugin:

```rust
      app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles clean. (The plugin builds without a configured pubkey; `check()` simply errors at runtime until the gated config step. That is fine for now.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10b): add and initialize the Tauri updater plugin"
```

## Task A2: Rust command wrappers `check_for_update` and `install_update`

**Files:** `src-tauri/src/lib.rs`

These wrap the plugin's Rust API so the frontend invokes them like every other command. NOTE TO IMPLEMENTER: verify the exact `tauri-plugin-updater` 2.x API (the `UpdaterExt` trait, `Update` fields `version`/`body`/`date`, and `download_and_install(on_chunk, on_finish)` signature) against the installed crate version before finalizing; adjust field/closure shapes if the crate differs. The shapes below are the expected 2.x API.

- [ ] **Step 1: Add the return type and the two commands**

In `src-tauri/src/lib.rs`, add near the other commands:

```rust
/// Minimal update metadata sent to the frontend. Empty strings where unknown.
#[derive(serde::Serialize)]
struct UpdateMeta {
    version: String,
    notes: String,
    #[serde(rename = "pubDate")]
    pub_date: String,
}

/// Check the release feed for a newer signed version. Returns None on no update
/// OR any failure (a failed check never blocks or fabricates an update).
#[tauri::command(async)]
async fn check_for_update(app: tauri::AppHandle) -> Option<UpdateMeta> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().ok()?.check().await.ok()??;
    Some(UpdateMeta {
        version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        pub_date: update.date.map(|d| d.to_string()).unwrap_or_default(),
    })
}

/// Download, verify (against the baked-in pubkey), install, and relaunch.
/// Returns an honest error string on any failure; never a silent partial install.
#[tauri::command(async)]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
```

(`app.restart()` diverges, satisfying the `Result` return.)

- [ ] **Step 2: Register both commands**

Add `check_for_update, install_update` to the `tauri::generate_handler![...]` list in `run()`.

- [ ] **Step 3: Build**

Run: `cd src-tauri && cargo build`
Expected: compiles clean. If the crate API differs, adjust per the implementer note and rebuild.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10b): check_for_update and install_update commands"
```

## Task A3: Updater capability permissions

**Files:** `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add the updater permissions**

In `src-tauri/capabilities/default.json`, add to the `permissions` array:

```json
    "updater:default"
```

(The Rust commands drive the flow, but the plugin requires its capability to be enabled. `app.restart()` is core and needs no extra permission.)

- [ ] **Step 2: Build to verify the capability parses**

Run: `cd src-tauri && cargo build`
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/capabilities/default.json
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10b): enable updater capability"
```

## Task A4: Frontend update modal, on-launch check, and Help-menu item

**Files:** `frontend/index.html` (+ mirror)

- [ ] **Step 1: Add an `showUpdate` modal and the check helpers**

In `frontend/index.html`, add a helper block near the other modal helpers (`showAbout`/`showPrefs`). It reuses the existing `#modalBack`/`#modalBox` modal and `inv()`:

```js
  // ---- UPDATER -----------------------------------------------------------
  var UPDATE_CHECKING=false;
  function showUpdateModal(u){
    var box=document.getElementById("modalBox");
    box.innerHTML='<div class="m-h">Update available<span class="x" data-close>×</span></div>'+
      '<div class="m-b"><div><b>napm v'+esc(u.version)+'</b> is available.</div>'+
      (u.notes?'<div class="muted" style="margin-top:6px;white-space:pre-wrap">'+esc(u.notes)+'</div>':'')+
      '<div id="updMsg" style="margin-top:8px"></div>'+
      '<div style="margin-top:12px;text-align:right"><button class="btn" data-close>Later</button> '+
      '<button class="btn primary" data-update-now>Update now</button></div></div>';
    document.getElementById("modalBack").style.display="flex";
  }
  function showUpToDate(){
    var box=document.getElementById("modalBox");
    box.innerHTML='<div class="m-h">Check for updates<span class="x" data-close>×</span></div>'+
      '<div class="m-b"><div>You are up to date (v'+esc(window.__appVer||"")+').</div>'+
      '<div style="margin-top:12px;text-align:right"><button class="btn" data-close>OK</button></div></div>';
    document.getElementById("modalBack").style.display="flex";
  }
  function checkForUpdate(manual){
    var i=inv(); if(!i || UPDATE_CHECKING) return;
    UPDATE_CHECKING=true;
    i("check_for_update").then(function(u){
      UPDATE_CHECKING=false;
      if(u) showUpdateModal(u);
      else if(manual) showUpToDate();
    }).catch(function(){ UPDATE_CHECKING=false; if(manual) showUpToDate(); });
  }
```

- [ ] **Step 2: Wire `Update now` in the modal click handler**

In the `#modalBack` click handler (where `data-prefs-save` etc. are handled), add a branch:

```js
    if(e.target.hasAttribute("data-update-now")){
      var msg=document.getElementById("updMsg"); if(msg) msg.textContent="Downloading update...";
      var i=inv();
      if(i) i("install_update").catch(function(err){ if(msg) msg.textContent="Update failed: "+err+". Try again later."; });
      return;
    }
```

- [ ] **Step 3: Add the Help-menu item**

In the `help` menu array (where "About napm" / "Repo on GitHub" live in `MENUS`), add:

```js
      {label:"Check for updates...", run:function(){ checkForUpdate(true); }},
```

- [ ] **Step 4: Check on launch after the first scan settles**

In `scanLibrary`, the `dismissSplash()` is called when the scan resolves. Right after the successful `.then` dismiss, kick a silent check (only once per session). Add a module flag `var DID_UPDATE_CHECK=false;` near the other state vars, and in the `.then` after `dismissSplash();`:

```js
      if(!DID_UPDATE_CHECK){ DID_UPDATE_CHECK=true; setTimeout(function(){ checkForUpdate(false); }, 1200); }
```

(The 1200ms delay lets the UI settle so the update prompt never fights the splash fade.)

- [ ] **Step 5: Mirror and commit**

```bash
cp frontend/index.html prototype/napm-prototype.html
git add frontend/index.html prototype/napm-prototype.html
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10b): in-app update modal, launch check, and Help-menu check"
```

## Task A5: Release scripts and the notary-config example

**Files:** `scripts/.notary-config.example`, `scripts/release.sh`, `scripts/make-latest-json.sh`

- [ ] **Step 1: Write `scripts/.notary-config.example`**

```bash
# Copy to scripts/.notary-config.local (gitignored) and fill in.
# Reuse the same App Store Connect API key as zMeet/zMD.

# Developer ID Application identity (already in the keychain)
export APPLE_SIGNING_IDENTITY="Developer ID Application: The University of Montana (5JJ6G6A84S)"

# App Store Connect API key for notarization
export APPLE_API_KEY_PATH="$HOME/Downloads/AuthKey_XXXXXXXXXX.p8"
export APPLE_API_KEY="XXXXXXXXXX"          # Key ID
export APPLE_API_ISSUER="00000000-0000-0000-0000-000000000000"  # Issuer UUID

# Tauri updater signing key (generated once; see the release runbook)
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.napm/napm-updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""   # if the key was password-protected
```

- [ ] **Step 2: Write `scripts/release.sh`**

```bash
#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG="$ROOT/scripts/.notary-config.local"
[ -f "$CONFIG" ] || { echo "error: $CONFIG not found (copy .notary-config.example)"; exit 1; }
# shellcheck disable=SC1090
source "$CONFIG"

echo "==> Building, signing, notarizing, stapling (this takes a few minutes)"
cd "$ROOT"
npm run tauri build

DMG="$(ls -t src-tauri/target/release/bundle/dmg/*.dmg | head -1)"
TARBALL="$(ls -t src-tauri/target/release/bundle/macos/*.app.tar.gz 2>/dev/null | head -1 || true)"
echo "==> Artifacts:"
echo "    DMG:     $DMG"
echo "    Updater: ${TARBALL:-<none - check createUpdaterArtifacts>}"
echo "==> Next: scripts/make-latest-json.sh, then upload DMG + tarball + latest.json to the GitHub release."
```

- [ ] **Step 3: Write `scripts/make-latest-json.sh`**

```bash
#!/bin/bash
set -euo pipefail
# Assemble latest.json from the built updater artifact for darwin-aarch64.
# Usage: scripts/make-latest-json.sh <version> <release-tag> ["notes"]
VERSION="${1:?usage: make-latest-json.sh <version> <tag> [notes]}"
TAG="${2:?usage: make-latest-json.sh <version> <tag> [notes]}"
NOTES="${3:-Update to v$VERSION}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIG_FILE="$(ls -t "$ROOT"/src-tauri/target/release/bundle/macos/*.app.tar.gz.sig | head -1)"
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
echo "wrote $ROOT/latest.json (do not commit; upload it to the release)"
```

(Note: `latest.json` is a release artifact, not committed - add it to `.gitignore` if it lands in the tree.)

- [ ] **Step 4: Make the scripts executable and commit (NOT `.notary-config.local`)**

```bash
chmod +x scripts/release.sh scripts/make-latest-json.sh
echo "/latest.json" >> .gitignore
git add scripts/.notary-config.example scripts/release.sh scripts/make-latest-json.sh .gitignore
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10b): release + latest.json scripts and notary-config example"
```

## Task A6: README install/update/build docs

**Files:** `README.md`

- [ ] **Step 1: Add sections** for Install (download the `.dmg`, drag to Applications), Updating (automatic + Help menu), and Building/Releasing (the `scripts/release.sh` flow, the gitignored notary config). House style, no em dashes.

- [ ] **Step 2: Commit**

```bash
git add README.md
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "docs(m10b): install, update, and release instructions"
```

---

# PHASE B - GATED (owner at the machine, in order)

> Each task here needs the owner. Do not attempt unattended.

## Task B1 (GATED): Generate the updater signing keypair

- [ ] **Step 1:** `mkdir -p ~/.napm && npm run tauri signer generate -- -w ~/.napm/napm-updater.key`
- [ ] **Step 2:** Back up `~/.napm/napm-updater.key` (private) somewhere safe. It is NEVER committed.
- [ ] **Step 3:** Copy the printed PUBLIC key (or the contents of `~/.napm/napm-updater.key.pub`) for the next task.

## Task B2 (GATED): Wire updater config in tauri.conf.json

- [ ] **Step 1:** Add to `src-tauri/tauri.conf.json`:

```json
  "bundle": { "...": "...", "createUpdaterArtifacts": true },
  "plugins": {
    "updater": {
      "pubkey": "<PUBLIC KEY FROM B1>",
      "endpoints": ["https://github.com/umzcio/napm/releases/latest/download/latest.json"]
    }
  }
```

- [ ] **Step 2:** `cd src-tauri && cargo build` (config validates). Commit (pubkey is safe to commit):

```bash
git add src-tauri/tauri.conf.json
git -c user.name=umzcio -c user.email=umzcio@users.noreply.github.com commit -m "feat(m10b): updater pubkey, endpoint, and updater artifacts"
```

## Task B3 (GATED): Create the local notary config

- [ ] **Step 1:** `cp scripts/.notary-config.example scripts/.notary-config.local`
- [ ] **Step 2:** Fill `scripts/.notary-config.local` with the real values, reusing zMeet's key: copy `NOTARY_KEY`/`NOTARY_KEY_ID`/`NOTARY_ISSUER` from `~/Documents/GitHub/zMeet/scripts/.notary-config.local` into `APPLE_API_KEY_PATH`/`APPLE_API_KEY`/`APPLE_API_ISSUER`, and point `TAURI_SIGNING_PRIVATE_KEY` at `~/.napm/napm-updater.key`.
- [ ] **Step 3:** Verify it is gitignored: `git check-ignore scripts/.notary-config.local` must print the path. (Do NOT commit it.)

## Task B4 (GATED): Signed + notarized release build

- [ ] **Step 1:** `source "$HOME/.cargo/env" && scripts/release.sh`. Takes several minutes (build + Apple notary round-trip).
- [ ] **Step 2:** Verify the signature and notarization on the `.app`:
  - `codesign -dv --verbose=4 src-tauri/target/release/bundle/macos/napm.app` shows `Authority=Developer ID Application: The University of Montana (5JJ6G6A84S)` (not adhoc).
  - `spctl -a -vvv -t install src-tauri/target/release/bundle/dmg/*.dmg` shows `accepted` / `source=Notarized Developer ID`.
  - `xcrun stapler validate src-tauri/target/release/bundle/dmg/*.dmg` passes.
- [ ] **Step 3:** Confirm the updater artifacts exist: `napm.app.tar.gz` and `napm.app.tar.gz.sig` under `src-tauri/target/release/bundle/macos/`.

## Task B5 (GATED): Cut the v0.1.0 GitHub release

- [ ] **Step 1:** `scripts/make-latest-json.sh 0.1.0 v0.1.0 "First public release."`
- [ ] **Step 2:** Create the release and upload all three assets (the repo can still be private for this; assets are what matter):

```bash
gh release create v0.1.0 \
  src-tauri/target/release/bundle/dmg/napm_0.1.0_aarch64.dmg \
  src-tauri/target/release/bundle/macos/napm.app.tar.gz \
  latest.json \
  --title "napm v0.1.0" --notes "First public release."
```

(Rename the tarball asset to exactly `napm.app.tar.gz` if its built name differs, so the `latest.json` URL matches.)

## Task B6 (GATED): Updater end-to-end test

- [ ] **Step 1:** Install the `v0.1.0` `.dmg` to `/Applications`.
- [ ] **Step 2:** Bump the version to `0.1.1` (`tauri.conf.json` + `Cargo.toml`), `scripts/release.sh`, `scripts/make-latest-json.sh 0.1.1 v0.1.1 "Updater test."`, and `gh release create v0.1.1 ...` with the new assets.
- [ ] **Step 3:** Launch the installed `v0.1.0`; confirm the update modal appears, accept it, and confirm it installs and relaunches as `v0.1.1`. Confirm Help -> Check for updates on `v0.1.1` shows "up to date."
- [ ] **Step 4:** Negative test: a tampered/missing `.sig` must cause `install_update` to fail (no install), surfaced as an honest error.

## Task B7 (GATED): Go public

- [ ] **Step 1: Secret scan of ALL history (not just HEAD):**
  - `git log -p | grep -nEi "AuthKey_|BEGIN .*PRIVATE KEY|\.p8|notary-config\.local|napm-updater\.key" | head` must find nothing.
  - `git ls-files | grep -E "\.p8$|\.key$|notary-config\.local"` must be empty.
  - Confirm the author on all commits is `umzcio <umzcio@users.noreply.github.com>`: `git log --format='%an <%ae>' | sort -u`.
- [ ] **Step 2: Repo hygiene:** LICENSE (MIT) present, README house-style, no "Napster" string: `grep -rni "napster" . --exclude-dir=.git --exclude-dir=target` is empty.
- [ ] **Step 3:** Push and flip public:
  - `git push -u origin main`
  - `gh repo edit umzcio/napm --visibility public --accept-visibility-change-consequences`
- [ ] **Step 4:** Confirm the updater endpoint resolves now that the repo is public (the `releases/latest/download/latest.json` URL returns the manifest).

## Task B8: Mark M10b done

- [ ] In `docs/ROADMAP.md`, mark "### M10b - Distributable & self-updating" as Done with a summary. Commit.

---

## Milestone-end review (after Phase B)

Adversarial pass focused on:

- **Updater security:** is signature verification against the baked-in pubkey actually enforced (a bad/missing `.sig` is rejected)? Does a failed/again network check ever fake an update or block the UI? Does `install_update` ever leave a half-installed state?
- **Secret leakage:** does any committed file or any point in git history contain the `.p8`, the updater private key, or the notary config? Does `release.sh` echo secrets to logs?
- **Honesty:** does "Check for updates" ever show a phantom update or a misleading "up to date"? Is the on-launch check unobtrusive (never fights the splash)?
- **Signing/notarization:** are `codesign`, `spctl`, and `stapler` all green on the shipped `.dmg`?

Fix findings, then mark M10b done.
