#!/bin/bash
set -euo pipefail
# Bump the app version in the three manifests that must agree, then verify
# they actually agree. This exists because package.json, Cargo.toml, and
# tauri.conf.json were hand-edited independently in the past: package.json
# drifted to 0.1.0 while the other two moved on to 0.1.4, and a bad manual
# edit once truncated Cargo.toml and tauri.conf.json entirely (see git
# history around commit 443ac45 / recovery 3efe538).
#
# Usage: scripts/bump-version.sh <version>
VERSION="${1:?usage: bump-version.sh <version>}"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must look like MAJOR.MINOR.PATCH (e.g. 0.1.5), got: $VERSION" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG_JSON="$ROOT/package.json"
CARGO_TOML="$ROOT/src-tauri/Cargo.toml"
TAURI_CONF="$ROOT/src-tauri/tauri.conf.json"

command -v jq >/dev/null || { echo "jq is required (brew install jq)"; exit 1; }

# package.json: set .version via jq (reformats to 2-space indent, which
# matches the existing style, and preserves key order).
TMP_PKG="$(mktemp)"
jq --arg v "$VERSION" '.version = $v' "$PKG_JSON" > "$TMP_PKG"
mv "$TMP_PKG" "$PKG_JSON"

# Cargo.toml: only the first `version = "..."` line, which is the one under
# [package]. Dependency version specifiers further down in the file must
# not be touched.
awk -v ver="$VERSION" '
  BEGIN { done = 0 }
  !done && /^version = "/ { print "version = \"" ver "\""; done = 1; next }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp"
mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# tauri.conf.json: the single top-level "version" line.
sed -E 's/^([[:space:]]*"version": ")[^"]*(",?)$/\1'"$VERSION"'\2/' "$TAURI_CONF" > "$TAURI_CONF.tmp"
mv "$TAURI_CONF.tmp" "$TAURI_CONF"

# Refresh the lockfile pin for the napm crate so Cargo.lock's recorded
# version does not drift from Cargo.toml.
(
  cd "$ROOT/src-tauri"
  cargo update -p napm --precise "$VERSION" 2>/dev/null || cargo check -q
)

# Final guard: all three manifests must now agree, or we refuse to call
# this a successful bump.
PKG_VER="$(jq -r .version "$PKG_JSON")"
CARGO_VER="$(awk '/^version = "/ { gsub(/(version = "|")/, ""); print; exit }' "$CARGO_TOML")"
TAURI_VER="$(jq -r .version "$TAURI_CONF")"

if [ "$PKG_VER" != "$VERSION" ] || [ "$CARGO_VER" != "$VERSION" ] || [ "$TAURI_VER" != "$VERSION" ]; then
  echo "error: version mismatch after bump: package.json=$PKG_VER Cargo.toml=$CARGO_VER tauri.conf.json=$TAURI_VER" >&2
  exit 1
fi

echo "version bumped and agreed: $VERSION (package.json, Cargo.toml, tauri.conf.json)"
