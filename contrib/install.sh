#!/usr/bin/env bash
# proofd installer — one-command setup for Option A (macOS Services, zero permissions).
#
# What it does:
#   1. Builds the Rust `proofd` binary in release mode.
#   2. Installs it to ~/.local/bin/proofd (adds to PATH if missing from shell profile).
#   3. Builds proofd.app (Swift NSServices host) from macos/ServiceHost.swift.
#   4. Installs proofd.app to /Applications.
#   5. Triggers Launch Services rescan so the "Proofread Selection" service appears.
#   6. Creates ~/.config/proofd/config.toml if missing (auto-detects provider).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/<user>/proofd/main/contrib/install.sh | bash
#
# Result: select text in any app → right-click → Services → Proofread Selection.
# No Accessibility or Input Monitoring permissions required.

set -euo pipefail

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
(cd "$REPO_ROOT/macos" && bash build.sh "$HOME/Applications")
APP_PATH="$HOME/Applications/$APP_NAME"
if [[ ! -d "$APP_PATH" ]]; then
    # Fall back to /Applications if ~/Applications doesn't exist
    (cd "$REPO_ROOT/macos" && bash build.sh "/Applications")
    APP_PATH="/Applications/$APP_NAME"
fi
log "Installed $APP_NAME to $APP_PATH"

# --- 4. Trigger Launch Services rescan ---
log "Registering NSServices with Launch Services…"
/usr/bin/lsregister -f "$APP_PATH" 2>/dev/null || true
# Touch to force Launch Services to re-scan
touch "$APP_PATH"
log "Done. The 'Proofread Selection' service is now available."

# --- 5. Create default config if missing ---
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
log "Usage:"
log "  1. Select text in any app."
log "  2. Right-click → Services → Proofread Selection"
log "     (or assign a keyboard shortcut in System Settings → Keyboard → Keyboard Shortcuts → Services)"
log ""
log "The service runs proofd polish on the selected text and replaces it inline."
log "No Accessibility or Input Monitoring permissions are required."
log ""
log "Tip: The first time you open proofd.app, macOS may ask to confirm it's from an unidentified developer."
log "     Open it once via right-click → Open, then the service will work."
