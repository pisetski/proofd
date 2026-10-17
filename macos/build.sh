#!/usr/bin/env bash
set -euo pipefail

# Builds proofd.app (Objective-C NSServices host) from macos/ServiceHost.m.
# Output: proofd.app (in current dir or $1)

APP_NAME="proofd.app"
OUT_DIR="${1:-.}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OBJC_SRC="$SCRIPT_DIR/ServiceHost.m"
INFO_PLIST="$SCRIPT_DIR/Info.plist"

if [[ ! -f "$OBJC_SRC" ]]; then
    echo "error: $OBJC_SRC not found" >&2
    exit 1
fi
if [[ ! -f "$INFO_PLIST" ]]; then
    echo "error: $INFO_PLIST not found" >&2
    exit 1
fi

TMPDIR="$(mktemp -d /tmp/proofd-build.XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

BIN_DIR="$TMPDIR/$APP_NAME/Contents/MacOS"
RES_DIR="$TMPDIR/$APP_NAME/Contents/Resources"
mkdir -p "$BIN_DIR" "$RES_DIR"

# Compile the Objective-C service host
clang "$OBJC_SRC" \
    -o "$BIN_DIR/proofd-service-host" \
    -framework AppKit \
    -framework Foundation \
    -fobjc-arc \
    -target arm64-apple-macosx14.0

chmod +x "$BIN_DIR/proofd-service-host"

# Copy Info.plist
cp "$INFO_PLIST" "$TMPDIR/$APP_NAME/Contents/Info.plist"

# Move to output
mkdir -p "$OUT_DIR"
DEST="$OUT_DIR/$APP_NAME"
if [[ -e "$DEST" ]]; then
    rm -rf "$DEST"
fi
mv "$TMPDIR/$APP_NAME" "$DEST"

echo "Built $DEST"
