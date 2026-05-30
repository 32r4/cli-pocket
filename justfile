# cli-pocket developer commands. Run `just --list` for a summary.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

# ---- workspace-wide gates ----

check:
    @cargo fmt --check
    @cargo clippy --quiet --workspace --all-targets -- -D warnings
    @cargo deny check --disable-fetch --hide-inclusion-graph -A duplicate
    @just frontend-check

check-deps:
    @cargo deny check --disable-fetch --hide-inclusion-graph

test:
    cargo test --workspace
    npm --prefix frontend/app test

# ---- one-time setup ----

# Installs the tauri-cli used by `cargo tauri` in apps/desktop and apps/mobile,
# plus wasm-pack and frontend dependencies used by Plans F, G, and I. Idempotent.
setup:
    cargo install tauri-cli --version "^2.1" --locked
    cargo install wasm-pack --locked
    just frontend-install
    just mobile-android-init

# ---- per-target builds ----

build-daemon:
    cargo build --release -p cli-pocket-daemon

build-relay:
    npm --prefix workers/relay-cloudflare install
    npm --prefix workers/relay-cloudflare run build

build-wasm:
    wasm-pack build crates/client/client-core-wasm --target web --release

build-desktop:
    just _frontend-install-if-missing
    npm --prefix frontend/app run build:desktop
    cd apps/desktop; cargo tauri build

build-mobile-android:
    just mobile-android-init
    just _frontend-install-if-missing
    npm --prefix frontend/app run build:mobile
    cd apps/mobile; cargo tauri android build --apk --aab

build-mobile-ios:
    just _frontend-install-if-missing
    npm --prefix frontend/app run build:mobile
    cd apps/mobile; cargo tauri ios build

build-web:
    just _frontend-install-if-missing
    npm --prefix frontend/app run build:web

# ---- dev workflows ----

dev-daemon:
    cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml start

dev-relay:
    cargo run -p cli-pocket-relay -- --config crates/relay/relay-bin/relay.dev.toml

dev-desktop:
    just _frontend-install-if-missing
    cd apps/desktop; cargo tauri dev

dev-mobile-android:
    just mobile-android-init
    cd apps/mobile; cargo tauri android dev

dev-mobile-ios:
    cd apps/mobile; cargo tauri ios dev

dev-web:
    just _frontend-install-if-missing
    npm --prefix frontend/app run dev:web

# ---- deploy ----

deploy-relay-cloudflare:
    npm --prefix workers/relay-cloudflare install
    npm --prefix workers/relay-cloudflare run deploy

# ---- maintenance ----

fmt:
    cargo fmt
    npm --prefix frontend/app exec tsc -- --noEmit

clean:
    cargo clean
    just _clean-node

[windows]
_clean-node:
    $paths = @('frontend/app/dist', 'frontend/app/node_modules') | Where-Object { Test-Path $_ }
    if ($paths.Count -gt 0) { Remove-Item -LiteralPath $paths -Recurse -Force }

[unix]
_clean-node:
    rm -rf frontend/app/dist frontend/app/node_modules

# ---- release ----

# Builds every artifact this project produces. Slow.
dist:
    just build-daemon
    just build-relay
    just build-wasm
    npm --prefix frontend/app run build:desktop
    npm --prefix frontend/app run build:web

# ---- frontend ----

frontend-install:
    npm --prefix frontend/app ci

[windows]
_frontend-install-if-missing:
    if (-not (Test-Path 'frontend/app/node_modules')) { npm --prefix frontend/app ci }

[unix]
_frontend-install-if-missing:
    test -d frontend/app/node_modules || npm --prefix frontend/app ci

mobile-android-init:
    cd apps/mobile; cargo tauri android init --ci

frontend-check:
    just _frontend-install-if-missing
    @npm --prefix frontend/app run --silent check

# ---- guardrails ----

verify-justfile:
    @rg -n '^dev-daemon:\r?$' justfile
    @rg -n '^    cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml start\r?$' justfile
