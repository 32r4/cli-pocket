# cli-pocket

cli-pocket is a remote terminal workspace for pairing clients with a host daemon through a relay. The repository contains the Rust daemon and relay, shared protocol and crypto crates, a Tauri desktop/mobile client, a Vite/React frontend, and a Cloudflare Workers relay deployment.

## Status

This repository is pre-1.0. Crates and packages are private, internal APIs may change, and only `main` is supported for security fixes.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `crates/shared/proto` | Shared wire protocol, frame codec, terminal messages, relay messages |
| `crates/shared/crypto` | Identity, Noise, SPAKE2, and redaction helpers |
| `crates/shared/transport` | Transport abstractions and WebSocket implementations |
| `crates/server/pty` | PTY, terminal parser, and output buffering |
| `crates/server/daemon-core` | Daemon session manager, pairing, client DB, relay dialer |
| `crates/server/daemon-bin` | `cli-pocket-daemon` binary |
| `crates/relay/relay-core` | Rust relay server and relay protocol behavior |
| `crates/relay/relay-bin` | `cli-pocket-relay` binary |
| `crates/client/client-core` | Platform-independent client state machine |
| `crates/client/client-core-wasm` | WASM client adapter for web builds |
| `crates/client/tauri-app` | Shared Tauri client commands/runtime integration |
| `crates/client/tauri-bindings` | Native bindings used by Tauri clients |
| `frontend/app` | Vite, React, xterm.js frontend |
| `apps/desktop/src-tauri` | Desktop Tauri shell |
| `apps/mobile/src-tauri` | Mobile Tauri shell |
| `workers/relay-cloudflare` | Cloudflare Workers + Durable Objects relay |
| `scripts/release` | Release artifact, signing, and checksum scripts |
| `packaging` | Debian, RPM, and Homebrew packaging templates |

## Prerequisites

- Rust toolchain from `rust-toolchain.toml` (`1.95.0`, with `rustfmt`, `clippy`, and `wasm32-unknown-unknown`)
- Node.js `>=20.19.0` (`.nvmrc` pins the major version to `20`)
- `just`
- `cargo-deny` for `just check`
- `tauri-cli` and `wasm-pack` for app and WASM workflows

Install the project-specific Rust tools and frontend dependencies:

```sh
just setup
```

`just setup` installs `tauri-cli`, installs `wasm-pack`, runs `npm ci` in `frontend/app`, and initializes the mobile Android Tauri project.

## Development

List available commands:

```sh
just --list
```

Run the required pre-commit gate:

```sh
just check
```

`just check` runs:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo deny check --disable-fetch --hide-inclusion-graph -A duplicate`
- `npm --prefix frontend/app run --silent check`

It does not run tests.

Run the full workspace test command only when needed:

```sh
just test
```

## Running Locally

Start the daemon with the development config:

```sh
just dev-daemon
```

Start the Rust relay with the development config:

```sh
just dev-relay
```

Start the desktop app:

```sh
just dev-desktop
```

Start the web frontend:

```sh
just dev-web
```

The frontend uses separate Vite modes and ports:

- desktop: `5173`
- mobile: `5174`
- web: `5175`

## Daemon

The daemon binary is `cli-pocket-daemon`.

Build it:

```sh
just build-daemon
```

Run it directly with a config file:

```sh
cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml start
```

Useful subcommands:

```sh
cargo run -p cli-pocket-daemon -- print-sample-config
cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml pair-key
cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml pair-url
cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml pair-qr
cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml list-clients
cargo run -p cli-pocket-daemon -- --config crates/server/daemon-bin/daemon.dev.toml revoke <client-id>
```

If `--config` is omitted, the daemon uses `~/.cli-pocket/daemon.toml`. The same value can be supplied through `CLI_POCKET_CONFIG`.

Development config lives at `crates/server/daemon-bin/daemon.dev.toml`. Build-oriented sample config lives at `crates/server/daemon-bin/daemon.build.toml`.

## Relay

The Rust relay binary is `cli-pocket-relay`.

Build the Rust relay binary:

```sh
cargo build --release -p cli-pocket-relay
```

Run the local Rust relay:

```sh
just dev-relay
```

Run it directly:

```sh
cargo run -p cli-pocket-relay -- --config crates/relay/relay-bin/relay.dev.toml
```

Print the default relay config:

```sh
cargo run -p cli-pocket-relay -- print-sample-config
```

If `--config` is omitted, the relay uses its default config. The same value can be supplied through `CLI_POCKET_RELAY_CONFIG`.

The Cloudflare implementation is in `workers/relay-cloudflare`. It exposes `/health`, `/ws/server`, and `/ws/client`, uses Durable Objects for per-server relay state, and is configured by `workers/relay-cloudflare/wrangler.toml`.

Build or deploy the Cloudflare relay:

```sh
just build-relay
just deploy-relay-cloudflare
```

## Frontend and Apps

The shared frontend is in `frontend/app` and uses Vite, React, xterm.js, Zustand, and Tauri APIs.

Common commands:

```sh
just frontend-install
just frontend-check
just build-web
just build-desktop
just build-mobile-android
just build-mobile-ios
```

Build the WASM client package:

```sh
just build-wasm
```

Regenerate app icons after changing `frontend/app/public/favicon.svg`:

```sh
npm ci
npm run generate-icons
```

## Release and Verification

Release scripts are under `scripts/release`. They build per-target artifacts into `dist/`, sign artifacts with minisign, and create SHA-256 checksum files.

Release process documentation:

- `docs/release/PROCEDURE.md`
- `docs/release/VERIFY.md`
- `docs/release/KEY_ROTATION.md`
- `docs/release/PUBLIC_KEY.md`

## Security

Do not open public GitHub issues for security problems. Use GitHub private vulnerability reporting as described in `SECURITY.md`.

Security-sensitive areas include the daemon, relay, client code, wire protocol, Noise handshake, SPAKE2 pairing flow, key persistence, and revocation behavior.

## License

AGPL-3.0-only. See `LICENSE`.
