# cli-pocket developer commands. Run `just --list` for a summary.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

# ---- workspace-wide gates ----

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check --disable-fetch
    just frontend-check

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
    cargo build --release -p cli-pocket-relay

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
    cargo run -p cli-pocket-daemon -- start

dev-relay:
    cargo run -p cli-pocket-relay

dev-desktop:
    cd apps/desktop; cargo tauri dev

dev-mobile-android:
    just mobile-android-init
    cd apps/mobile; cargo tauri android dev

dev-mobile-ios:
    cd apps/mobile; cargo tauri ios dev

dev-web:
    npm --prefix frontend/app run dev:web

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
    npm --prefix frontend/app run check

# ---- guardrails ----

verify-justfile:
    @rg -n '^dev-daemon:\r?$' justfile
    @rg -n '^    cargo run -p cli-pocket-daemon -- start\r?$' justfile
