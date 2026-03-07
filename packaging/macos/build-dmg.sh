#!/bin/bash
set -euo pipefail

APP_NAME="Glass"
VERSION="${1:-0.1.0}"
BUNDLE_DIR="target/macos/${APP_NAME}.app"

rm -rf "${BUNDLE_DIR}"
mkdir -p "${BUNDLE_DIR}/Contents/MacOS"
mkdir -p "${BUNDLE_DIR}/Contents/Resources"

cp target/release/glass "${BUNDLE_DIR}/Contents/MacOS/glass"

# Generate Info.plist with correct version from argument
sed "s/0\\.1\\.0/${VERSION}/g" packaging/macos/Info.plist > "${BUNDLE_DIR}/Contents/Info.plist"

hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${BUNDLE_DIR}" \
    -ov -format UDZO \
    "target/macos/Glass-${VERSION}-aarch64.dmg"

echo "Created target/macos/Glass-${VERSION}-aarch64.dmg"
