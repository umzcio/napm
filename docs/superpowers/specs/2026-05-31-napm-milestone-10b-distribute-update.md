# napm M10b - Distributable & self-updating

**Date:** 2026-05-31
**Status:** Draft for review (written while owner was away; some defaults assumed, flagged below)
**Milestone:** M10b (see `docs/ROADMAP.md`, "M10 - Packaging")

## Goal

Turn the working local `.app` from M10a into a notarized `.dmg` that opens
cleanly on any Mac, that updates itself in place, published from a now-public
GitHub repo. Reuses the Apple signing setup already proven in zMeet/zMD/fiddle.

## Decisions already made (from the M10 chat)

- Notarized `.dmg` + public GitHub release (audience: shareable + open source).
- Updater UX: check on launch and notify (unobtrusive prompt), plus a manual
  "Check for updates" in the Help menu. Never silent auto-install.
- Build pipeline: local scripts on the Mac. GitHub Actions CI deferred.
- Signing identity: `Developer ID Application: The University of Montana
  (5JJ6G6A84S)` (already in the keychain). Notarization via an App Store Connect
  API key (`.p8` + key-id + issuer), the same method zMeet uses.

## Assumptions made while owner was away (CONFIRM on review)

1. **Reuse the existing Apple API key.** napm will reuse the same App Store
   Connect `.p8` / key-id / issuer that zMeet/zMD use (same Apple team), copied
   into napm's own gitignored `scripts/.notary-config.local`. Alternative: mint a
   napm-specific key. Default: reuse.
2. **First public version stays `0.1.0`.** It signals "early" honestly and gives
   the updater a real baseline to update from as features land (M11+). Alternative:
   bump to `1.0.0`. Default: ship `0.1.0`, defer `1.0.0` to feature-complete.
