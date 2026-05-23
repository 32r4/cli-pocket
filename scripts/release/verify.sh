#!/usr/bin/env bash
set -euo pipefail
DIR="${1:-dist}"
PUB="${MINISIGN_PUB_FILE:-./cli-pocket-minisign.pub}"

shopt -s nullglob
fail=0
for sig in "$DIR"/*.minisig; do
  file="${sig%.minisig}"
  if minisign -V -p "$PUB" -m "$file" >/dev/null 2>&1; then
    echo "OK   $file"
  else
    echo "FAIL $file"
    fail=1
  fi
done
exit $fail
