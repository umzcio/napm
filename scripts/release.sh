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
