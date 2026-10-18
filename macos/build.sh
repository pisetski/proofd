#!/usr/bin/env bash
set -euo pipefail

# Builds the self-contained proofd.app: the Objective-C NSServices host plus the
# Rust proofd binary it runs, both in Contents/MacOS.
# Output: proofd.app (in current dir or $1)

APP_NAME="proofd.app"
OUT_DIR="${1:-.}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

(cd "$REPO_ROOT" && cargo build --release --bin proofd)

TMPDIR="$(mktemp -d /tmp/proofd-build.XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

APP="$TMPDIR/$APP_NAME"
BIN_DIR="$APP/Contents/MacOS"
mkdir -p "$BIN_DIR"

clang "$SCRIPT_DIR/ServiceHost.m" \
    -o "$BIN_DIR/proofd-service-host" \
    -framework AppKit \
    -framework Foundation \
    -fobjc-arc \
    -mmacosx-version-min=14.0
cp "$REPO_ROOT/target/release/proofd" "$BIN_DIR/proofd"
cp "$SCRIPT_DIR/Info.plist" "$APP/Contents/Info.plist"

codesign --sign - --force "$BIN_DIR/proofd"
codesign --sign - --force "$APP"

mkdir -p "$OUT_DIR"
DEST="$OUT_DIR/$APP_NAME"
rm -rf "$DEST"
mv "$APP" "$DEST"

echo "Built $DEST"
