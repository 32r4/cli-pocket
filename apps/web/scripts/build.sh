#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."

# Build the wasm package first (idempotent — wasm-pack will skip if up-to-date).
(cd ../../crates/client/client-core-wasm && wasm-pack build --target web --release)

# Typecheck + Vite build.
npx tsc --noEmit
npx vite build
