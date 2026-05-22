# Handoff - Plan B (Shared contract layer)

Date completed: 2026-05-22
Implementer: Codex

## What was built

### `cli-pocket-proto` (0.1.0)

Public API surface, re-exported from `lib.rs`:

- IDs: `TerminalId`, `StreamId`, `StreamSeq`, `SessionId`, `HostId`, `ClientId`
- Lifecycle: `TerminalCreateParams`, `TerminalInfo`, `ExitInfo`, `KillSignal`
- Errors: `ProtocolError`, `ByeReason`
- Handshake: `Hello`, `HelloOk`, `HelloErr`, `Capabilities`, `ClientKind`, `ResumeToken`, `ResumeAttachment`, `ServerInfo`
- Render: `Snapshot`, `AnchorState`, `SgrAttrs`, `Color`, `TerminalModes`, `MouseMode`, `CharsetState`, `DeltaSlice`
- Wire: `Frame`, `FrameBody`
- Relay: `RelayCtrl`, `RelayData`, `PairId`, `OfferId`, `Endpoint`, `PairCloseReason`, `RELAY_DISC_CTRL`, `RELAY_DISC_DATA`
- Codec: `encode_frame`, `decode_frame`, `encode_relay_ctrl`, `encode_relay_data`, `decode_relay`, `CodecError`, `RelayWire`
- `PROTOCOL_VERSION = 1`

### `cli-pocket-crypto` (0.1.0)

- `Secret<T>` redaction wrapper; `Debug` and `Display` redact, while `Serialize` preserves wrapped values.
- `KeyPair`, `Identity`, `IdentityError`, `KeyBytes32` for X25519 Noise identity material.
- `Identity::generate`, `Identity::load_strict`, `Identity::load`, and `Identity::save` for `host_identity.json`.
- Identity JSON uses `host_id` per the Plan B contract.
- Unix identity save writes a restrictive temporary file, fsyncs it, then renames it into place. Unix strict load enforces mode `0600`; Windows ACL policy is deferred to `daemon-bin`.
- `NoiseInitiator`, `NoiseResponder`, `NoiseSession` implement `Noise_XK_25519_ChaChaPoly_BLAKE2s` with optional PSK at position 2.
- `Spake2Side`, `Spake2Outcome`, `Spake2Error` wrap RustCrypto SPAKE2 over Ed25519.
- `Spake2Outcome` includes both `shared: Vec<u8>` and typed `psk: [u8; 32]` for the B11 Noise PSK handoff.

### `cli-pocket-transport` (0.1.0)

- `Transport` trait with `send`, `recv`, and `close`.
- `TokioWsTransport` native tokio-tungstenite implementation.
- `InMemoryTransport` and `InMemoryTransportPair` test helper for C, D, E, and F.

## Deviations from Plan B / B16

- The workspace `uuid` dependency enables the `js` feature so wasm builds can call `Uuid::now_v7()`.
- `cli-pocket-crypto` depends on workspace `getrandom`, activating the existing B1 `getrandom 0.2/js` feature for wasm RNG through `snow` and `spake2`.
- Local subagent shells did not have `just` available, so manual equivalents were run for the gate components. Those manual checks passed except for a local `cargo-deny` 0.17.0 TOML parse issue; CI and Plan A pin newer `cargo-deny` behavior for that check.
- `Spake2Outcome` includes typed `psk: [u8; 32]` in addition to `shared: Vec<u8>` for the B11 Noise PSK handoff.
- Identity JSON uses `host_id` per spec and exposes `Identity::generate`, `load_strict`, `load`, and `save`; Unix save writes a restrictive temp file then renames it.
- `cli-pocket-proto` still exports `SCAFFOLD_VERSION = 0` so downstream scaffold crates that have not yet migrated to the Plan B shared contract can continue compiling until their own plans replace the placeholder dependency surface.

## Open questions / follow-ups

- The web client's wasm transport is added by Plan F using `web-sys::WebSocket`.
- Frame compression, out of scope in spec Section 2, is not represented in the contract. If a future capability bit adds it, the codec module is the place.
- `chrono` is intentionally not a dependency. `Identity::created_at` uses a hand-rolled RFC3339 formatter. If a downstream plan needs richer time handling, add `time` or `chrono` to `workspace.dependencies` and migrate.
- `cli-pocket-transport` is native-only by design because it depends on `tokio::net::TcpStream`.

## Validation

- Fresh B17 checks:
  - `cargo test --workspace` - passed; workspace tests and doctests passed.
  - `cargo build --target wasm32-unknown-unknown -p cli-pocket-proto` - passed.
  - `cargo build --target wasm32-unknown-unknown -p cli-pocket-crypto` - passed after the workspace `uuid/js` and `getrandom/js` wiring.
- Latest B16 evidence:
  - `cargo clippy --workspace --all-targets -- -D warnings` - passed.
  - `npm --prefix webview/terminal run lint` - passed.
  - `npm --prefix webview/terminal test` - passed.
- `just check` was not run locally because `just` was unavailable in subagent shells. Manual equivalents passed for the available gate components above, except `cargo deny check` was blocked locally by `cargo-deny` 0.17.0 parsing `deny.toml`; CI and Plan A use newer cargo-deny behavior.

## Proto freeze

The tag `proto-v1.0.0-frozen` is a required Plan B gate artifact for downstream Plans C, D, E, and F. It is ready to create at the final Plan B commit after the parent/user approval step. This task did not create the tag; the parent must ask before cutting it per orchestration instructions.

When approved, run:

```bash
git tag -a proto-v1.0.0-frozen -m "Freeze cli-pocket-proto and cli-pocket-crypto for downstream plans C/D/E/F"
git push --tags
```

After this tag, changes to `crates/shared/proto/**` or `crates/shared/crypto/**` require an ADR.
