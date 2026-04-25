#!/usr/bin/env bash
#
# build-dist.sh — distribution build: Release xcodebuild → codesign bottom-up
# → pkgbuild/productbuild → notarytool submit → stapler staple.
#
# Requires a paid Apple Developer account with Apple's DriverKit entitlements
# already granted to the team. Until Apple approves those entitlements, the
# `notarytool submit --wait` step will fail with a provisioning-profile error
# even if every local command succeeds.
#
# Required env:
#   MASCHINE_TEAM_ID          10-char Apple Team ID (no spaces)
#   MASCHINE_DEVID_APP        Developer ID Application identity (full name as
#                             in `security find-identity`, e.g.
#                             "Developer ID Application: Can Tonic (XXXXXXXXXX)")
#   MASCHINE_DEVID_INSTALLER  Developer ID Installer identity (for .pkg)
#   MASCHINE_APPLE_ID         Apple ID email for notarytool
#   MASCHINE_NOTARY_PASSWORD  app-specific password (appleid.apple.com) OR set
#                             MASCHINE_NOTARY_KEYCHAIN_PROFILE instead and we
#                             pass --keychain-profile.
#
# Optional env:
#   MASCHINE_NOTARY_KEYCHAIN_PROFILE  Preferred over APPLE_ID+PASSWORD for CI.
#   MASCHINE_VERSION          Overrides 0.1.0 as marketing version.
#   MASCHINE_INSTALL_LAUNCHD  If "1", bundle install-daemon.sh as a postinstall
#                             hook so the .pkg installs maschined as a
#                             LaunchDaemon. Default: unset → .pkg only copies
#                             Maschine.app to /Applications, user launches it.
#   MASCHINE_SKIP_NOTARIZE    If "1", stop after building the .pkg (useful for
#                             dry-runs: confirms the script flows through
#                             every codesign call and pkgbuild without
#                             contacting Apple's notary service).
#   MASCHINE_DRYRUN           If "1", don't actually run codesign/pkgbuild —
#                             just echo every command. Used by CI and by the
#                             P1 scaffolding to verify the script is
#                             syntactically sound on a machine with no certs.
#
# Exit codes:
#   0   — signed + notarized + stapled .pkg produced
#   2   — missing env / toolchain / project
#   10  — xcodebuild failed
#   20  — codesign failed
#   30  — pkgbuild/productbuild failed
#   40  — notarytool rejected the submission
#   50  — stapler failed

set -euo pipefail

# -- Paths --------------------------------------------------------------------

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEXT_ROOT="$(cd "$HERE/.." && pwd)"
REPO_ROOT="$(cd "$DEXT_ROOT/.." && pwd)"
cd "$DEXT_ROOT"

CFG="Release"
DERIVED="$DEXT_ROOT/build"
PROJECT="$DEXT_ROOT/MaschineDext.xcodeproj"
SCHEME="MaschineHost"
VERSION="${MASCHINE_VERSION:-0.1.0}"
DRYRUN="${MASCHINE_DRYRUN:-0}"

HOST_ENT="$DEXT_ROOT/MaschineHost/MaschineHost.entitlements"
DEXT_ENT="$DEXT_ROOT/MaschineMk3Dext/MaschineMk3Dext.entitlements"

# -- Helpers ------------------------------------------------------------------

run() {
  if [[ "$DRYRUN" == "1" ]]; then
    echo "[dryrun] $*"
  else
    echo "==> $*"
    "$@"
  fi
}

fail() { echo "error: $*" >&2; exit "${2:-1}"; }

require_env() {
  local v="$1"
  if [[ -z "${!v:-}" ]]; then
    fail "missing required env: $v" 2
  fi
}

# -- Precondition check -------------------------------------------------------

require_env MASCHINE_TEAM_ID
require_env MASCHINE_DEVID_APP
require_env MASCHINE_DEVID_INSTALLER

if [[ -z "${MASCHINE_NOTARY_KEYCHAIN_PROFILE:-}" ]]; then
  require_env MASCHINE_APPLE_ID
  require_env MASCHINE_NOTARY_PASSWORD
fi

