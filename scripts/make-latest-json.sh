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

command -v jq >/dev/null || { echo "jq is required (brew install jq)"; exit 1; }
jq -n --arg version "$VERSION" --arg notes "$NOTES" --arg sig "$SIG" --arg url "$URL" \
  '{version: $version, notes: $notes, platforms: {"darwin-aarch64": {signature: $sig, url: $url}}}' \
  > "$ROOT/latest.json"
jq -e .version "$ROOT/latest.json" >/dev/null

echo "wrote $ROOT/latest.json (do not commit; upload it to the release)"
