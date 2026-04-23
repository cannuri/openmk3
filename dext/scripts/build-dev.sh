#!/usr/bin/env bash
#
# build-dev.sh — local developer build.
#
# Produces an UNSIGNED (or ad-hoc signed) Maschine.app at
#   dext/build/Maschine.app
# that can be launched directly from Finder or CLI to activate the embedded
# dext. Assumes the operator has already done (once, machine-global):
#
#   1. Boot to Recovery (⌘R) → Terminal → `csrutil disable` → reboot
#   2. `sudo systemextensionsctl developer on`
#
# Codesigning:
#   - DriverKit rejects pure ad-hoc signing on a default-locked Mac, but with
#     SIP off + developer-mode on, macOS accepts an unsigned or ad-hoc dext.
#   - If a codesigning identity exists, we let Xcode's Automatic signing pick
#     it (typically "Apple Development" — fine for local testing).
#   - Otherwise we disable signing entirely (CODE_SIGNING_ALLOWED=NO).
#
# This script does NOT:
#   - Notarize   (dev builds aren't stapled)
#   - Build a .pkg
#   - Touch maschined (the .app shell has no Rust payload yet in M1; later
#     milestones will copy the Rust daemon into Contents/MacOS)
#
# Exit codes:
#   0 — Maschine.app was built
#   1 — xcodebuild failed or Xcode not installed
#   2 — prerequisite check failed

set -euo pipefail

# -- Paths --------------------------------------------------------------------

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEXT_ROOT="$(cd "$HERE/.." && pwd)"
cd "$DEXT_ROOT"

CFG="${CONFIG:-Debug}"
DERIVED="$DEXT_ROOT/build"
PROJECT="$DEXT_ROOT/MaschineDext.xcodeproj"
SCHEME="MaschineHost"

# -- Xcode -------------------------------------------------------------------

if [[ -z "${DEVELOPER_DIR:-}" ]] && [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

if ! /usr/bin/xcrun --find xcodebuild >/dev/null 2>&1; then
  echo "error: xcodebuild not found. Install Xcode (not just Command Line Tools) at /Applications/Xcode.app." >&2
  echo "       sudo xcode-select -s /Applications/Xcode.app/Contents/Developer" >&2
  exit 2
fi

if [[ ! -d "$PROJECT" ]]; then
  echo "error: Xcode project missing at $PROJECT" >&2
  exit 2
fi

# -- Signing mode ------------------------------------------------------------

XCB_EXTRA=()
if security find-identity -v -p codesigning 2>/dev/null | grep -q 'valid identities found' \
   && ! security find-identity -v -p codesigning 2>/dev/null | grep -q '^ *0 valid identities found'; then
  echo "==> codesigning identity found — letting Xcode Automatic signing pick one"
else
  echo "==> no codesigning identity found — building unsigned (ok for SIP-off + developer-mode)"
  XCB_EXTRA+=(
    CODE_SIGNING_ALLOWED=NO
    CODE_SIGNING_REQUIRED=NO
    CODE_SIGN_IDENTITY=""
    ENTITLEMENTS_REQUIRED=NO
  )
fi

# -- Build -------------------------------------------------------------------

echo "==> xcodebuild: scheme=$SCHEME config=$CFG"
/usr/bin/xcrun xcodebuild \
  -project "$PROJECT" \
  -scheme "$SCHEME" \
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

# -- Optional: bundle maschined if the Rust daemon was built -----------------
# (no-op for M1; later milestones will cargo-build and drop the binary in)
MASCHINED_BIN="$DEXT_ROOT/../target/release/maschined"
if [[ -x "$MASCHINED_BIN" ]]; then
  echo "==> bundling maschined: $MASCHINED_BIN"
  cp "$MASCHINED_BIN" "$DST_APP/Contents/MacOS/maschined"
fi

EMBEDDED="$(find "$DST_APP/Contents/Library/SystemExtensions" -maxdepth 1 -name '*.dext' 2>/dev/null | head -n1 || true)"

cat <<EOF

Dev build complete.
  Product:   $DST_APP
  Embedded:  ${EMBEDDED:-<none>}

Next steps (see dext/docs/INSTALL.md §Developer mode):
  1. Confirm SIP is off:      csrutil status
  2. Confirm developer mode:  systemextensionsctl developer
  3. Launch the host app:     open "$DST_APP"
     (or: "$DST_APP/Contents/MacOS/MaschineHost")
  4. Approve in: System Settings → General → Login Items & Extensions →
     Driver Extensions → (i) → toggle Maschine on.
EOF
