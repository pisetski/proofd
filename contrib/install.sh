#!/usr/bin/env bash
# Installs proofd.app to ~/Applications, links its proofd CLI into ~/.local/bin and
# registers the "Proofread Selection" service. Safe to re-run to update.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="$HOME/Applications/proofd.app"
CLI_LINK="$HOME/.local/bin/proofd"
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

FIRST_INSTALL=false
[[ -d "$APP_PATH" ]] || FIRST_INSTALL=true

# A running host keeps serving the old build until it exits.
pkill -f "$APP_PATH/Contents/MacOS/proofd-service-host" || true

bash "$REPO_ROOT/macos/build.sh" "$(dirname "$APP_PATH")"

mkdir -p "$(dirname "$CLI_LINK")"
ln -sf "$APP_PATH/Contents/MacOS/proofd" "$CLI_LINK"
echo "Linked $CLI_LINK"

"$LSREGISTER" -f "$APP_PATH"
/System/Library/CoreServices/pbs -update

[[ ":$PATH:" == *":$HOME/.local/bin:"* ]] ||
    echo "Add ~/.local/bin to PATH to run proofd from the terminal."

if $FIRST_INSTALL; then
    echo "Enable \"Proofread Selection\" under Text in the Services settings that just opened."
    open "x-apple.systempreferences:com.apple.Keyboard-Settings.extension"
fi
