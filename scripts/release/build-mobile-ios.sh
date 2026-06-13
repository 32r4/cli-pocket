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
cargo tauri ios init --ci
if [ -n "${APPLE_DEVELOPMENT_TEAM:-}" ] || [ -n "${APPLE_TEAM_ID:-}" ] || [ -n "${APPLE_API_KEY:-}" ] || [ -n "${APPLE_API_KEY_PATH:-}" ] || [ -n "${IOS_CERTIFICATE:-}" ] || [ -n "${IOS_MOBILE_PROVISION:-}" ] || grep -q '"developmentTeam"' src-tauri/tauri.conf.json; then
  cargo tauri ios build
else
  echo "APPLE_DEVELOPMENT_TEAM is not set and tauri.conf.json has no iOS developmentTeam; building unsigned simulator app instead." >&2
  SIM_TARGET="aarch64-sim"
  if [ "$(uname -m)" = "x86_64" ]; then
    SIM_TARGET="x86_64"
  fi
  cargo tauri ios build --no-sign --target "$SIM_TARGET"
fi
cd "$OLDPWD"

copied=0

if [ -n "${APPLE_DEVELOPMENT_TEAM:-}" ] || [ -n "${APPLE_TEAM_ID:-}" ] || [ -n "${APPLE_API_KEY:-}" ] || [ -n "${APPLE_API_KEY_PATH:-}" ] || [ -n "${IOS_CERTIFICATE:-}" ] || [ -n "${IOS_MOBILE_PROVISION:-}" ] || grep -q '"developmentTeam"' "$APP_CONFIG"; then
  TARGET_DIR="apps/mobile/src-tauri/gen/apple/build"
  while IFS= read -r f; do
    [ -f "$f" ] || continue
    base="$(basename "$f")"
    cp "$f" "$OUT_DIR/cli-pocket-ios-${VERSION}-${base}"
    copied=1
  done < <(find "$TARGET_DIR" -type f -name '*.ipa')

  if [ "$copied" -eq 0 ]; then
    echo "iOS build completed but no .ipa artifacts were found in $TARGET_DIR." >&2
    exit 1
  fi
else
  APP_DIR="$(find apps/mobile/src-tauri/gen/apple -type d -name '*.app' | head -n 1)"
  if [ -z "$APP_DIR" ]; then
    echo "iOS simulator build completed but no .app artifact was found." >&2
    exit 1
  fi
  SIM_ARCH="arm64"
  if [ "$(uname -m)" = "x86_64" ]; then
    SIM_ARCH="x64"
  fi
  base="$(basename "$APP_DIR")"
  tar -C "$(dirname "$APP_DIR")" -czf "$OUT_DIR/cli-pocket-ios-${VERSION}-simulator-unsigned-${SIM_ARCH}.app.tar.gz" "$base"
  copied=1
fi

ls -lh "$OUT_DIR"
