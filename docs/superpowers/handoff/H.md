# Plan H Handoff: Tauri Desktop + Mobile Apps

Date completed: 2026-05-24
Implementer: Codex orchestrator

## What shipped

### `crates/client/tauri-bindings`

- `TokioWsTransport`, `TokioClock`, `OsRandom`, `FileKvStore` remain the native
  adapters used by Tauri apps.
- `SessionHandle` now exposes command-safe methods for:
  - `connect`
  - `create_terminal`
  - `send_input`
  - `resize`
  - `kill`
  - `shutdown`
- The actor still owns the `!Send` `ClientSession` on a dedicated thread, but
  command handlers can now address the active terminal through
  `send_input/resize/kill`.

### `apps/desktop/src-tauri`

- Tauri command surface now matches Plan G's snake_case aliases:
  - `cli_pocket_connect`
  - `cli_pocket_create_terminal`
  - `cli_pocket_send_input`
  - `cli_pocket_resize`
  - `cli_pocket_kill`
  - `cli_pocket_export_identity`
  - `cli_pocket_import_identity`
  - `cli_pocket_close`
- `connect` builds a native `SessionBuilder` using:
  - `TokioWsTransport`
  - `TokioClock`
  - `OsRandom`
  - `FileKvStore`
- The desktop app emits a single frontend event channel:
  - `cli_pocket:event`
- Event payload JSON now matches `webview/terminal/src/types/events.ts` and
  `webview/terminal/src/types/frame.ts` for:
  - `Connecting`
  - `Connected`
  - `Disconnected`
  - `TerminalCreated`
  - `TerminalOutput`
  - `TerminalExited`
  - `Error`
- Deep links are forwarded as:
  - `cli_pocket:deep_link`

### `apps/mobile/src-tauri`

- Added a host-checkable Tauri mobile crate:
  - `apps/mobile/src-tauri/Cargo.toml`
  - `apps/mobile/src-tauri/tauri.conf.json`
  - `apps/mobile/src-tauri/build.rs`
  - `apps/mobile/src-tauri/src/{lib,commands,event_pump,state,deep_link}.rs`
- The mobile command/event surface mirrors desktop exactly.
- The workspace now includes `apps/mobile/src-tauri`.

## Deviations from plan / current limitations

- The current `client-core` still models one active terminal handle at a time.
  `send_input`, `resize`, and `kill` accept a `terminal_id` because the frontend
  contract requires it, but the implementation validates that it matches the
  active terminal rather than supporting arbitrary concurrent terminal routing.
- `export_identity` / `import_identity` are implemented through
  `tauri::async_runtime::block_on(...)` because Tauri command futures must be
  `Send` while the KV/identity traits in `client-core` are explicitly `?Send`.
- Mobile Android/iOS generated scaffolding under `src-tauri/gen/{android,apple}`
  was not produced here. The Rust crates are host-checkable, but platform init
  still requires `cargo tauri android init` / `cargo tauri ios init` in an
  environment with the relevant SDKs.
- The desktop/mobile configs and code are wired for direct host sessions via
  `cli-pocket-host/v1`; relay-only client flows remain owned by the web app / a
  future native pairing flow.

## Commands

- `cargo check -p cli-pocket-tauri-bindings`
- `cargo check -p cli-pocket-desktop`
- `cargo check -p cli-pocket-mobile`

## Validation

- `cargo check -p cli-pocket-tauri-bindings` - passed.
- `cargo check -p cli-pocket-desktop` - passed.
- `cargo check -p cli-pocket-mobile` - passed.

## Follow-ups for downstream work

- Plan J can now treat mobile as a real workspace package, but CI/release jobs
  must still tolerate missing generated Android/iOS project assets.
- If native apps need relay-mode connections later, add an endpoint/subprotocol
  selector rather than hardcoding direct host transport in `cli_pocket_connect`.
- If multi-terminal native control becomes required, extend `client-core` to
  expose terminal lookup/management beyond the single active handle model.
