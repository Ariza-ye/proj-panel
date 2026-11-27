#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PKG_NAME="proj-panel"
BIN_NAME="ProjPanel"
RAW_VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")"
ARCH_PKGVER="${ARCH_PKGVER:-${RAW_VERSION//-/_}}"
BINARY_PATH="${BINARY_PATH:-$ROOT_DIR/target/x86_64-unknown-linux-gnu/release/$BIN_NAME}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/dist}"

if [[ ! -f "$BINARY_PATH" ]]; then
  echo "Error: release binary not found at $BINARY_PATH" >&2
  exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

# Stage pre-built artifacts into a clean makepkg build directory.
cp "$BINARY_PATH" "$WORK_DIR/$BIN_NAME"
cp "$ROOT_DIR/packaging/linux/proj-panel.desktop" "$WORK_DIR/proj-panel.desktop"
cp "$ROOT_DIR/assets/icons/app/app-icon-light.png" "$WORK_DIR/proj-panel.png"
cp "$ROOT_DIR/README.md" "$WORK_DIR/README.md"

cat > "$WORK_DIR/PKGBUILD" <<EOF
pkgname=${PKG_NAME}
pkgver=${ARCH_PKGVER}
pkgrel=1
pkgdesc="Cross-platform GPUI Kit desktop tool aggregating projects loaded across AI agents"
arch=('x86_64')
url="https://github.com/Ariza-ye/proj-panel"
license=('custom')
depends=(
  'glibc'
  'gcc-libs'
  'fontconfig'
  'libxkbcommon'
  'libxkbcommon-x11'
  'libxcb'
  'wayland'
  'vulkan-icd-loader'
)
optdepends=(
  'xdg-utils: open project directories in file manager'
)
options=('!debug' '!strip')

package() {
  install -Dm755 "\${startdir}/${BIN_NAME}" "\${pkgdir}/usr/bin/${BIN_NAME}"
  install -Dm644 "\${startdir}/proj-panel.desktop" "\${pkgdir}/usr/share/applications/proj-panel.desktop"
  install -Dm644 "\${startdir}/proj-panel.png" "\${pkgdir}/usr/share/icons/hicolor/512x512/apps/proj-panel.png"
  install -Dm644 "\${startdir}/README.md" "\${pkgdir}/usr/share/doc/${PKG_NAME}/README.md"
}
EOF

# makepkg refuses to run as root; execute under a dedicated unprivileged user in CI containers.
if [[ "$(id -u)" -eq 0 ]]; then
  if ! id -u builder >/dev/null 2>&1; then
    useradd -m -s /bin/bash builder
  fi
  chown -R builder:builder "$WORK_DIR"
  su -s /bin/bash builder -c "cd '$WORK_DIR' && makepkg -f --noconfirm --nodeps"
else
  (cd "$WORK_DIR" && makepkg -f --noconfirm --nodeps)
fi

mkdir -p "$OUTPUT_DIR"
shopt -s nullglob
PKG_FILES=("$WORK_DIR"/${PKG_NAME}-${ARCH_PKGVER}-1-*.pkg.tar.*)
if [[ "${#PKG_FILES[@]}" -eq 0 ]]; then
  echo "Error: makepkg did not produce a package archive" >&2
  exit 1
fi

for pkg in "${PKG_FILES[@]}"; do
  cp "$pkg" "$OUTPUT_DIR/"
  echo "Created Arch package: $OUTPUT_DIR/$(basename "$pkg")"
done
