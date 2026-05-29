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
    npm --prefix frontend/app run build
    cd apps/desktop; cargo tauri build

build-mobile-android:
    just mobile-android-init
    npm --prefix frontend/app run build
    cd apps/mobile; cargo tauri android build --apk --aab

build-mobile-ios:
    npm --prefix frontend/app run build
    cd apps/mobile; cargo tauri ios build

build-web:
    npm --prefix frontend/app run build:web

# ---- dev workflows ----

dev-daemon:
    cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml start

dev-relay:
    cargo run -p cli-pocket-relay -- --config crates/relay/relay-bin/relay.dev.toml

dev-desktop:
    cd apps/desktop; cargo tauri dev

dev-mobile-android:
    just mobile-android-init
    cd apps/mobile; cargo tauri android dev

dev-mobile-ios:
    cd apps/mobile; cargo tauri ios dev

dev-web:
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
    Remove-Item -LiteralPath 'frontend/app/dist', 'frontend/app/node_modules' -Recurse -Force -ErrorAction SilentlyContinue

[unix]
_clean-node:
    rm -rf frontend/app/dist frontend/app/node_modules

# ---- release ----

# Builds every artifact this project produces. Slow.
dist:
    just build-daemon
    just build-relay
    just build-wasm
    npm --prefix frontend/app run build
    npm --prefix frontend/app run build:web

# ---- frontend ----

frontend-install:
    npm --prefix frontend/app install

mobile-android-init:
    cd apps/mobile; cargo tauri android init --ci

frontend-check:
    @npm --prefix frontend/app run --silent check

# ---- guardrails ----

verify-justfile:
    @rg -n '^dev-daemon:\r?$' justfile
    @rg -n '^    cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml start\r?$' justfile
