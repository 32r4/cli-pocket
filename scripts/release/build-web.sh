#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?VERSION env var required}"
OUT_DIR="${OUT_DIR:-dist}"
mkdir -p "$OUT_DIR"

cd frontend/app
if [ -f package-lock.json ]; then
  npm ci
else
  npm install --no-package-lock
fi
npm run build:web
cd "$OLDPWD"

ART="cli-pocket-web-${VERSION}"
tar -C frontend/app -czf "$OUT_DIR/${ART}.tar.gz" dist/web/

echo "Built $OUT_DIR/${ART}.tar.gz"
