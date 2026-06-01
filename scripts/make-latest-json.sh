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