3. **DMG: default Tauri layout first.** A functional drag-to-Applications `.dmg`
   (Tauri's built-in dmg target). A custom npstr-branded background is optional
   polish, deferred. Default: functional first.
4. **GATED steps wait for the owner at the machine:** (a) running a real notarized
   build with the Apple key, (b) generating + storing the updater signing key,
   (c) flipping the repo public. Everything else (config, scripts, code, the
   updater UI) is prepared up front so each gate is a single confirmation.

## Part 1 - Sign + notarize

Tauri does this in one pass during `tauri build` when the env is set; no separate
`notarytool`/`stapler` step (this is simpler than zMeet's manual Swift flow).

- **Signing:** `APPLE_SIGNING_IDENTITY="Developer ID Application: The University
  of Montana (5JJ6G6A84S)"`. The cert is already in the keychain. Tauri signs the
  `.app` with the hardened runtime automatically. The identity is supplied via env
  (kept out of the committed `tauri.conf.json`).
- **Notarization (App Store Connect API key):** set `APPLE_API_ISSUER`,
  `APPLE_API_KEY` (the key id), and `APPLE_API_KEY_PATH` (the `.p8` path). With
  these set, `tauri build` submits to Apple notary, waits, and staples
  automatically.
- **Entitlements / hardened runtime:** start with NO custom entitlements (default
  hardened runtime). napm spawns SEPARATE child processes (`npm`, `brew`, `pip`),
  which run as independent processes, not loaded into napm, so no entitlement is
  required. Add a `bundle.macOS.entitlements` plist only if a notarization or
  runtime failure proves one is needed (honest "minimal, add if forced").
- The signed/notarized `.app` no longer needs the right-click -> Open dance; it
  opens normally on any Mac.

## Part 2 - DMG installer

Tauri's `dmg` bundle target already builds `napm_<version>_aarch64.dmg` (seen in
M10a). For M10b it inherits the signing/notarization above (the `.dmg` is stapled).
Default Tauri drag-to-Applications layout for v1; a custom background image
(`bundle.macOS.dmg`) is deferred polish.

## Part 3 - Auto-updater

### Plugin + keys

- Add `tauri-plugin-updater` (Rust) and the `@tauri-apps/plugin-process` capability
  for relaunch. The JS plugin package is not used directly (napm has no bundler);
  the update flow is driven by Rust commands the frontend invokes (see below),
  matching napm's "all logic in the agent" architecture.
- **GATED:** generate the signing keypair with
  `npm run tauri signer generate -- -w ~/.napm/napm-updater.key`. The PUBLIC key
  goes in `tauri.conf.json` (`plugins.updater.pubkey`, committed). The PRIVATE key
  (password-protected) lives at `~/.napm/napm-updater.key`, is NEVER committed, is
  backed up by the owner, and is supplied at release time via
  `TAURI_SIGNING_PRIVATE_KEY` (+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).
- `bundle.createUpdaterArtifacts: true` so each build emits the updater artifact
  (`napm.app.tar.gz`) and its `.sig`.

### Config

```json
"plugins": {
  "updater": {
    "pubkey": "<content of napm-updater.key.pub>",
    "endpoints": ["https://github.com/umzcio/napm/releases/latest/download/latest.json"]
  }
}
```

GitHub's `releases/latest/download/<asset>` always serves the asset from the
newest published release, so the endpoint needs no per-version templating.

### Update manifest (latest.json, attached to each GitHub release)

```json
{
  "version": "0.1.1",
  "notes": "What changed in this release.",
  "pub_date": "2026-06-01T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<contents of napm.app.tar.gz.sig>",
      "url": "https://github.com/umzcio/napm/releases/download/v0.1.1/napm.app.tar.gz"
    }
  }
}
```

(Only `darwin-aarch64` for now; napm is Apple-silicon macOS. An Intel
`darwin-x86_64` row can be added if/when an x86_64 build is produced.)

### Rust command wrappers (consistent with napm's architecture)

Two thin commands over the plugin's Rust API, so the frontend calls `invoke`
exactly like every other feature (no JS plugin import, no shell logic in the UI):

- `check_for_update() -> Option<UpdateMeta>` - returns `{ version, notes,
  pubDate }` when a newer signed release exists, else `null`. Network/parse
  failures return `null` (a failed check never blocks or fakes an update).
- `install_update() -> Result<(), String>` - downloads, verifies the signature
  against the baked-in pubkey, installs, and relaunches. Returns an honest error
  string on failure (shown in the UI), never a silent partial install.

### Frontend UX (Win98-styled, reusing the modal + menu engines)

- **On launch:** after the first library scan settles, invoke `check_for_update`.
  If it returns an update, show an unobtrusive beveled modal: "napm v<new> is
  available" + the notes + `Update now` / `Later`. `Later` dismisses for the
  session. Never interrupts the dial-up splash or the scan.
- **Manual:** Help -> "Check for updates..." invokes the same `check_for_update`;
  if none, a small "You are up to date (v<current>)" modal; if some, the same
  update modal. Greyed/disabled while a check is in flight.
- **Install:** `Update now` invokes `install_update`, shows a progress line in the
  modal, then the app relaunches into the new version. A failure shows the error
  and a "Try later" out, never a broken half-state.

## Part 4 - Release script

One local script, `scripts/release.sh` (modeled on zMeet's), that:

1. Sources `scripts/.notary-config.local` (gitignored) for the Apple API key path,
   key id, and issuer.
2. Exports `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_ISSUER`,
   `APPLE_API_KEY_PATH`, and `TAURI_SIGNING_PRIVATE_KEY` (+ password) for the
   updater key at `~/.napm/napm-updater.key`.
3. Runs `npm run tauri build`. Tauri then signs, notarizes, staples, and emits the
   `.dmg`, the `.app`, and the updater `napm.app.tar.gz` + `.sig`.
4. Prints the artifacts and a checklist for the manual GitHub release step
   (upload `.dmg`, `napm.app.tar.gz`, and a generated `latest.json`).

A committed `scripts/.notary-config.example` documents the fields. A small helper
(`scripts/make-latest-json.sh`) assembles `latest.json` from the build's version
and the `.sig` contents so the manifest is never hand-edited.

## Part 5 - Go public (GATED)

Flip `github.com/umzcio/napm` public only after:

- **Secret scan of git history:** confirm no Apple `.p8`, no
  `.notary-config.local`, no updater private key, no personal paths/emails beyond
  the intended `umzcio <umzcio@users.noreply.github.com>` were ever committed
  (scan all history, not just HEAD). `.gitignore` must already exclude
  `scripts/.notary-config.local`, `*.p8`, and any `*.key`.
- **Repo hygiene:** LICENSE (MIT) and README present and house-style; the npstr
  brand assets in place; no "Napster" string anywhere.
- Then `gh repo edit umzcio/napm --visibility public` (or the dashboard), and cut
  the first release (`v0.1.0`) with the `.dmg` + updater artifacts + `latest.json`.

## Secrets handling (must be airtight before going public)

- `.gitignore` adds: `scripts/.notary-config.local`, `*.p8`, `*.key`,
  `~/.napm/` is outside the repo already.
- The updater PUBLIC key in `tauri.conf.json` is safe to commit (it only verifies).
- The updater PRIVATE key and the Apple `.p8` never enter the repo.

## Testing / verification

- **Signing/notarization:** after the gated build, `codesign -dv --verbose=4
  napm.app` shows the Developer ID identity (not adhoc) and `spctl -a -vvv
  napm.app` / `xcrun stapler validate` pass. The `.dmg` opens on a Mac with no
  Gatekeeper warning.
- **Updater (the real end-to-end test):** install `v0.1.0`, publish a `v0.1.1`
  release with a valid `latest.json`, launch `v0.1.0`, confirm the update modal
  appears, accept, and confirm it installs and relaunches into `v0.1.1`. A
  tampered/missing signature must be rejected (no install).
- **No-update path:** Help -> Check for updates on the latest version shows "up to
  date," never an error or a phantom update.
- **Milestone-end review:** adversarial pass over the updater command wrappers
  (signature verification not bypassable, failed checks never fake an update,
  no secret leakage), the release script (no secret echoed/committed), and the
  go-public secret scan.

## Out of scope (deferred)

- GitHub Actions CI (tagged-push auto-release) - a later pass once local releases
  are proven.
- Custom npstr-branded DMG background art.
- Intel (`darwin-x86_64`) and any non-macOS targets.
- The `1.0.0` version bump (until feature-complete).
- In-app changelog rendering beyond the release `notes` string.
