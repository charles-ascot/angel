#!/usr/bin/env bash
# install.sh — installs Angel.app and sets it up to run at login.
#
# Run this after a successful `pnpm tauri build`:
#   bash scripts/install.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

APP_SRC="$REPO_ROOT/src-tauri/target/release/bundle/macos/Angel.app"
APP_DEST="/Applications/Angel.app"
PLIST_SRC="$SCRIPT_DIR/com.angel.app.plist"
PLIST_DEST="$HOME/Library/LaunchAgents/com.angel.app.plist"

# ── Verify the build exists ───────────────────────────────────────────────────
if [ ! -d "$APP_SRC" ]; then
    echo "Error: Angel.app not found at $APP_SRC"
    echo "Run 'pnpm tauri build' first, then re-run this script."
    exit 1
fi

# ── Install Angel.app to /Applications ───────────────────────────────────────
echo "Installing Angel.app to /Applications..."
if [ -d "$APP_DEST" ]; then
    echo "  Removing existing Angel.app..."
    rm -rf "$APP_DEST"
fi
cp -r "$APP_SRC" "$APP_DEST"
echo "  Done."

# ── Install the launchd plist ─────────────────────────────────────────────────
echo "Installing launch agent..."
mkdir -p "$HOME/Library/LaunchAgents"
cp "$PLIST_SRC" "$PLIST_DEST"

# Unload any existing agent first (ignore errors if not loaded)
launchctl unload "$PLIST_DEST" 2>/dev/null || true
launchctl load "$PLIST_DEST"
echo "  Done — Angel will now start automatically at login."

# ── Add to Dock ───────────────────────────────────────────────────────────────
echo ""
echo "To add Angel to your Dock:"
echo "  Open Finder → Applications → drag Angel.app to your Dock"
echo ""
echo "To view Angel logs at any time:"
echo "  tail -f /tmp/angel.log"
echo ""
echo "To stop/start manually:"
echo "  launchctl unload ~/Library/LaunchAgents/com.angel.app.plist"
echo "  launchctl load  ~/Library/LaunchAgents/com.angel.app.plist"
echo ""
echo "Angel is running. ✓"
