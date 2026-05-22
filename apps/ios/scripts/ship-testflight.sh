#!/usr/bin/env bash
#
# ship-testflight.sh — one-command TestFlight release.
#
# Auto-bumps the build number, archives the app, exports an IPA, and
# uploads to App Store Connect. TestFlight picks the build up within
# 5-30 minutes and notifies internal testers.
#
# Prereqs (one time):
#   1. App record created at https://appstoreconnect.apple.com/apps
#      with bundle id com.enreign.pagerunner.ios.
#   2. App Store Connect API key: .p8 at
#      ~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8
#      Export these env vars (add to ~/.zshrc):
#          export ASC_KEY_ID=...
#          export ASC_ISSUER_ID=...
#
# Usage:
#   ./scripts/ship-testflight.sh              # bump + archive + upload
#   SKIP_UPLOAD=1 ./scripts/ship-testflight.sh # archive only, no upload
#
set -euo pipefail

: "${ASC_KEY_ID:?set ASC_KEY_ID (App Store Connect API key id)}"
: "${ASC_ISSUER_ID:?set ASC_ISSUER_ID (App Store Connect issuer id)}"

cd "$(dirname "$0")/.."

SCHEME="Pagerunner"
PROJECT="Pagerunner.xcodeproj"
INFO_PLIST="Sources/Pagerunner/Info.plist"
BUILD_DIR="$(mktemp -d -t pagerunner-ship)"
ARCHIVE_PATH="$BUILD_DIR/Pagerunner.xcarchive"
EXPORT_DIR="$BUILD_DIR/export"
EXPORT_OPTS="$BUILD_DIR/ExportOptions.plist"

# 1. Bump CFBundleVersion (unique per upload)
CURRENT_BUILD=$(plutil -extract CFBundleVersion raw "$INFO_PLIST" 2>/dev/null || echo "0")
NEW_BUILD=$((CURRENT_BUILD + 1))
plutil -replace CFBundleVersion -string "$NEW_BUILD" "$INFO_PLIST"
echo "▶︎ build $CURRENT_BUILD → $NEW_BUILD"

# 2. Archive
echo "▶︎ archiving…"
xcodebuild \
  -project "$PROJECT" \
  -scheme "$SCHEME" \
  -destination 'generic/platform=iOS' \
  -archivePath "$ARCHIVE_PATH" \
  -allowProvisioningUpdates \
  archive \
  | xcbeautify 2>/dev/null || xcodebuild \
    -project "$PROJECT" \
    -scheme "$SCHEME" \
    -destination 'generic/platform=iOS' \
    -archivePath "$ARCHIVE_PATH" \
    -allowProvisioningUpdates \
    archive \
    | tail -5

# 3. Write export options
cat > "$EXPORT_OPTS" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>app-store</string>
    <key>destination</key>
    <string>export</string>
    <key>signingStyle</key>
    <string>automatic</string>
    <key>stripSwiftSymbols</key>
    <true/>
    <key>uploadBitcode</key>
    <false/>
    <key>uploadSymbols</key>
    <true/>
</dict>
</plist>
EOF

# 4. Export IPA
echo "▶︎ exporting IPA…"
xcodebuild \
  -exportArchive \
  -archivePath "$ARCHIVE_PATH" \
  -exportPath "$EXPORT_DIR" \
  -exportOptionsPlist "$EXPORT_OPTS" \
  -allowProvisioningUpdates \
  | tail -3

IPA=$(find "$EXPORT_DIR" -name "*.ipa" | head -1)
echo "▶︎ built $IPA"
ls -lh "$IPA"

if [[ "${SKIP_UPLOAD:-0}" == "1" ]]; then
  echo "▶︎ SKIP_UPLOAD set — stopping. IPA at: $IPA"
  exit 0
fi

# 5. Upload to App Store Connect
echo "▶︎ uploading to App Store Connect…"
xcrun altool --upload-app \
  -f "$IPA" \
  -t ios \
  --apiKey "$ASC_KEY_ID" \
  --apiIssuer "$ASC_ISSUER_ID"

echo "✓ uploaded build $NEW_BUILD"
echo "  Processing on App Store Connect (5-30 min)."
echo "  https://appstoreconnect.apple.com/apps"
