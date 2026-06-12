#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

npm --prefix frontend/app ci
npm --prefix frontend/app run build:desktop

if [ ! -f apps/desktop/src-tauri/icons/icon.icns ]; then
  cargo tauri icon apps/desktop/src-tauri/icons/icon.png --output apps/desktop/src-tauri/icons
fi

cd apps/desktop
cargo tauri build --bundles "${TAURI_BUNDLES:-app,deb,msi,dmg,appimage,rpm}"
cd "$OLDPWD"

TARGET_DIR="apps/desktop/src-tauri/target/release/bundle"

# Collect every produced installer into dist/.
find "$TARGET_DIR" -type f | while IFS= read -r f; do
  case "$f" in
    *.deb|*.rpm|*.AppImage|*.msi|*.dmg|*.app.tar.gz) ;;
    *) continue ;;
  esac
  base="$(basename "$f")"
  cp "$f" "$OUT_DIR/cli-pocket-desktop-${VERSION}-${base}"
done

ls -lh "$OUT_DIR"
