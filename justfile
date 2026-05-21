# cli-pocket developer commands. Run `just --list` for a summary.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

# ---- workspace-wide gates ----

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    npm --prefix webview/terminal run lint

test:
    cargo test --workspace
    npm --prefix webview/terminal test

# ---- one-time setup ----

# Installs the tauri-cli used by `cargo tauri` in apps/desktop and apps/mobile,
# plus wasm-pack used by Plans F and I. Idempotent.
setup:
    cargo install tauri-cli --version "^2.1" --locked
    cargo install wasm-pack --locked

# ---- per-target builds ----

build-daemon:
    cargo build --release -p cli-pocket-daemon

build-relay:
    cargo build --release -p cli-pocket-relay

build-wasm:
    wasm-pack build crates/client/client-core-wasm --target web --release

build-webview-tauri:
    just _build-webview-tauri

build-webview-web:
    just _build-webview-web

# Built in Plan H — recipes shown here so `just --list` is the single index.
build-desktop:
    just _build-webview-tauri
    cd apps/desktop; cargo tauri build

build-mobile-android:
    just _build-webview-tauri
    cd apps/mobile; cargo tauri android build --apk --aab

build-mobile-ios:
    just _build-webview-tauri
    cd apps/mobile; cargo tauri ios build

# Built in Plan I — needs `just build-wasm` first.
build-web:
    just build-wasm
    npm --prefix apps/web run build

# ---- dev workflows ----

dev-daemon:
    cargo run -p cli-pocket-daemon

dev-relay:
    cargo run -p cli-pocket-relay

dev-desktop:
    cd apps/desktop; cargo tauri dev

dev-mobile-android:
    cd apps/mobile; cargo tauri android dev

dev-mobile-ios:
    cd apps/mobile; cargo tauri ios dev

dev-web:
    npm --prefix apps/web run dev

# ---- maintenance ----

fmt:
    cargo fmt
    npm --prefix webview/terminal exec tsc -- --noEmit

clean:
    cargo clean
    just _clean-node

[windows]
_clean-node:
    Remove-Item -LiteralPath 'webview/terminal/dist', 'webview/terminal/node_modules', 'apps/web/dist', 'apps/web/node_modules' -Recurse -Force -ErrorAction SilentlyContinue

[unix]
_clean-node:
    rm -rf webview/terminal/dist webview/terminal/node_modules apps/web/dist apps/web/node_modules

[windows]
_build-webview-tauri:
    cd webview/terminal; $env:VITE_CLIENT_KIND='tauri'; npm exec vite -- build --outDir dist/tauri

[unix]
_build-webview-tauri:
    cd webview/terminal && VITE_CLIENT_KIND=tauri npm exec vite -- build --outDir dist/tauri

[windows]
_build-webview-web:
    cd webview/terminal; $env:VITE_CLIENT_KIND='web'; npm exec vite -- build --outDir dist/web

[unix]
_build-webview-web:
    cd webview/terminal && VITE_CLIENT_KIND=web npm exec vite -- build --outDir dist/web

# ---- release ----

# Builds every artifact this project produces. Slow.
dist:
    just build-daemon
    just build-relay
    just build-wasm
    just build-webview-tauri
    just build-webview-web
