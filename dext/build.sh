#!/usr/bin/env bash
#
# build.sh — one-shot build for the MaschineHost .app + the embedded
# MaschineMk3Dext system extension.
#
# Idempotent: safe to rerun. On success, the .app lands both at the
# native derived-data path and at ./build/Maschine.app.
#
# Signing mode:
#   - If security(1) finds a valid Developer ID / Apple Development
#     identity, the default automatic signing path runs. You will need
#     DEVELOPMENT_TEAM set (see README §0) for the dext SDK to accept it.
#   - Otherwise the script disables code signing entirely
#     (CODE_SIGNING_ALLOWED=NO). DriverKit rejects ad-hoc signing, so
#     unsigned is the only local option until a real cert is available.
#

set -euo pipefail

cd "$(dirname "$0")"

CFG="${CONFIG:-Debug}"
DERIVED="$PWD/build"

if [[ -z "${DEVELOPER_DIR:-}" ]] && [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

XCB_EXTRA=()
if ! security find-identity -v -p codesigning 2>/dev/null | grep -q 'valid identities found' \
   || security find-identity -v -p codesigning 2>/dev/null | grep -q '^ *0 valid identities found'; then
  echo "==> no codesigning identity found — building unsigned"
  XCB_EXTRA+=(CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY="" ENTITLEMENTS_REQUIRED=NO)
fi

echo "==> xcodebuild: scheme=MaschineHost config=$CFG derived=$DERIVED"
xcodebuild \
  -project MaschineDext.xcodeproj \
  -scheme MaschineHost \
  -configuration "$CFG" \
  -derivedDataPath "$DERIVED" \
  "${XCB_EXTRA[@]}" \
  build

SRC_APP="$DERIVED/Build/Products/$CFG/MaschineHost.app"
DST_APP="$DERIVED/Maschine.app"

if [[ ! -d "$SRC_APP" ]]; then
  echo "error: expected product not found at $SRC_APP" >&2
  exit 1
fi

echo "==> copying $SRC_APP -> $DST_APP"
rm -rf "$DST_APP"
cp -R "$SRC_APP" "$DST_APP"

EMBEDDED="$(find "$DST_APP/Contents/Library/SystemExtensions" -maxdepth 1 -name '*.dext' 2>/dev/null | head -n1)"
echo
echo "Build complete."
echo "  Product:  $DST_APP"
echo "  Embedded: ${EMBEDDED:-none}"
echo
echo "Next: ./build/Maschine.app/Contents/MacOS/MaschineHost"
