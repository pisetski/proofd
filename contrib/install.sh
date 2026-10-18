#!/usr/bin/env bash
# proofd installer — one-command setup of the macOS "Proofread Selection" service.
#
# What it does:
#   1. Builds the Rust `proofd` binary in release mode.
#   2. Installs it to ~/.local/bin/proofd (adds to PATH if missing from shell profile).
#   3. Builds proofd.app (Objective-C NSServices host) from macos/ServiceHost.m.
#   4. Installs proofd.app to ~/Applications (or /Applications as fallback).
#   5. Triggers Launch Services rescan so the "Proofread Selection" service appears.
#   6. Creates ~/.config/proofd/config.toml if missing (auto-detects provider).
#
# Usage:
#   bash contrib/install.sh
#
# Result: select text in any app → right-click → Services → Proofread Selection.
# No Accessibility or Input Monitoring permissions required.

set -euo pipefail

# Stop a running service host so the rebuilt app is the one that registers
pkill -f "proofd-service-host" 2>/dev/null || true
sleep 1

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_NAME="proofd"
APP_NAME="proofd.app"
LOCAL_BIN_DIR="$HOME/.local/bin"
LOCAL_BIN_PATH="$LOCAL_BIN_DIR/$BIN_NAME"
CONFIG_DIR="$HOME/.config/proofd"
CONFIG_PATH="$CONFIG_DIR/config.toml"

log() { echo "[proofd] $*"; }
warn() { echo "[proofd] WARNING: $*" >&2; }
err() { echo "[proofd] ERROR: $*" >&2; exit 1; }

# --- 1. Build Rust binary in release mode ---
log "Building proofd binary (release)…"
(cd "$REPO_ROOT" && cargo build --release --bin proofd)

RELEASE_BIN="$REPO_ROOT/target/release/$BIN_NAME"
if [[ ! -x "$RELEASE_BIN" ]]; then
    err "Release binary not found at $RELEASE_BIN"
fi

# --- 2. Install binary ---
mkdir -p "$LOCAL_BIN_DIR"
cp "$RELEASE_BIN" "$LOCAL_BIN_PATH"
chmod +x "$LOCAL_BIN_PATH"
log "Installed $BIN_NAME to $LOCAL_BIN_PATH"

# Ensure PATH includes ~/.local/bin in common shell profiles
add_to_path() {
    local profile="$1"
    local marker="# proofd PATH"
    if [[ -f "$profile" ]] && grep -q "$marker" "$profile"; then
        return
    fi
    {
        echo ""
        echo "$marker"
        echo "export PATH=\"$HOME/.local/bin:\$PATH\""
    } >> "$profile"
    log "Added ~/.local/bin to PATH in $profile"
}

for profile in "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.bashrc" "$HOME/.profile"; do
    if [[ -f "$profile" ]]; then
        add_to_path "$profile"
    fi
done

export PATH="$HOME/.local/bin:$PATH"

# --- 3. Build proofd.app ---
log "Building proofd.app…"
# Prefer ~/Applications (user-writable, no sudo needed); fall back to /Applications
if [[ ! -d "$HOME/Applications" ]]; then
    mkdir -p "$HOME/Applications"
fi
(cd "$REPO_ROOT/macos" && bash build.sh "$HOME/Applications")
APP_PATH="$HOME/Applications/$APP_NAME"
log "Installed $APP_NAME to $APP_PATH"

# --- 4. Remove Gatekeeper quarantine ---
log "Clearing Gatekeeper quarantine…"
xattr -cr "$APP_PATH" 2>/dev/null || true

# --- 5. Launch app once to register NSServices ---
log "Launching proofd.app once to register services…"
open "$APP_PATH" 2>/dev/null || true
sleep 3

# --- 6. Trigger Launch Services rescan ---
LOG_REGISTER="/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
log "Registering NSServices with Launch Services…"
"$LOG_REGISTER" -f "$APP_PATH" 2>/dev/null || true
# Touch to force Launch Services to re-scan
touch "$APP_PATH"
log "Done. The 'Proofread Selection' service is now registered."

# --- 7. Create default config if missing ---
if [[ ! -f "$CONFIG_PATH" ]]; then
    mkdir -p "$CONFIG_DIR"
    # Auto-detect provider
    if command -v claude >/dev/null 2>&1; then
        PROVIDER="claude"
        MODEL="haiku"
    elif command -v opencode >/dev/null 2>&1; then
        PROVIDER="opencode"
        MODEL="opencode/mimo-v2.6-flash-free"
    else
        PROVIDER="claude"
        MODEL="haiku"
    fi
    cat > "$CONFIG_PATH" <<EOF
provider = "$PROVIDER"
model = "$MODEL"
timeout_secs = 15
max_input_chars = 12000
EOF
    log "Created default config at $CONFIG_PATH (provider=$PROVIDER)"
else
    log "Config already exists at $CONFIG_PATH; leaving unchanged"
fi

log ""
log "Installation complete!"
log ""
log "The service runs proofd polish on the selected text and replaces it inline."
log "No Accessibility or Input Monitoring permissions are required."
log ""
log "IMPORTANT — final steps to make the service appear:"
log "  1. In Finder, right-click ~/Applications/proofd.app → Open (to bypass Gatekeeper)"
log "  2. Open System Settings → Keyboard → Keyboard Shortcuts → Services"
log "  3. Find 'Proofread Selection' under Text and make sure it's checked"
log "  4. (Optional) Assign a keyboard shortcut to 'Proofread Selection'"
log ""
log "If something goes wrong, check the service log:"
log "  tail -f ~/Library/Logs/proofd-service.log"
