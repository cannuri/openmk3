#!/bin/bash
#
# install-daemon.sh — invoked as the .pkg postinstall hook when the installer
# was built with MASCHINE_INSTALL_LAUNCHD=1.
#
# Copies com.cannuri.maschined.plist into /Library/LaunchDaemons and loads it.
# The plist launches /Applications/Maschine.app/Contents/MacOS/maschined at
# boot with RunAtLoad + KeepAlive.
#
# Two invocation paths:
#   1. As a .pkg postinstall script — Installer.app runs it as root with the
#      plist sitting alongside us in the pkg-scripts directory.
#   2. Manually from a terminal — user runs `sudo install-daemon.sh` after
#      dropping Maschine.app into /Applications themselves.

set -euo pipefail

LABEL="com.cannuri.maschined"
PLIST_DST="/Library/LaunchDaemons/${LABEL}.plist"
APP_PATH="/Applications/Maschine.app"
EXEC_PATH="$APP_PATH/Contents/MacOS/maschined"

# Must be root (LaunchDaemons live in a root-only directory).
if [[ $EUID -ne 0 ]]; then
  echo "error: install-daemon.sh must run as root (sudo)" >&2
  exit 1
fi

# Locate the plist: next to us (pkg-scripts) first, then in the .app.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PLIST_SRC=""
for candidate in \
    "$SCRIPT_DIR/${LABEL}.plist" \
    "$APP_PATH/Contents/Resources/${LABEL}.plist" \
    "$APP_PATH/Contents/Library/LaunchDaemons/${LABEL}.plist"; do
  if [[ -f "$candidate" ]]; then
    PLIST_SRC="$candidate"
    break
  fi
done

if [[ -z "$PLIST_SRC" ]]; then
  echo "error: LaunchDaemon plist not found; looked in pkg-scripts and $APP_PATH" >&2
  exit 1
fi

if [[ ! -x "$EXEC_PATH" ]]; then
  echo "warning: $EXEC_PATH missing or not executable — the daemon will fail to start." >&2
  echo "         This is expected if the host .app is pre-M3 (no bundled maschined yet)." >&2
fi

# Unload existing if already loaded (idempotency).
if /bin/launchctl list | awk '{print $3}' | grep -qx "$LABEL"; then
  echo "==> unloading existing $LABEL"
  /bin/launchctl bootout "system/$LABEL" 2>/dev/null || /bin/launchctl unload "$PLIST_DST" 2>/dev/null || true
fi

echo "==> installing plist: $PLIST_SRC -> $PLIST_DST"
install -m 0644 -o root -g wheel "$PLIST_SRC" "$PLIST_DST"

echo "==> loading $LABEL"
# bootstrap is the modern launchctl verb; fall back to load on older systems.
if ! /bin/launchctl bootstrap system "$PLIST_DST" 2>/dev/null; then
  /bin/launchctl load -w "$PLIST_DST"
fi

echo "==> enabled; status:"
/bin/launchctl print "system/$LABEL" 2>/dev/null | head -20 || \
  /bin/launchctl list | grep -E "\b${LABEL}\b" || true

echo "done."
