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

npm ci
npx playwright install chromium chromium-headless-shell
npm --prefix frontend/app ci
npm --prefix frontend/app run build:mobile

cd apps/mobile
cargo tauri android init --ci
cd "$OLDPWD"

node scripts/mobile/configure-android-signing.mjs

cd apps/mobile
cargo tauri android build --split-per-abi --apk --aab
cd "$OLDPWD"

TARGET_DIR="apps/mobile/src-tauri/gen/android/app/build/outputs"
copied=0

while IFS= read -r f; do
  [ -f "$f" ] || continue
  base="$(basename "$f")"
  cp "$f" "$OUT_DIR/cli-pocket-android-${VERSION}-${base}"
  copied=1
done < <(find "$TARGET_DIR" -type f \( -name '*.apk' -o -name '*.aab' \))

if [ "$copied" -eq 0 ]; then
  echo "Android build completed but no .apk or .aab artifacts were found in $TARGET_DIR." >&2
  exit 1
fi

ls -lh "$OUT_DIR"
