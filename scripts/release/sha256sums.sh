#!/usr/bin/env bash
set -euo pipefail
DIR="${1:-dist}"

compute_sha256() {
  local file="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
    return
  fi

  if command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoLogo -Command \
      "(Get-FileHash -Algorithm SHA256 -LiteralPath '$file').Hash.ToLowerInvariant()" \
      | tr -d '\r'
    return
  fi

  echo "No SHA-256 implementation found." >&2
  exit 1
}

shopt -s nullglob
files=()
for f in "$DIR"/*; do
  case "$f" in
    *.minisig|*SHA256SUMS) continue ;;
  esac
  [ -f "$f" ] || continue
  files+=("$f")
done

if [ "${#files[@]}" -eq 0 ]; then
  echo "No release artifacts found in $DIR." >&2
  exit 1
fi

LC_ALL=C printf '%s\n' "${files[@]##*/}" | sort | while IFS= read -r name; do
  hash="$(compute_sha256 "$DIR/$name")"
  printf '%s  %s\n' "$hash" "$name"
done > "$DIR/SHA256SUMS"

echo "Wrote $DIR/SHA256SUMS"
