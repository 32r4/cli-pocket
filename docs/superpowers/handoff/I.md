# Plan I Handoff: Web App + Wasm CliPocketClient

Date completed: 2026-05-23

## What shipped

### `cli-pocket-client-core-wasm` (`crates/client/client-core-wasm`)

`CliPocketClient` public API completed:

- `new()` — opens an `IdbStore` lazily; identity is loaded/created on first `connect`.
- `connect({ endpoint_url, server_public_hex, resume_token_hex? })` — eagerly opens `WsTransport`, builds a `SessionBuilder` over `WsTransport / IdbStore / PerfClock / CryptoRng / WasmSpawner`, and starts the session loop via `spawn_local`. Accepts both snake_case and camelCase keys via serde aliases.
- `next_event()` — borrows the receiver, drains one event, serializes it to a JS plain object whose shape matches Plan G's `events.ts`.
- `create_terminal(params_json)` — JSON in (cols, rows, cwd, cmd, env, scrollback_bytes), forwards to `ClientSession::create_terminal`.
- `send_input(bytes)` / `resize(cols, rows)` / `kill()` — operate on the active `TerminalHandle` returned by `session.terminal().await`. V1 has one active terminal at a time; the optional `terminalId` argument is accepted but routed through the active handle.
- `export_identity()` — returns base64-encoded serialized identity.
- `import_identity(blob)` — decodes base64, calls `ClientIdentity::import_serialized` (which writes to KV), then `load_or_create` refreshes the in-memory cache.
- `close()` — drops the inner `ClientSession` and event receiver, allowing a fresh `connect`.

New module: `pairing.rs`

- `client_pair_with_code(daemon_pairing_url, code)` — wasm-bindgen async function that drives the SPAKE2 client side against the daemon's raw-bytes pairing protocol (see `crates/server/daemon-core/src/server.rs::run_pairing`). Returns `{ server_public_hex, client_public_hex }`. Uses `Spake2Side::start_client` + `ChaCha20Poly1305` (zero nonce, PSK-derived key) to mirror the daemon side exactly.

### `apps/web/` — Vite web app

Files:

- `package.json` — `cli-pocket-web@0.1.0` with `file:` dep on the wasm `pkg/`.
- `tsconfig.json` — ES2022, bundler resolution, `@terminal/*` alias to `webview/terminal/src/*`, `@/*` to `apps/web/src/*`.
- `vite.config.ts` — port 5174, `__CLIENT_KIND__ = "web"`, Tauri externals.
- `index.html`, `public/manifest.webmanifest`, `public/favicon.svg`.
- `src/main.ts` — boot script that lazy-imports `PairingFlow.startWebApp`.
- `src/env.d.ts` — Vite client types + `__CLIENT_KIND__`.
- `src/styles/web.css` — pairing card styles.
- `src/pairing/`:
  - `relayEndpoint.ts` — `ServerSelector` / `SavedServer` types + `validateUrl` / `validateCode`.
  - `PairingView.ts` — DOM mount + form handler.
  - `PairingFlow.ts` — state machine: checks `localStorage` for saved server; if absent, mount pairing UI; on submit call `client_pair_with_code`, persist `SavedServer`, then `launchTerminal` (uses `WebBridge.create()` + `App` from `@terminal`).
- `src/identity/IdentityActions.ts` — `installHashHandlers` listens for `#export` / `#import`, downloads or restores a `.txt` identity blob. Defines a minimal `IdentityClient` interface satisfied structurally by `WebBridge`, so the wasm layer is not coupled to the UI.
- `scripts/build.sh` — POSIX wrapper that builds the wasm `pkg/` then `tsc --noEmit && vite build`. `package.json` `build` chains the same steps cross-platform.

## Deviations from Plan I as written

