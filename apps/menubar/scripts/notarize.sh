#!/usr/bin/env bash
# notarize.sh — Submit to Apple notarization and staple ticket
# Required env vars: APPLE_TEAM_ID, APPLE_ID, NOTARIZE_PASSWORD, CODE_SIGN_IDENTITY
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ZIP_PATH="$SCRIPT_DIR/Pagerunner.zip"
APP_PATH="$SCRIPT_DIR/Pagerunner.app"

: "${APPLE_TEAM_ID:?APPLE_TEAM_ID must be set}"
: "${APPLE_ID:?APPLE_ID must be set}"
: "${NOTARIZE_PASSWORD:?NOTARIZE_PASSWORD must be set}"

echo "Submitting $ZIP_PATH for notarization..."
xcrun notarytool submit "$ZIP_PATH" \
    --team-id "$APPLE_TEAM_ID" \
    --apple-id "$APPLE_ID" \
    --password "$NOTARIZE_PASSWORD" \
    --wait

echo "Stapling ticket to $APP_PATH..."
xcrun stapler staple "$APP_PATH"

echo "Notarization complete."
