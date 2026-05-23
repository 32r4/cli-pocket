#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

cd apps/web
npm ci
npm run build
cd "$OLDPWD"

ART="cli-pocket-web-${VERSION}"
tar -C apps/web -czf "$OUT_DIR/${ART}.tar.gz" dist/

echo "Built $OUT_DIR/${ART}.tar.gz"
