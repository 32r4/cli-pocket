# Plan F Handoff: Client-Core + Wasm

Date completed: 2026-05-23
Implementer: cli-pocket orchestrator

## What shipped

### `cli-pocket-client-core` (`crates/client/client-core`)

Native + wasm library. Compiles to the host target and to
`wasm32-unknown-unknown` with no duplicated logic.

Public surface (re-exports from `lib.rs`):

- `Transport`, `Clock`, `Rng`, `KeyValueStore` — the four `?Send` traits
  parameterising the session.
- `ClientSession`, `SessionBuilder`, `SessionConfig` — the connection
  state machine + builder + immutable config.
- `TerminalHandle` — per-terminal handle wrapping a `SessionCommand` mpsc.
- `ClientEvent` — `Connecting / Connected / Disconnected /
  TerminalCreated / TerminalOutput / TerminalExited / Error`.
- `ClientIdentity` — KV-backed long-lived identity persistence.
- Reconnect with resume token via `SessionConfig::backoff` + the resume
  branch of the state machine.

### `cli-pocket-client-core-wasm` (`crates/client/client-core-wasm`)

`wasm-bindgen` surface exposing `CliPocketClient` to JS:

- Constructor `new()`.
- `connect(config_json)` — eagerly validates `{endpoint_url,
  server_public_hex, resume_token_hex?}`; full Plan I wiring deferred.
- `create_terminal`, `send_input`, `resize`, `kill` — stubs returning
  `JsValue::from_str("not yet implemented")` until Plan I.
- `next_event()` — JS-callable event poll; Plan I rewires to a
  `ReadableStream`-like iterator.
- `export_identity`, `import_identity` — stubs for the Plan I identity flow.

Wasm-side trait implementations:

- `ws_transport.rs` — `web-sys::WebSocket` `Transport` impl.
- `clock_perf.rs` — `Performance.now()` `Clock` impl.
- `rng_crypto.rs` — `Crypto.getRandomValues()` `Rng` impl.
- `kv_idb.rs` — IndexedDB `KeyValueStore` impl.

## Key types

- `Transport` (`?Send`): `async fn send(Vec<u8>)`, `async fn recv() ->
  Option<Vec<u8>>`, `async fn close()`.
- `Clock` (`?Send`): `now_ms() -> u64`, `async fn sleep_ms(u64)`.
- `Rng`: `fill(&mut [u8])`.
- `KeyValueStore` (`?Send`): `get/put/delete` over `&[u8]` keys/values.
- `SessionConfig { endpoint, server_public, resume_token, capabilities, backoff }`.
- `ClientEvent`: as above.

## Deviations

- The wasm `CliPocketClient::connect / send_input / next_event` bodies ship
  as stubs. Plan I fills them in because the JS-side contract is best
  designed against a real consumer (the web app).
- The four traits use `async fn` directly via `async-trait` rather than the
  bare `Future` GATs — the resulting bound is `?Send`-friendly and keeps the
  wasm side single-threaded as intended.
- Reconnect+resume happy path validated by `tests/reconnect_resume.rs`. Web
  reconnect behaviour is exercised by Plan I.
- The plan's example JS surface mentioned an `events()` method returning a
  ReadableStream-like iterator; the v1 surface ships `next_event()` (single
  poll) per the plan skeleton, with the streaming shape promoted in Plan I.

## Open questions for downstream plans

- **Plan G (webview):** the xterm.js renderer expects snapshot+delta frames
  via `ClientEvent::TerminalOutput`; confirm chunk size + lag handling on
  first integration.
- **Plan I (web app):** the wasm identity import/export format needs to be
  designed jointly with the browser UI for the QR/clipboard handoff.

## Commands

- Native build: `cargo build -p cli-pocket-client-core`.
- Wasm build: `cargo build -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown`.
- Native happy-path: `cargo test -p cli-pocket-client-core --test happy_path_native`.
- Reconnect+resume: `cargo test -p cli-pocket-client-core --test reconnect_resume`.

## Validation

- `cargo build --workspace` — passed.
- `cargo build -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown` — passed.
- `cargo test -p cli-pocket-client-core` — passed (happy-path + reconnect).
- `cargo clippy -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown -- -D warnings` — clean.
