#!/usr/bin/env bash
# Sign every file in $1 (default: dist/) with the maintainer's minisign key.
# Requires $MINISIGN_PASSWORD to be set; reads the key from $MINISIGN_KEY_FILE
# (default: ./cli-pocket-minisign.key). In CI the secret is materialised into
# that path by the workflow step that calls this script.
set -euo pipefail
DIR="${1:-dist}"
KEY="${MINISIGN_KEY_FILE:-./cli-pocket-minisign.key}"
: "${MINISIGN_PASSWORD:?MINISIGN_PASSWORD env var required}"

shopt -s nullglob
for f in "$DIR"/*; do
  case "$f" in
    *.minisig|*SHA256SUMS) continue ;;
  esac
  if [ -f "$f" ]; then
    echo "Signing $f"
    echo "$MINISIGN_PASSWORD" | minisign -S -s "$KEY" -m "$f" -t "cli-pocket release"
  fi
done
