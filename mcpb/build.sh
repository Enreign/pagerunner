#!/bin/bash
set -euo pipefail

VERSION="0.2.0"
REPO="Enreign/pagerunner"
BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="${SCRIPT_DIR}/dist"

TARGETS="darwin-arm64:pagerunner-macos-arm64 darwin-x64:pagerunner-macos-x86_64 linux-x64:pagerunner-linux-x86_64"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

for entry in $TARGETS; do
  platform_arch="${entry%%:*}"
  asset="${entry#*:}"
  bundle_name="pagerunner-${platform_arch}.mcpb"
  work_dir=$(mktemp -d)

  echo "=== Building ${bundle_name} ==="

  # Copy manifest
  cp "${SCRIPT_DIR}/manifest.json" "${work_dir}/manifest.json"

  # Download binary
  mkdir -p "${work_dir}/server"
  echo "  Downloading ${asset}..."
  curl -fsSL "${BASE_URL}/${asset}" -o "${work_dir}/server/pagerunner"
  chmod 755 "${work_dir}/server/pagerunner"

  # Verify SHA256
  echo "  Verifying SHA256..."
  expected=$(curl -fsSL "${BASE_URL}/${asset}.sha256" | awk '{print $1}')
  actual=$(shasum -a 256 "${work_dir}/server/pagerunner" | awk '{print $1}')
  if [ "$expected" != "$actual" ]; then
    echo "  ERROR: SHA256 mismatch for ${asset}"
    echo "    Expected: ${expected}"
    echo "    Actual:   ${actual}"
    rm -rf "$work_dir"
    exit 1
  fi
  echo "  SHA256 verified."

  # Create .mcpb (ZIP)
  (cd "$work_dir" && zip -r9 "${OUT_DIR}/${bundle_name}" manifest.json server/)
  echo "  Created ${OUT_DIR}/${bundle_name}"

  rm -rf "$work_dir"
done

echo ""
echo "=== Done ==="
ls -lh "$OUT_DIR"/*.mcpb
