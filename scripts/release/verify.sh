#!/usr/bin/env bash
set -euo pipefail
DIR="${1:-dist}"
PUB="${MINISIGN_PUB_FILE:-./cli-pocket-minisign.pub}"

shopt -s nullglob
signatures=("$DIR"/*.minisig)
if [ "${#signatures[@]}" -eq 0 ]; then
  echo "No minisign signatures found in $DIR." >&2
  exit 1
fi

fail=0
for sig in "${signatures[@]}"; do
  file="${sig%.minisig}"
  if minisign -V -p "$PUB" -m "$file" >/dev/null 2>&1; then
    echo "OK   $file"
  else
    echo "FAIL $file"
    fail=1
  fi
done
exit $fail
