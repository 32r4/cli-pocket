# cli-pocket

> Cross-platform remote terminal. Self-hosted, end-to-end encrypted, no SaaS.

cli-pocket is an OSS remote terminal: run a daemon on the machine you want to
reach, connect from desktop / mobile / web. Sessions are end-to-end encrypted
with Noise; the optional self-hosted relay only forwards ciphertext.

**Status:** pre-alpha. The desktop app can start an embedded daemon, connect to
direct WebSocket endpoints, import relay pairing links, and open remote terminal
sessions. Pairing-link generation/import is wired through the daemon and UI.

## Quick start (developer)

```bash
just --list
just check
just test
just build-daemon
just build-web
```

Requires Rust (pinned in `rust-toolchain.toml`), Node (pinned in `.nvmrc`),
`just`, and `cargo-deny`. See `docs/superpowers/specs/` for the full design.

## Architecture

| Area | What it does |
|---|---|
| `frontend/app` | Shared UI application for web, desktop, and mobile |
| `apps/{desktop,mobile}/src-tauri` | Native host shells and platform commands |
| `crates/shared/{proto,crypto,transport}` | Wire protocol, Noise session crypto, WebSocket transport |
| `crates/server/{pty,daemon-core,daemon-bin}` | The host-side daemon |
| `crates/relay/{relay-core,relay-bin}` | Optional self-hosted relay |
| `crates/client/{client-core,client-core-wasm}` | Shared client runtime |
| `workers/relay-cloudflare` | Cloudflare Workers + Durable Objects relay deployment scaffold |

## Security

See [SECURITY.md](./SECURITY.md) for vulnerability reporting.

## License

[AGPL-3.0-only](./LICENSE).
