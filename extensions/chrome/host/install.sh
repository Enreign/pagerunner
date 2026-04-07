#!/usr/bin/env bash
# Install the Pagerunner native messaging host for Chrome on macOS.
#
# Usage:
#   ./install.sh <extension-id>
#
# The extension ID is the 32-character string shown in chrome://extensions
# when developer mode is on and the extension is loaded unpacked.
#
# After running this script, reload the extension in Chrome.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOST_SCRIPT="${SCRIPT_DIR}/pagerunner-chrome-host.sh"
MANIFEST_TEMPLATE="${SCRIPT_DIR}/com.pagerunner.host.json"

# ── Validate args ─────────────────────────────────────────────────────────────

EXTENSION_ID="${1:-}"
if [[ -z "$EXTENSION_ID" ]]; then
  echo "Usage: $0 <chrome-extension-id>"
  echo ""
  echo "Find your extension ID in chrome://extensions (enable developer mode)."
  exit 1
fi

# Basic sanity check — Chrome extension IDs are 32 lowercase letters.
if ! [[ "$EXTENSION_ID" =~ ^[a-z]{32}$ ]]; then
  echo "Warning: '$EXTENSION_ID' does not look like a standard Chrome extension ID."
  echo "It should be 32 lowercase letters (e.g. abcdefghijklmnopqrstuvwxyzabcdef)."
  echo "Continuing anyway…"
fi

# ── macOS: install to Chrome native messaging hosts dir ───────────────────────

if [[ "$(uname)" != "Darwin" ]]; then
  echo "Error: this script currently supports macOS only."
  echo "For Linux, place the manifest in ~/.config/google-chrome/NativeMessagingHosts/"
  exit 1
fi

DEST_DIR="${HOME}/Library/Application Support/Google/Chrome/NativeMessagingHosts"
mkdir -p "$DEST_DIR"

MANIFEST_DEST="${DEST_DIR}/com.pagerunner.host.json"

# Make the host script executable.
chmod +x "$HOST_SCRIPT"

# Write the manifest with real paths substituted.
sed \
  -e "s|INSTALL_PATH_PLACEHOLDER|${HOST_SCRIPT}|g" \
  -e "s|EXTENSION_ID_PLACEHOLDER|${EXTENSION_ID}|g" \
  "$MANIFEST_TEMPLATE" > "$MANIFEST_DEST"

echo "Installed native messaging host manifest:"
echo "  ${MANIFEST_DEST}"
echo ""
echo "Host script: ${HOST_SCRIPT}"
echo ""
echo "Next steps:"
echo "  1. Open chrome://extensions"
echo "  2. Enable 'Developer mode'"
echo "  3. Click 'Load unpacked' and select: $(dirname "$SCRIPT_DIR")"
echo "  4. If you haven't already, copy your extension ID and re-run:"
echo "     $0 <extension-id>"
echo "  5. Reload the extension — the Pagerunner popup should now connect."
