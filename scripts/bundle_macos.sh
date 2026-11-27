#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="ProjPanel"
BIN_NAME="ProjPanel"
BUNDLE_ID="com.projpanel.app"
VERSION="${APP_VERSION:-$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")}"
TARGET_TRIPLE="${TARGET_TRIPLE:-}"
SKIP_BUILD="${SKIP_BUILD:-0}"
CREATE_DMG="${CREATE_DMG:-0}"

ICON_DIR="$ROOT_DIR/assets/icons/app"
if [[ -n "$TARGET_TRIPLE" ]]; then
  RELEASE_DIR="$ROOT_DIR/target/$TARGET_TRIPLE/release"
else
  RELEASE_DIR="$ROOT_DIR/target/release"
fi
BUNDLE_DIR="$RELEASE_DIR/bundle"
APP_DIR="$BUNDLE_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

if [[ ! -f "$ICON_DIR/AppIcon.icns" ]]; then
  echo "Generating macOS app icons..."
  (cd "$ROOT_DIR" && swift scripts/generate_icons.swift)
fi

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "Building release binary..."
  if [[ -n "$TARGET_TRIPLE" ]]; then
    cargo build --release --locked --target "$TARGET_TRIPLE" --manifest-path "$ROOT_DIR/Cargo.toml"
  else
    cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"
  fi
fi

BIN_PATH="$RELEASE_DIR/$BIN_NAME"
if [[ ! -f "$BIN_PATH" ]]; then
  echo "Error: release binary not found at $BIN_PATH" >&2
  exit 1
fi

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp "$BIN_PATH" "$MACOS_DIR/$APP_NAME"
chmod +x "$MACOS_DIR/$APP_NAME"

cp "$ICON_DIR/AppIcon.icns" "$RESOURCES_DIR/AppIcon.icns"
if [[ -f "$ICON_DIR/Assets.car" ]]; then
  cp "$ICON_DIR/Assets.car" "$RESOURCES_DIR/Assets.car"
fi

cat > "$CONTENTS_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>zh_CN</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIconName</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

# Apply ad-hoc code signature so macOS accepts the assembled bundle cleanly.
codesign --force --deep --sign - "$APP_DIR"

echo "Created macOS application bundle: $APP_DIR"

if [[ "$CREATE_DMG" == "1" ]]; then
  DMG_OUTPUT="${DMG_OUTPUT:-$BUNDLE_DIR/$APP_NAME-$VERSION.dmg}"
  mkdir -p "$(dirname "$DMG_OUTPUT")"
  DMG_STAGE="$(mktemp -d)"
  trap 'rm -rf "$DMG_STAGE"' EXIT

  cp -R "$APP_DIR" "$DMG_STAGE/$APP_NAME.app"
  ln -s /Applications "$DMG_STAGE/Applications"

  rm -f "$DMG_OUTPUT"
  hdiutil create \
    -volname "$APP_NAME $VERSION" \
    -srcfolder "$DMG_STAGE" \
    -ov \
    -format UDZO \
    "$DMG_OUTPUT"

  echo "Created macOS disk image: $DMG_OUTPUT"
fi
