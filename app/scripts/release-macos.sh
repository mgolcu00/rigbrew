#!/bin/sh
# Builds the macOS download: the app with rigbrewd inside, signed with a Developer ID, notarized
# by Apple and stapled, so it opens on any Mac without a warning.
#
#   APPLE_SIGNING_IDENTITY   "Developer ID Application: Name (TEAMID)", in the login keychain
#   APPLE_API_KEY            App Store Connect API key ID        \
#   APPLE_API_ISSUER         its issuer ID                        } for notarization
#   APPLE_API_KEY_PATH       path to the AuthKey_<id>.p8 file    /
#
#   RIGBREW_ACCOUNT_DIR      the private account module, for the official download (optional)
#
# Without the three APPLE_API_* values the app is signed but not notarized (it says so).
# With APPLE_SIGNING_IDENTITY=- it is signed ad hoc: for this Mac only.
set -eu
cd "$(dirname "$0")/.."
: "${APPLE_SIGNING_IDENTITY:?set APPLE_SIGNING_IDENTITY (\"Developer ID Application: …\", or - for this Mac only)}"
if [ "$APPLE_SIGNING_IDENTITY" != "-" ] && ! security find-identity -v -p codesigning | grep -Fq "$APPLE_SIGNING_IDENTITY"; then
  echo "No such signing identity in the keychain: $APPLE_SIGNING_IDENTITY" >&2
  security find-identity -v -p codesigning | sed -E 's/^ +[0-9]+\) [0-9A-F]+ /  /' >&2
  exit 1
fi
export APPLE_SIGNING_IDENTITY
# The official download has accounts: RIGBREW_ACCOUNT_DIR is the private module (app-account/ in
# the server repository). Without it the build has no account at all, like any build from source.
if [ -n "${RIGBREW_ACCOUNT_DIR:-}" ]; then
  sh "$RIGBREW_ACCOUNT_DIR/link.sh" "$PWD"
  RIGBREW_ACCOUNT=1 pnpm release --features account
  echo "Built with accounts."
else
  pnpm release
  echo "Built WITHOUT accounts: set RIGBREW_ACCOUNT_DIR for the official download."
fi

out=src-tauri/target/release/bundle
app=$out/macos/Rigbrew.app
dmg=$(ls -t "$out"/dmg/*.dmg 2>/dev/null | head -1)
[ -d "$app" ] && [ -n "$dmg" ] || { echo "The build left no app or disk image in $out" >&2; exit 1; }
echo "--- signature"
codesign --verify --deep --strict "$app"
codesign -dv "$app" 2>&1 | grep -E "Authority|TeamIdentifier|flags|Signature" || true
codesign -d --entitlements - "$app" 2>/dev/null | grep -q audio-input || { echo "The microphone entitlement is missing from the signature" >&2; exit 1; }
if [ "$APPLE_SIGNING_IDENTITY" = "-" ]; then
  echo "Signed ad hoc: opens on this Mac only."
elif [ -n "${APPLE_API_KEY:-}" ]; then
  # Tauri notarizes and staples the app; the disk image gets its own ticket here.
  xcrun notarytool submit "$dmg" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
  xcrun stapler staple "$dmg"
  echo "--- Gatekeeper"
  spctl --assess --type execute -vv "$app"
  spctl --assess --type open --context context:primary-signature -vv "$dmg"
else
  echo "Signed, NOT notarized: set APPLE_API_KEY, APPLE_API_ISSUER and APPLE_API_KEY_PATH."
fi
echo "$dmg"
shasum -a 256 "$dmg"
