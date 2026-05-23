#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

APP_CONFIG="apps/mobile/src-tauri/tauri.conf.json"
if [ ! -f "$APP_CONFIG" ]; then
  echo "Skipping Android release build: $APP_CONFIG is missing." >&2
  exit 0
fi

cd webview/terminal
npm ci
npm run build:tauri
cd "$OLDPWD"

cd apps/mobile
cargo tauri android build --apk --aab
cd "$OLDPWD"

TARGET_DIR="apps/mobile/src-tauri/gen/android/app/build/outputs"
copied=0

shopt -s globstar nullglob
for f in "$TARGET_DIR"/**/*.apk "$TARGET_DIR"/**/*.aab; do
  [ -f "$f" ] || continue
  base="$(basename "$f")"
  cp "$f" "$OUT_DIR/cli-pocket-mobile-${VERSION}-${base}"
  copied=1
done

if [ "$copied" -eq 0 ]; then
  echo "Android build completed but no .apk or .aab artifacts were found in $TARGET_DIR." >&2
  exit 1
fi

ls -lh "$OUT_DIR"
