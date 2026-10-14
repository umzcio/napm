#!/bin/bash
set -euo pipefail
# Build the styled drag-to-Applications installer .dmg from a built napm.app,
# using create-dmg (brew install create-dmg). The background art comes from
# scripts/make-dmg-bg.py; the icon coordinates below must match its layout.
# Signs the image when APPLE_SIGNING_IDENTITY is set. Notarizing and stapling
# stay in release.sh.
# The window size is the frame, title bar included: 420pt of content plus a
# 32pt title bar (the tallest current macOS uses; the art has slack for less).
# Usage: scripts/make-dmg.sh <path/to/napm.app> <out.dmg>
APP="${1:?usage: make-dmg.sh <path/to/napm.app> <out.dmg>}"
OUT="${2:?usage: make-dmg.sh <path/to/napm.app> <out.dmg>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v create-dmg >/dev/null || { echo "create-dmg is required (brew install create-dmg)"; exit 1; }
[ -d "$APP" ] || { echo "error: $APP not found"; exit 1; }

# create-dmg copies a whole folder into the image, so stage the app alone.
# ditto keeps the code signature and extended attributes intact.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/napm.app"

SIGN=()
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  SIGN=(--codesign "$APPLE_SIGNING_IDENTITY")
fi

rm -f "$OUT"
create-dmg \
  --volname "napm" \
  --volicon "$ROOT/src-tauri/icons/icon.icns" \
  --background "$ROOT/src-tauri/dmg/background.tiff" \
  --window-pos 200 120 \
  --window-size 660 452 \
  --icon-size 128 \
  --text-size 12 \
  --icon "napm.app" 165 215 \
  --hide-extension "napm.app" \
  --app-drop-link 495 215 \
  --hdiutil-quiet \
  ${SIGN[@]+"${SIGN[@]}"} \
  "$OUT" "$STAGE"
