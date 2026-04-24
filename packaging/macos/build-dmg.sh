#!/bin/bash
set -euo pipefail

APP_NAME="Glass"
VERSION="${1:-0.1.0}"
TARGET="${2:-aarch64-apple-darwin}"

# Extract arch from target triple (e.g., aarch64-apple-darwin -> aarch64)
ARCH="${TARGET%%-*}"

BUNDLE_DIR="target/macos/${APP_NAME}.app"

rm -rf "${BUNDLE_DIR}"
mkdir -p "${BUNDLE_DIR}/Contents/MacOS"
mkdir -p "${BUNDLE_DIR}/Contents/Resources"

cp "target/${TARGET}/release/glass" "${BUNDLE_DIR}/Contents/MacOS/glass"

# Generate Info.plist with correct version from argument
sed "s/0\\.1\\.0/${VERSION}/g" packaging/macos/Info.plist > "${BUNDLE_DIR}/Contents/Info.plist"

hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${BUNDLE_DIR}" \
    -ov -format UDZO \
    "target/macos/Glass-${VERSION}-${ARCH}.dmg"

echo "Created target/macos/Glass-${VERSION}-${ARCH}.dmg"
