#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

cd webview/terminal
npm ci
npm run build:tauri
cd "$OLDPWD"

cd apps/desktop
cargo tauri build --bundles "${TAURI_BUNDLES:-app,deb,msi,dmg,appimage,rpm}"
cd "$OLDPWD"

TARGET_DIR="apps/desktop/src-tauri/target/release/bundle"

# Collect every produced installer into dist/.
shopt -s globstar nullglob
for f in "$TARGET_DIR"/**/*; do
  case "$f" in
    *.deb|*.rpm|*.AppImage|*.msi|*.dmg|*.app.tar.gz) ;;
    *) continue ;;
  esac
  base="$(basename "$f")"
  cp "$f" "$OUT_DIR/cli-pocket-desktop-${VERSION}-${base}"
done

ls -lh "$OUT_DIR"
