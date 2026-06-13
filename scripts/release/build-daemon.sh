#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
: "${TARGET:?TARGET env var required}"

ARTIFACT_TARGET="${ARTIFACT_TARGET:-$TARGET}"
ARTIFACT="cli-pocket-daemon-${VERSION}-${ARTIFACT_TARGET}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

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

archive_stage_zip() {
  local stage_parent="$1"
  local stage_name="$2"
  local output_path="$3"

  if command -v zip >/dev/null 2>&1; then
    (
      cd "$stage_parent"
      zip -qr "$output_path" "$stage_name"
    )
    return
  fi

  if command -v powershell.exe >/dev/null 2>&1; then
    local stage_path_win
    local output_path_win
    if command -v cygpath >/dev/null 2>&1; then
      stage_path_win="$(cygpath -w "${stage_parent}/${stage_name}")"
      output_path_win="$(cygpath -w "$output_path")"
    elif command -v wslpath >/dev/null 2>&1; then
      stage_path_win="$(wslpath -w "${stage_parent}/${stage_name}")"
      output_path_win="$(wslpath -w "$output_path")"
    else
      echo "zip packaging requires cygpath or wslpath when using powershell.exe" >&2
      exit 1
    fi
    powershell.exe -NoLogo -Command \
      "Compress-Archive -LiteralPath '$stage_path_win' -DestinationPath '$output_path_win' -Force" \
      >/dev/null
    return
  fi

  echo "zip packaging requires either zip or powershell.exe" >&2
  exit 1
}

STAGE_ROOT="$(mktemp -d)"
STAGE="${STAGE_ROOT}/${ARTIFACT}"
mkdir -p "$STAGE"
cp "target/${TARGET}/release/cli-pocket-daemon${BIN_SUFFIX}" "$STAGE/"
cp README.md LICENSE-MIT LICENSE-APACHE "$STAGE/" 2>/dev/null || true
cp docs/release/VERIFY.md "$STAGE/" || true

case "$EXT" in
  tar.gz)
    tar -C "$STAGE_ROOT" -czf "$OUT_DIR/${ARTIFACT}.tar.gz" "$ARTIFACT"
    ;;
  zip)
    archive_stage_zip "$STAGE_ROOT" "$ARTIFACT" "$OUT_DIR/${ARTIFACT}.zip"
    ;;
esac

echo "Built $OUT_DIR/${ARTIFACT}.${EXT}"
