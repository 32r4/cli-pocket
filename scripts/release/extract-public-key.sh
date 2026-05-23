#!/usr/bin/env bash
set -euo pipefail

SOURCE="${1:-docs/release/PUBLIC_KEY.md}"
OUT="${2:-cli-pocket-minisign.pub}"

if [ ! -f "$SOURCE" ]; then
  echo "Public key source not found: $SOURCE" >&2
  exit 1
fi

if grep -q '^untrusted comment:' "$SOURCE"; then
  cp "$SOURCE" "$OUT"
else
  awk '/```text/{flag=1;next}/```/{flag=0}flag' "$SOURCE" > "$OUT"
fi

if ! grep -q '^untrusted comment:' "$OUT"; then
  echo "No minisign public-key block found in $SOURCE." >&2
  exit 1
fi

if grep -q '<paste public key' "$OUT"; then
  echo "Public key placeholder is still present in $SOURCE." >&2
  exit 1
fi

if [ "$(wc -l < "$OUT")" -lt 2 ]; then
  echo "Extracted public key from $SOURCE is incomplete." >&2
  exit 1
fi

echo "Wrote $OUT"
