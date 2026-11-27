#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PKG_NAME="proj-panel"
BIN_NAME="ProjPanel"
ARCH="${ARCH:-amd64}"
DEB_VERSION="${DEB_VERSION:-$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")}"
BINARY_PATH="${BINARY_PATH:-$ROOT_DIR/target/x86_64-unknown-linux-gnu/release/$BIN_NAME}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/dist}"

if [[ ! -f "$BINARY_PATH" ]]; then
  echo "Error: release binary not found at $BINARY_PATH" >&2
  exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

PKG_ROOT="$WORK_DIR/pkg"
mkdir -p \
  "$PKG_ROOT/DEBIAN" \
  "$PKG_ROOT/usr/bin" \
  "$PKG_ROOT/usr/share/applications" \
  "$PKG_ROOT/usr/share/icons/hicolor/512x512/apps" \
  "$PKG_ROOT/usr/share/doc/$PKG_NAME"

install -m 0755 "$BINARY_PATH" "$PKG_ROOT/usr/bin/$BIN_NAME"
install -m 0644 "$ROOT_DIR/packaging/linux/proj-panel.desktop" "$PKG_ROOT/usr/share/applications/proj-panel.desktop"
install -m 0644 "$ROOT_DIR/assets/icons/app/app-icon-light.png" "$PKG_ROOT/usr/share/icons/hicolor/512x512/apps/proj-panel.png"
install -m 0644 "$ROOT_DIR/README.md" "$PKG_ROOT/usr/share/doc/$PKG_NAME/README.md"

# Resolve exact shared library dependencies via dpkg-shlibdeps when available.
DEPENDS="libc6, libfontconfig1, libxkbcommon0, libxkbcommon-x11-0, libxcb1"
if command -v dpkg-shlibdeps >/dev/null 2>&1; then
  SHLIB_DIR="$WORK_DIR/shlib"
  mkdir -p "$SHLIB_DIR/debian"
  cat > "$SHLIB_DIR/debian/control" <<EOF
Source: $PKG_NAME

Package: $PKG_NAME
Architecture: $ARCH
EOF
  RESOLVED_DEPS="$(cd "$SHLIB_DIR" && dpkg-shlibdeps -O "$PKG_ROOT/usr/bin/$BIN_NAME" | sed -n 's/^shlibs:Depends=//p')"
  if [[ -n "$RESOLVED_DEPS" ]]; then
    DEPENDS="$RESOLVED_DEPS"
  fi
fi

INSTALLED_SIZE="$(du -sk "$PKG_ROOT/usr" | awk '{print $1}')"

cat > "$PKG_ROOT/DEBIAN/control" <<EOF
Package: $PKG_NAME
Version: $DEB_VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Installed-Size: $INSTALLED_SIZE
Depends: $DEPENDS
Recommends: libvulkan1, libwayland-client0, xdg-utils
Maintainer: ProjPanel Maintainers <noreply@github.com>
Homepage: https://github.com/Ariza-ye/proj-panel
Description: Cross-platform desktop workbench aggregating projects across AI agents
 ProjPanel automatically scans and aggregates projects opened across local
 AI agents and editors, with a GPUI desktop workbench and system tray popover.
EOF

cat > "$PKG_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
EOF

cat > "$PKG_ROOT/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
EOF

chmod 0755 "$PKG_ROOT/DEBIAN/postinst" "$PKG_ROOT/DEBIAN/postrm"

mkdir -p "$OUTPUT_DIR"
DEB_OUTPUT="$OUTPUT_DIR/${PKG_NAME}_${DEB_VERSION}_${ARCH}.deb"
dpkg-deb --build --root-owner-group "$PKG_ROOT" "$DEB_OUTPUT"

echo "Created Debian package: $DEB_OUTPUT"
