#!/usr/bin/env bash
# package.sh — Build release binary, bundle .app, produce signed .zip
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MENUBAR_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$MENUBAR_DIR/.build/release"
APP_NAME="Pagerunner"
APP_DIR="$SCRIPT_DIR/$APP_NAME.app"
BUNDLE_ID="io.pagerunner.bar"

# Build
echo "Building release (arm64 + x86_64)..."
cd "$MENUBAR_DIR"
swift build -c release --arch arm64
swift build -c release --arch x86_64

# Create universal binary
echo "Creating universal binary..."
mkdir -p "$BUILD_DIR"
lipo -create \
    "$MENUBAR_DIR/.build/arm64-apple-macosx/release/PagerunnerBar" \
    "$MENUBAR_DIR/.build/x86_64-apple-macosx/release/PagerunnerBar" \
    -output "$BUILD_DIR/PagerunnerBar"

# Bundle
echo "Bundling .app..."
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"
cp "$BUILD_DIR/PagerunnerBar" "$APP_DIR/Contents/MacOS/PagerunnerBar"
cp "$MENUBAR_DIR/Sources/PagerunnerBar/Info.plist" "$APP_DIR/Contents/Info.plist"

# Embed Sparkle.framework
mkdir -p "$APP_DIR/Contents/Frameworks"
cp -R "$MENUBAR_DIR/.build/arm64-apple-macosx/release/Sparkle.framework" "$APP_DIR/Contents/Frameworks/"
install_name_tool -add_rpath @executable_path/../Frameworks "$APP_DIR/Contents/MacOS/PagerunnerBar"

# Code sign — use real identity for distribution, ad-hoc (-) for local dev.
# Ad-hoc signing is required for UNUserNotificationCenter to work on macOS.
SIGN_IDENTITY="${CODE_SIGN_IDENTITY:--}"
RUNTIME_FLAG=""
if [ "$SIGN_IDENTITY" = "-" ]; then
    echo "No CODE_SIGN_IDENTITY set — using ad-hoc signature (local dev only)"
else
    echo "Signing with $SIGN_IDENTITY..."
    RUNTIME_FLAG="--options runtime"
fi
codesign --force --deep --sign "$SIGN_IDENTITY" \
    $RUNTIME_FLAG \
    --entitlements "$SCRIPT_DIR/entitlements.plist" \
    "$APP_DIR"

# Zip
ZIP_PATH="$SCRIPT_DIR/$APP_NAME.zip"
ditto -c -k --keepParent "$APP_DIR" "$ZIP_PATH"
echo "Package ready: $ZIP_PATH"
