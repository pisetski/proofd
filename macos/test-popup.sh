#!/usr/bin/env bash
set -euo pipefail

# Native integration tests with fake providers and private pasteboards.
# Requires a logged-in macOS desktop; never calls an LLM or touches user config.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_DIR="$(mktemp -d /tmp/proofd-popup-test.XXXXXX)"
trap 'rm -rf "$TEST_DIR"' EXIT
# Optional directory for rendering only our panel (no screen recording access).
if [[ $# -gt 0 ]]; then
    mkdir -p "$1"
    export PROOFD_TEST_SNAPSHOT_DIR="$1"
fi
clang -fobjc-arc -fblocks -Wall -Wextra -Wno-unused-parameter \
    -DPROOFD_NO_MAIN -mmacosx-version-min=14.0 -framework AppKit -framework Foundation \
    "$SCRIPT_DIR/ProofreadProcess.m" "$SCRIPT_DIR/ResultPanel.m" \
    "$SCRIPT_DIR/ServiceHost.m" "$SCRIPT_DIR/tests/popup_tests.m" \
    -o "$TEST_DIR/popup-tests"
"$TEST_DIR/popup-tests"
