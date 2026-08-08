#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG="$ROOT/scripts/.notary-config.local"
[ -f "$CONFIG" ] || { echo "error: $CONFIG not found (copy .notary-config.example)"; exit 1; }
# shellcheck disable=SC1090
source "$CONFIG"

# Refuse to build on a version mismatch across the three manifests that must
# agree (package.json, Cargo.toml, tauri.conf.json). Run scripts/bump-version.sh
# to bring them back into sync.
command -v jq >/dev/null || { echo "jq is required (brew install jq)"; exit 1; }
PKG_VER="$(jq -r .version "$ROOT/package.json")"
CARGO_VER="$(awk '/^version = "/ { gsub(/(version = "|")/, ""); print; exit }' "$ROOT/src-tauri/Cargo.toml")"
TAURI_VER="$(jq -r .version "$ROOT/src-tauri/tauri.conf.json")"
if [ "$PKG_VER" != "$CARGO_VER" ] || [ "$CARGO_VER" != "$TAURI_VER" ]; then
  echo "error: version mismatch: package.json=$PKG_VER Cargo.toml=$CARGO_VER tauri.conf.json=$TAURI_VER" >&2
  echo "run scripts/bump-version.sh <version> to sync all three" >&2
  exit 1
fi
echo "==> Building v$TAURI_VER"

echo "==> Building, signing, notarizing, stapling (this takes a few minutes)"
cd "$ROOT"
npm run tauri build

DMG="$(ls -t src-tauri/target/release/bundle/dmg/*.dmg | head -1)"
TARBALL="$(ls -t src-tauri/target/release/bundle/macos/*.app.tar.gz 2>/dev/null | head -1 || true)"

# Tauri notarizes and staples the .app, but not the .dmg container. Notarize and
# staple the .dmg too so the downloaded installer opens with no Gatekeeper warning.
echo "==> Notarizing and stapling the .dmg (Apple notary round-trip)"
xcrun notarytool submit "$DMG" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
xcrun stapler staple "$DMG"
echo "==> Gatekeeper assessment:"
spctl -a -vvv -t install "$DMG" 2>&1 | sed 's/^/    /'

echo "==> Artifacts:"
echo "    DMG:     $DMG"
echo "    Updater: ${TARBALL:-<none - check createUpdaterArtifacts>}"
echo "==> Next: scripts/make-latest-json.sh, then upload DMG + tarball + latest.json to the GitHub release."
