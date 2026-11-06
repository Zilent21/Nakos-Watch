#!/usr/bin/env bash
# Signs, notarizes and staples the macOS build. Needs an Apple Developer ID Application certificate
# in the login keychain, and an App Store Connect API key for notarytool.
#
#   export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
#   export APPLE_API_KEY_ID=XXXXXXXXXX
#   export APPLE_API_ISSUER=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
#   export APPLE_API_KEY_PATH=$HOME/.appstoreconnect/AuthKey_XXXXXXXXXX.p8   # never in the repo
#   scripts/sign-macos.sh
#
# Tauri can do the signing and notarizing itself when these variables are set (it reads
# APPLE_SIGNING_IDENTITY, APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_PATH). This script runs the
# same steps explicitly so each one can be checked, and works on an already-built .app as well.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
: "${APPLE_SIGNING_IDENTITY:?set APPLE_SIGNING_IDENTITY}"
: "${APPLE_API_KEY_ID:?set APPLE_API_KEY_ID}"
: "${APPLE_API_ISSUER:?set APPLE_API_ISSUER}"
: "${APPLE_API_KEY_PATH:?set APPLE_API_KEY_PATH}"

version=$(node -p "require('./package.json').version")
bundle=src-tauri/target/release/bundle
app="$bundle/macos/Nakos.app"
dmg="$bundle/dmg/Nakos_${version}_aarch64.dmg"

if [[ ! -d "$app" ]]; then
  APPLE_SIGNING_IDENTITY="$APPLE_SIGNING_IDENTITY" npx tauri build --bundles app,dmg
fi

# No entitlements: the hardened runtime needs none for Nakos (WKWebView's JIT runs in Apple's own
# process). Add one only when a concrete failure shows it is required (docs/PACKAGING.md).
echo "== sign the app (hardened runtime, timestamp)"
codesign --force --deep --options runtime --timestamp \
  --sign "$APPLE_SIGNING_IDENTITY" "$app"
codesign --verify --strict --verbose=2 "$app"

echo "== build and sign the dmg"
rm -f "$dmg"
hdiutil create -volname Nakos -srcfolder "$app" -ov -format UDZO "$dmg"
codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"

echo "== notarize (waits for Apple, usually a few minutes)"
xcrun notarytool submit "$dmg" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" --wait

echo "== staple and check"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
shasum -a 256 "$dmg"
