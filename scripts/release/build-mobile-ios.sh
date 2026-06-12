#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Skipping iOS release build on $(uname -s)." >&2
  exit 0
fi

APP_CONFIG="apps/mobile/src-tauri/tauri.conf.json"
if [ ! -f "$APP_CONFIG" ]; then
  echo "Skipping iOS release build: $APP_CONFIG is missing." >&2
  exit 0
fi

npm ci
npx playwright install chromium
npm --prefix frontend/app ci
npm --prefix frontend/app run build:mobile

cd apps/mobile
cargo tauri ios build
cd "$OLDPWD"

TARGET_DIR="apps/mobile/src-tauri/gen/apple/build"
copied=0

while IFS= read -r f; do
  [ -f "$f" ] || continue
  base="$(basename "$f")"
  cp "$f" "$OUT_DIR/cli-pocket-mobile-${VERSION}-${base}"
  copied=1
done < <(find "$TARGET_DIR" -type f -name '*.ipa')

if [ "$copied" -eq 0 ]; then
  echo "iOS build completed but no .ipa artifacts were found in $TARGET_DIR." >&2
  exit 1
fi

ls -lh "$OUT_DIR"
