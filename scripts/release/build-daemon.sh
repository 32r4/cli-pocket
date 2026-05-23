#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
: "${TARGET:?TARGET env var required}"

ARTIFACT="cli-pocket-daemon-${VERSION}-${TARGET}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

cargo build --release --locked --target "$TARGET" -p cli-pocket-daemon

case "$TARGET" in
  *-windows-*)
    BIN_SUFFIX=".exe"
    EXT="zip"
    ;;
  *)
    BIN_SUFFIX=""
    EXT="tar.gz"
    ;;
esac

STAGE="$(mktemp -d)/$ARTIFACT"
mkdir -p "$STAGE"
cp "target/${TARGET}/release/cli-pocket-daemon${BIN_SUFFIX}" "$STAGE/"
cp README.md LICENSE-MIT LICENSE-APACHE "$STAGE/" 2>/dev/null || true
cp docs/release/VERIFY.md "$STAGE/" || true

case "$EXT" in
  tar.gz)
    tar -C "$(dirname "$STAGE")" -czf "$OUT_DIR/${ARTIFACT}.tar.gz" "$(basename "$STAGE")"
    ;;
  zip)
    (cd "$(dirname "$STAGE")" && zip -qr "$OLDPWD/$OUT_DIR/${ARTIFACT}.zip" "$(basename "$STAGE")")
    ;;
esac

echo "Built $OUT_DIR/${ARTIFACT}.${EXT}"
