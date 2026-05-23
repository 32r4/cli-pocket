#!/usr/bin/env bash
set -euo pipefail
DIR="${1:-dist}"
(cd "$DIR" && find . -maxdepth 1 -type f ! -name 'SHA256SUMS*' ! -name '*.minisig' \
  | sort \
  | xargs sha256sum) > "$DIR/SHA256SUMS"
echo "Wrote $DIR/SHA256SUMS"