1. **SPAKE2 wire format.** Plan I text used hypothetical `Frame::ClientPairBegin / HostPairBegin / ClientPairOk` enum variants and a relay-routed pairing flow. The actual daemon protocol is RAW BYTES over a direct WebSocket (no `Frame` wrapping, no relay). `client_pair_with_code` mirrors `run_pairing` in `daemon-core/src/server.rs`. The web client therefore pairs **directly to the daemon's pairing listener** (currently `cli-pocket-daemon pair --code <CODE>` starts a one-shot TCP listener on the configured bind address); pairing-through-relay is a future enhancement.
2. **Return shape of `client_pair_with_code`.** Plan I said it returns `{ server_public_hex, resume_token_hex }`. The daemon never mints a resume token during pairing — it just records the client and returns its own static public key. The actual return is `{ server_public_hex, client_public_hex }`. The web app stores `resume_token_hex: null` initially; a real resume token arrives later via `ClientEvent::Connected { session_id }` once a session is established.
3. **App + WebBridge instantiation.** Plan I's `launchTerminal` skeleton used `new WebBridge(client)` and `startApp(root, bridge)`. The actual surface (per Plan G handoff) is `WebBridge.create()` (static factory that constructs its own wasm client internally) and `new App(host, bridge, "web").start()`. `PairingFlow.ts` uses those.
4. **ConnectConfig keys.** Plan G's `ClientBridge.connect` accepts camelCase (`endpointUrl`, `serverPublicHex`, `resumeTokenHex`). The wasm `connect` accepts both via serde aliases — both work.
5. **I10 (PWA service worker) — skipped.** It was marked optional in Plan I; deferred to a later cycle. The `manifest.webmanifest` is shipped so installability still works in browsers that fall back gracefully.
6. **I11 (manual smoke test) — pending.** Requires a running relay + daemon; documented in Plan I as verification-only.
7. **I2 (`next_event` method) — folded into I1's commit** (`feat(client-core-wasm): CliPocketClient.connect drives Noise via WsTransport`). The codex agent that completed I1 implemented `next_event` in the same change because the test required it.

## Commands

- Native check (already gated in CI):
  - `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown` — passes.
- Local web dev:
  - `cd apps/web && npm install` (consumes the local wasm `pkg/`).
  - `cd apps/web && npm run dev` → `http://localhost:5174/`.
- Local web build:
  - `cd apps/web && npm run build` (chains `wasm-pack build` → `tsc --noEmit` → `vite build`).

## Open questions / followups for Plan J

1. Web client deployment: static `apps/web/dist/`. Plan J publishes it via gh-pages / Netlify / similar. Origin MUST be HTTPS for `import.meta` + WebSocket-to-LAN trickery; LAN daemons reachable from a browser typically require ws:// (mixed-content blocked from https:// origins). Document this constraint in the release docs.
2. Pairing UX assumes daemon is reachable at a user-typed ws:// or wss:// URL. For real-world flows, document how to start `cli-pocket-daemon pair --code <CODE> --bind <addr>:<port>`.
3. The `IdentityActions` `Uint8Array` interface differs from the raw wasm `export_identity` (base64 string). The bridge handles the conversion; if a power user wants to import a base64 string directly, expose a second path or document the file format (currently binary bytes of `ClientIdentity::export_serialized`).
4. Resume token flow: post-pairing, the first `Connected` event carries a `session_id`; persist that as the `resume_token_hex` for the next launch. `PairingFlow.ts` currently stores `null`; wiring this up requires the renderer / bridge to surface `Connected` events back to the PairingFlow layer.

## Validation

All Plan I commits on `main`:
- `ddaaf06` `feat(client-core-wasm): CliPocketClient.connect drives Noise via WsTransport`
- `2a38a76` `feat(client-core-wasm): create_terminal / send_input / resize / kill`
- `abe9831` `feat(client-core-wasm): export/import identity, close session`
- `f73c751` `feat(client-core-wasm): client_pair_with_code (SPAKE2 initiator)`
- `875b9d9` `feat(web): vite app skeleton`
- `cee1b37` `feat(web): pairing flow (SPAKE2 + localStorage server selector)`
- `973fc38` `feat(web): identity export/import via #export and #import hashes`
- `12a24e6` `build(web): wasm-pack then vite build pipeline`

Final compile gate: `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown` passes with no warnings.
