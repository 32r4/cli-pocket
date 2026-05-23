# Plan D Handoff: Daemon

Date completed: 2026-05-23
Implementer: cli-pocket orchestrator

## What was built

### `cli-pocket-daemon-core` (`crates/server/daemon-core`)

Library crate exposing the daemon lifecycle, session manager, Noise XK
responder, per-connection state machine, HMAC resume tokens, and a
`notify`-driven client database with revocation propagation.

Public surface (re-exports from `lib.rs`):

- `DaemonConfig`, `DaemonError`, `DaemonResult`, `Daemon`.
- `client_db::{ClientDb, ClientRecord, RevocationSet}` with `add`, `revoke`,
  `is_revoked`, `lookup_by_public`, `watch_revocations`.
- `config::DaemonConfig` (TOML round-trip; platform-default paths).
- `connection::{run_connection_with_handshake, ConnectionDeps}` — the
  post-Noise frame loop. Biased `tokio::select!` on the revocation watch
  channel; if the connection's `client_id` joins the revoked set, the loop
  sends `Frame::body(FrameBody::Bye { reason: ByeReason::Revoked })` and
  closes.
- `handshake::{responder_handshake, AcceptedHandshake}` — Noise XK responder
  driver over any `Transport`.
- `identity_store::load_or_create` (returns `DaemonIdentity { host_id, keypair }`).
- `listener::Listener` + `server::Daemon::{boot, run_pairing, public_key_hex}`.
- `relay_dialer` (skeleton — full host-side forwarder lands as part of the
  relay integration work in Plans F/G).
- `resume::ResumeTokenSecret::{mint, verify}` — HMAC-SHA256 with constant-time
  compare; default 7-day TTL.
- `session::SessionManager` — `create`, `attach`, `kill`, `list`, with a
  background reaper drain.

### `cli-pocket-daemon` (`crates/server/daemon-bin`)

`cli-pocket-daemon` binary built with `clap`. Subcommands:

- `start` — run the daemon listener.
- `pair --label <name> --bind <ip:port> [--code <code>]` — generate (or
  consume) a 6-digit pairing code, accept ONE inbound connection on `bind`,
  complete SPAKE2, append the new client to `clients.json`.
- `pair-key` — print the host's pairing public key (hex).
- `list-clients` — list paired clients from `clients.json`.
- `revoke <client-id>` — append the client to `revoked.json`; the live
  connection (if any) sees `Bye { Revoked }` via the watch channel.
- `regenerate-identity --yes-i-understand-this-breaks-all-clients` —
  regenerate the daemon Noise identity. Invalidates all pairings.
- `print-sample-config` — print a sample TOML config.

## Key types

- `DaemonConfig` (TOML; default path: platform data dir).
- `DaemonIdentity { host_id, keypair }`.
- `ClientDb` with `lookup_by_public`, `add`, `revoke`, `is_revoked`,
  `watch_revocations()` returning `watch::Receiver<RevocationSet>`.
- `SessionManager` with `create`, `attach`, `kill`, `list`, `count`.
- `ResumeTokenSecret` with `mint(client, terminal, now_ms, ttl_ms)` and
  `verify(token, now_ms)`.

## Deviations from Plan D / spec

- The Plan D template imports `cli_pocket_shared_*` crate names; in practice
  the workspace uses `cli-pocket-{proto,crypto,transport,pty}` per Plan B's
  handoff. All D code adopted the shipped names.
- SPAKE2 pairing uses ChaCha20Poly1305 with a zero nonce on the post-SPAKE2
  confirmation exchange, driven directly by the typed `Spake2Outcome::psk`
  rather than a separate HKDF expansion. This is safe because the key is
  single-use (one new key per pairing session) and avoids modifying
  `crates/shared/crypto/**` after the `proto-v1.0.0-frozen` tag.
- The relay dialer (`relay_dialer.rs`) ships as a no-op skeleton. The
  host-side ciphertext forwarder graduates alongside the Plan F/G relay
  integration tests.
- `connection::run_connection_with_handshake` was originally documented to
  watch revocations but the watch arm was not wired in the main frame loop
  when D11 landed. Task D14 added the biased `tokio::select!` on
  `watch_revocations()` so the revocation Bye actually fires.
- `kill(KillSignal)` ignores the signal variant per Plan C's handoff
  (`portable-pty` 0.8 exposes a single kill primitive). Downstream platform
  work can revisit if real signal semantics are needed.

## Open questions for downstream plans

- **Plan F:** which resume-token TTL is right? Currently 7 days, configurable
  in code only. Promote to `DaemonConfig.limits` if demand emerges.
- **Plan E (already shipped):** confirm `RelayCtrl` variant names — daemon's
  `relay_dialer` does not yet import them.

## Commands

- Start: `target/debug/cli-pocket-daemon start --config daemon.toml`
- Pair:  `target/debug/cli-pocket-daemon pair --label phone`
- Revoke: `target/debug/cli-pocket-daemon revoke <client-id>`
- Sample config: `target/debug/cli-pocket-daemon print-sample-config`

## Validation

- `cargo build --workspace` — passed.
- `cargo test --workspace --no-fail-fast` — all daemon-core unit + integration
  tests pass (`pairing_roundtrip`, `revocation_drops`, `client_db`, `config_roundtrip`,
  `identity_store`, `resume_token`, `session_manager`).
- `cargo clippy -p cli-pocket-daemon-core -p cli-pocket-daemon --all-targets -- -D warnings` — clean.
- `./target/debug/cli-pocket-daemon.exe pair --help` — surface matches spec.
