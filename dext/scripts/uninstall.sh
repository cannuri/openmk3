#!/bin/bash
#
# uninstall.sh — user-facing uninstaller for Maschine.
#
# Removes every piece of Maschine we install:
#   1. Stops and removes the LaunchDaemon (if installed).
#   2. Deactivates the dext via `systemextensionsctl uninstall`.
#   3. Deletes /Applications/Maschine.app.
#
# Note: on macOS, truly deactivating a dext requires either the app's own
# `OSSystemExtensionRequest.deactivationRequest` API or systemextensionsctl.
# We use systemextensionsctl here — it works for both user-approved and
# dev-mode dexts.
#
# The user is still prompted in System Settings for the dext removal on
# some macOS versions; this script drives the CLI side and tells the user
# what to click.

set -euo pipefail

LABEL="com.cannuri.maschined"
PLIST_DST="/Library/LaunchDaemons/${LABEL}.plist"
APP_PATH="/Applications/Maschine.app"
DEXT_BUNDLE_ID="com.cannuri.maschine.dext"
HOST_BUNDLE_ID="com.cannuri.maschine"

# Team ID: try to read from the signed bundle, fall back to "-" (ad-hoc).
TEAM_ID="${MASCHINE_TEAM_ID:-}"
if [[ -z "$TEAM_ID" ]] && [[ -d "$APP_PATH" ]]; then
  TEAM_ID="$(/usr/bin/codesign -dvv "$APP_PATH" 2>&1 | awk -F'=' '/TeamIdentifier/ {print $2; exit}' || true)"
fi
TEAM_ID="${TEAM_ID:--}"

need_root=0
[[ -f "$PLIST_DST" ]] && need_root=1
[[ -d "$APP_PATH" ]] && need_root=1

if (( need_root )) && [[ $EUID -ne 0 ]]; then
  echo "error: uninstall.sh needs root to remove the LaunchDaemon and /Applications/Maschine.app." >&2
  echo "       re-run with: sudo $0" >&2
  exit 1
fi

# -- 1. LaunchDaemon ----------------------------------------------------------

if [[ -f "$PLIST_DST" ]]; then
  echo "==> stopping LaunchDaemon $LABEL"
  /bin/launchctl bootout "system/$LABEL" 2>/dev/null \
    || /bin/launchctl unload "$PLIST_DST" 2>/dev/null \
    || true
  echo "==> removing $PLIST_DST"
  rm -f "$PLIST_DST"
else
  echo "==> no LaunchDaemon plist at $PLIST_DST (skipping)"
fi

# -- 2. Dext deactivation ----------------------------------------------------

if /usr/bin/systemextensionsctl list 2>/dev/null | grep -qF "$DEXT_BUNDLE_ID"; then
  echo "==> uninstalling dext $DEXT_BUNDLE_ID (team=$TEAM_ID)"
  # systemextensionsctl uninstall doesn't need root itself, but it will
  # prompt via System Settings on first uninstall of a user-approved dext.
  /usr/bin/systemextensionsctl uninstall "$TEAM_ID" "$DEXT_BUNDLE_ID" || {
    echo
    echo "note: systemextensionsctl refused the uninstall. Finish it by hand:"
    echo "      System Settings → General → Login Items & Extensions →"
    echo "      Driver Extensions → (i) → toggle Maschine off, then re-run this script."
  }
else
  echo "==> dext $DEXT_BUNDLE_ID not registered (skipping)"
fi

# -- 3. App bundle -----------------------------------------------------------

if [[ -d "$APP_PATH" ]]; then
  echo "==> removing $APP_PATH"
  rm -rf "$APP_PATH"
else
  echo "==> $APP_PATH already gone"
fi

# -- 4. Caches / user prefs (non-fatal) --------------------------------------

for p in \
    "/Library/Preferences/${HOST_BUNDLE_ID}.plist" \
    "${HOME:-/var/empty}/Library/Preferences/${HOST_BUNDLE_ID}.plist" \
    "${HOME:-/var/empty}/Library/Caches/${HOST_BUNDLE_ID}" \
    "${HOME:-/var/empty}/Library/Application Support/Maschine"; do
  if [[ -e "$p" ]]; then
    echo "==> removing $p"
    rm -rf "$p"
  fi
done

cat <<EOF

Uninstall complete.

If System Settings still lists Maschine under Driver Extensions, open
System Settings → General → Login Items & Extensions → Driver Extensions →
(i) and toggle it off. That entry disappears on next reboot.
EOF
