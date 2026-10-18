#!/usr/bin/env bash
set -euo pipefail

# End-to-end smoke test for the installed "Proofread Selection" service.
# Needs a logged-in GUI session, an installed app (bash contrib/install.sh) and a
# working provider in ~/.config/proofd/config.toml, so it is not part of CI.

APP_PATH="${APP_PATH:-$HOME/Applications/proofd.app}"
SERVICE_LOG="$HOME/Library/Logs/proofd-service.log"
INPUT="${1:-this are a sentense with erors}"
PBS=/System/Library/CoreServices/pbs

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMPDIR="$(mktemp -d /tmp/proofd-test.XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

pass() { echo "ok   $*"; }
fail() {
    echo "FAIL $*" >&2
    exit 1
}

[[ -d "$APP_PATH" ]] || fail "$APP_PATH not installed; run: bash contrib/install.sh"
pass "app installed at $APP_PATH"

# Registration: pbs must route the menu item to this bundle with the plist's selector.
# Entries are back to back, so take the closest NSBundlePath before the menu item
# and the NSMessage right after it.
DUMP="$("$PBS" -dump_pboard)"
grep -q 'default = "Proofread Selection"' <<<"$DUMP" ||
    fail "'Proofread Selection' not registered with pbs; run: $PBS -update"
REGISTERED_PATH="$(grep -B6 'default = "Proofread Selection"' <<<"$DUMP" |
    sed -n 's/.*NSBundlePath = "\(.*\)";/\1/p' | tail -1)"
ENTRY="$(grep -A3 'default = "Proofread Selection"' <<<"$DUMP")"
[[ "$REGISTERED_PATH" == "$APP_PATH" ]] ||
    fail "service registered for '$REGISTERED_PATH', expected '$APP_PATH' (stale copy?)"
EXPECTED_MESSAGE="$(/usr/libexec/PlistBuddy -c 'Print :NSServices:0:NSMessage' "$APP_PATH/Contents/Info.plist")"
grep -q "NSMessage = \"\{0,1\}$EXPECTED_MESSAGE\"\{0,1\};" <<<"$ENTRY" ||
    fail "pbs has a stale NSMessage (installed plist says '$EXPECTED_MESSAGE'); run: $PBS -update"
pass "registered with pbs (NSMessage=$EXPECTED_MESSAGE)"

clang -fobjc-arc -framework AppKit "$SCRIPT_DIR/tests/perform_service.m" -o "$TMPDIR/perform_service"

touch "$SERVICE_LOG"
LOG_OFFSET=$(($(wc -c <"$SERVICE_LOG") + 1))
START="$(date '+%Y-%m-%d %H:%M:%S')"

set +e
OUTPUT="$("$TMPDIR/perform_service" "$INPUT")"
STATUS=$?
set -e
NEW_LOG="$(tail -c +"$LOG_OFFSET" "$SERVICE_LOG")"

if ! grep -q "Service invoked" <<<"$NEW_LOG"; then
    echo "AppKit messages from the service host:" >&2
    /usr/bin/log show --style compact --start "$START" \
        --predicate 'process == "proofd-service-host" AND sender == "AppKit" AND NOT category IN {"Appearance", "AutomaticTermination", "StateRestoration"}' >&2 || true
    fail "service method was never called (request dropped before reaching ServiceHost.m)"
fi
pass "service method invoked"

if [[ $STATUS -ne 0 ]]; then
    echo "service log:" >&2
    echo "$NEW_LOG" >&2
    fail "text was not proofread (exit $STATUS): '$OUTPUT'"
fi
pass "proofread: '$INPUT' -> '$OUTPUT'"