if [[ -z "${DEVELOPER_DIR:-}" ]] && [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

if ! /usr/bin/xcrun --find xcodebuild >/dev/null 2>&1; then
  fail "xcodebuild not found — install Xcode and run: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer" 2
fi

for f in "$HOST_ENT" "$DEXT_ENT"; do
  [[ -f "$f" ]] || fail "entitlements file missing: $f" 2
done

# -- 1. Build Release (unsigned — we sign manually below) --------------------

echo "==> [1/7] xcodebuild Release (unsigned — we sign manually)"
XCB_ARGS=(
  -project "$PROJECT"
  -scheme "$SCHEME"
  -configuration "$CFG"
  -derivedDataPath "$DERIVED"
  CODE_SIGN_IDENTITY=""
  CODE_SIGNING_REQUIRED=NO
  CODE_SIGNING_ALLOWED=NO
  DEVELOPMENT_TEAM="$MASCHINE_TEAM_ID"
  MARKETING_VERSION="$VERSION"
)

if [[ "$DRYRUN" == "1" ]]; then
  echo "[dryrun] xcrun xcodebuild ${XCB_ARGS[*]} build"
else
  /usr/bin/xcrun xcodebuild "${XCB_ARGS[@]}" build || exit 10
fi

SRC_APP="$DERIVED/Build/Products/$CFG/MaschineHost.app"
STAGING="$DERIVED/dist"
APP_BUNDLE="$STAGING/Maschine.app"
DEXT_BUNDLE="$APP_BUNDLE/Contents/Library/SystemExtensions/MaschineMk3Dext.dext"

run rm -rf "$STAGING"
run mkdir -p "$STAGING"
if [[ "$DRYRUN" != "1" ]]; then
  [[ -d "$SRC_APP" ]] || fail "xcodebuild did not produce $SRC_APP" 10
  cp -R "$SRC_APP" "$APP_BUNDLE"
fi

# Optionally bundle maschined binary (Rust daemon).
MASCHINED_BIN="$REPO_ROOT/target/release/maschined"
if [[ "$DRYRUN" != "1" ]] && [[ -x "$MASCHINED_BIN" ]]; then
  cp "$MASCHINED_BIN" "$APP_BUNDLE/Contents/MacOS/maschined"
  echo "==> bundled maschined at Contents/MacOS/maschined"
fi

# -- 2. Bundle LaunchDaemon artefacts if asked -------------------------------

if [[ "${MASCHINE_INSTALL_LAUNCHD:-0}" == "1" ]]; then
  echo "==> [2/7] LaunchDaemon variant: bundling install-daemon.sh + plist"
  PKG_SCRIPTS="$STAGING/pkg-scripts"
  run mkdir -p "$PKG_SCRIPTS"
  # postinstall dispatches to our install-daemon.sh
  if [[ "$DRYRUN" != "1" ]]; then
    cat >"$PKG_SCRIPTS/postinstall" <<'POSTINSTALL'
#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
"$SCRIPT_DIR/install-daemon.sh"
exit 0
POSTINSTALL
    chmod +x "$PKG_SCRIPTS/postinstall"
    cp "$HERE/install-daemon.sh" "$PKG_SCRIPTS/install-daemon.sh"
    chmod +x "$PKG_SCRIPTS/install-daemon.sh"
    cp "$DEXT_ROOT/MaschineHost/LaunchDaemon.plist" "$PKG_SCRIPTS/com.cannuri.maschined.plist"
  fi
  PKGBUILD_SCRIPTS_ARG=(--scripts "$PKG_SCRIPTS")
else
  echo "==> [2/7] app-launched variant (MASCHINE_INSTALL_LAUNCHD not set)"
  PKGBUILD_SCRIPTS_ARG=()
fi

# -- 3. Codesign bottom-up ---------------------------------------------------
# R2 §4.3 explicit order. NEVER use --deep.

echo "==> [3/7] codesign bottom-up"

CODESIGN_COMMON=(--force --options runtime --timestamp --sign "$MASCHINE_DEVID_APP")

# 3a: host binary
run codesign "${CODESIGN_COMMON[@]}" \
  "$APP_BUNDLE/Contents/MacOS/MaschineHost"

# 3b: bundled maschined (if present)
if [[ "$DRYRUN" != "1" ]] && [[ -f "$APP_BUNDLE/Contents/MacOS/maschined" ]]; then
  run codesign "${CODESIGN_COMMON[@]}" "$APP_BUNDLE/Contents/MacOS/maschined"
elif [[ "$DRYRUN" == "1" ]]; then
  echo "[dryrun] codesign maschined (if bundled)"
fi

# 3c: dext's inner Mach-O.
# DriverKit dexts use a flat bundle layout (the Mach-O sits at
# $DEXT/MaschineMk3Dext, not $DEXT/Contents/MacOS/MaschineMk3Dext). R2 §4.3
# shows the nested macOS-app layout; the driverkit SDK flattens it.
DEXT_MACHO="$DEXT_BUNDLE/MaschineMk3Dext"
if [[ "$DRYRUN" != "1" ]] && [[ ! -f "$DEXT_MACHO" ]]; then
  # Fall back to the nested layout in case a future DriverKit SDK changes it.
  if [[ -f "$DEXT_BUNDLE/Contents/MacOS/MaschineMk3Dext" ]]; then
    DEXT_MACHO="$DEXT_BUNDLE/Contents/MacOS/MaschineMk3Dext"
  else
    fail "dext Mach-O not found at $DEXT_MACHO" 20
  fi
fi
run codesign "${CODESIGN_COMMON[@]}" "$DEXT_MACHO"

# 3d: dext bundle itself WITH its entitlements
run codesign "${CODESIGN_COMMON[@]}" \
  --entitlements "$DEXT_ENT" \
  "$DEXT_BUNDLE"

# 3e: host .app WITH its entitlements (signs the outer wrapper last)
run codesign "${CODESIGN_COMMON[@]}" \
  --entitlements "$HOST_ENT" \
  "$APP_BUNDLE"

# Verify — this would catch a dropped entitlement.
if [[ "$DRYRUN" != "1" ]]; then
  codesign --verify --verbose=2 --strict "$APP_BUNDLE" || exit 20
  codesign --verify --verbose=2 --strict "$DEXT_BUNDLE" || exit 20
fi

# -- 4. pkgbuild (component) -------------------------------------------------

COMPONENT_PKG="$DERIVED/Maschine-component.pkg"
FINAL_PKG="$DERIVED/Maschine-Mk3-Host-$VERSION.pkg"

echo "==> [4/7] pkgbuild component"
run pkgbuild \
  --root "$STAGING" \
  --identifier "com.cannuri.maschine.installer" \
  --version "$VERSION" \
  --install-location "/Applications" \
  ${PKGBUILD_SCRIPTS_ARG[@]+"${PKGBUILD_SCRIPTS_ARG[@]}"} \
  --sign "$MASCHINE_DEVID_INSTALLER" \
  --timestamp \
  "$COMPONENT_PKG"

# -- 5. productbuild (distribution) -----------------------------------------

echo "==> [5/7] productbuild distribution"
run productbuild \
  --package "$COMPONENT_PKG" \
  --sign "$MASCHINE_DEVID_INSTALLER" \
  --timestamp \
  "$FINAL_PKG"

if [[ "${MASCHINE_SKIP_NOTARIZE:-0}" == "1" ]]; then
  echo
  echo "MASCHINE_SKIP_NOTARIZE=1: stopping before notarization."
  echo "  Package: $FINAL_PKG"
  exit 0
fi

# -- 6. notarytool submit --wait --------------------------------------------

echo "==> [6/7] notarytool submit"
NOTARY_ARGS=("$FINAL_PKG" --wait)
if [[ -n "${MASCHINE_NOTARY_KEYCHAIN_PROFILE:-}" ]]; then
  NOTARY_ARGS+=(--keychain-profile "$MASCHINE_NOTARY_KEYCHAIN_PROFILE")
else
  NOTARY_ARGS+=(
    --apple-id "$MASCHINE_APPLE_ID"
    --team-id "$MASCHINE_TEAM_ID"
    --password "$MASCHINE_NOTARY_PASSWORD"
  )
fi

run xcrun notarytool submit "${NOTARY_ARGS[@]}"

# -- 7. stapler staple ------------------------------------------------------

echo "==> [7/7] stapler staple"
run xcrun stapler staple "$FINAL_PKG"
run xcrun stapler validate "$FINAL_PKG"

cat <<EOF

Distribution build complete.
  Package: $FINAL_PKG
  Version: $VERSION
  Team ID: $MASCHINE_TEAM_ID
  Variant: $([[ "${MASCHINE_INSTALL_LAUNCHD:-0}" == "1" ]] && echo "LaunchDaemon (maschined runs at boot)" || echo "app-launched (user opens Maschine.app)")

Ship \`$FINAL_PKG\` to end users. See dext/docs/INSTALL.md for the end-user
install walkthrough.
EOF
