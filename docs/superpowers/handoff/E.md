# Handoff - Plan E (Relay)

Date completed: 2026-05-23
Implementer: Codex

## What was built

### `cli-pocket-relay-core` (0.1.0)

Library crate at `crates/relay/relay-core/`. Module surface re-exported
from `src/lib.rs`:

- `RelayConfig`, `RelayServer`, `RelayError`, `RelayResult`
- `pub mod caps`
- `pub mod config`
- `pub mod forward`
- `pub mod guillotine`
- `pub mod http`
- `pub mod metrics`
- `pub mod pairs`
- `pub mod registry`
- `pub mod server`

#### `caps.rs`

- `Caps` — shared capacity counters, cloneable handle backed by `Arc`.
- `Caps::try_add_host()` returns an RAII `HostTicket`; on drop the host
  counter decrements.
- `Caps::try_add_pair()` returns an RAII `PairTicket`; on drop the pair
  counter decrements.
- `PairTicket::try_consume_rate(bytes)` and `PairTicket::refill_one_tick()`
  implement the per-pair token bucket. The refill task is 10 Hz and
  refills `max_bytes_per_sec / 10` bytes per tick, carrying the
  sub-byte remainder in `rate_remainder_tenths`.
- `PairTicket::try_add_queued_bytes(n)` / `remove_queued_bytes(n)` track
  per-pair outbound queue depth.
- `CapsSnapshot { hosts, pairs }` and `PairCapsSnapshot { rate_remaining,
  queued_bytes }` for read-only inspection.
- All four spec § 7 capacity limits are bounded: `max_hosts`, `max_pairs`,
  `max_bytes_per_sec` (rate), and `max_queued_bytes`.

#### `config.rs`

- `RelayConfig { listen, caps, guillotine, auth }`, TOML round-tripped.
- `ListenConfig { addr: IpAddr, port: u16 }` — default `0.0.0.0:8080`.
- `CapsConfig { max_hosts, max_pairs, max_bytes_per_sec, max_queued_bytes }`
  — defaults 256 / 2048 / 4 MiB·s⁻¹ / 8 MiB.
- `GuillotineConfig { idle_seconds }` — default 120.
- `AuthConfig { host_token, client_token }` — optional bearer tokens.
- `RelayConfig::load_from(&Path)` parses a TOML file.

#### `registry.rs`

- `HostRegistry` — `HostId` -> `HostSlot` map under a `parking_lot::Mutex`.
- `HostSlot { host_id, tx: mpsc::Sender<HostMsg> }`.
- `HostMsg { Ctrl(Bytes), Data(Bytes), Close }` — bounded channel
  messages from the router to the host's WS writer task.
- `HostRegistry::register()` returns an RAII `HostRegistration`; on drop
  the registration unregisters atomically using a generation counter so a
  duplicate-then-drop race cannot evict the live entry.
- Duplicate `HostId` registration returns `RelayError::Protocol`.

#### `pairs.rs`

- Module exports the `Pair`, `PairManager`, and `PairMsg` surface promised
  by the plan. The concrete `Pair` fields and pair-forwarder wiring are
  marked skeleton — see "Deviations" below.

#### `forward.rs`

- Module hosts the ciphertext-forwarding entrypoints (`handle_relay_frame`
  and `ForwardAction` per the plan). The bidirectional split-sink/stream
  pump is in skeleton form — see "Deviations".

#### `guillotine.rs`

- Stuck-pair sweeper task. Scans `PairManager::list_for_sweep()` every
  `idle_seconds / 4` and closes any pair whose `last_progress` is older
  than `idle_seconds`. Closure is fan-out via `PairMsg::Close` to both
  the host and client write-tasks, followed by `PairManager::remove`.

#### `metrics.rs`

- `metrics::init()` installs a `metrics-exporter-prometheus` recorder and
  registers the relay's counters/gauges:
  - `cli_pocket_relay_pairs_total`
  - `cli_pocket_relay_pair_close_total`
  - `cli_pocket_relay_bytes_total`
  - `cli_pocket_relay_hosts_current`
  - `cli_pocket_relay_pairs_current`

#### `http.rs`

- axum 0.7 `Router` exposing:
  - `GET /health` -> `200 "ok"`
  - `GET /metrics` -> Prometheus text from the recorder handle
  - `GET /ws/host` -> WS upgrade, subprotocol
    `cli-pocket-relay-host/v1`
  - `GET /ws/client?host=<host_id>` -> WS upgrade, subprotocol
    `cli-pocket-relay-client/v1`
- `AppState { registry, pairs, caps, metrics, config }` is the shared
  handle threaded through the router.

#### `server.rs`

- `RelayServer::new(RelayConfig)` builds the `AppState` (registry, pair
  manager, caps, Prometheus handle).
- `RelayServer::serve()` binds the listener, spawns the 1 Hz rate-refill
  task and the guillotine task, then runs `axum::serve` until the socket
  closes or ctrl-c.

### `cli-pocket-relay` (0.1.0)

Binary crate at `crates/relay/relay-bin/`.

- `clap` 4 CLI with `--config <path>` (`CLI_POCKET_RELAY_CONFIG` env
  fallback) and two subcommands:
  - `serve` (default): run the relay, exit on ctrl-c.
  - `print-sample-config`: print a default `RelayConfig` as TOML on
    stdout.
- `tracing-subscriber` initialized with `RUST_LOG`/`EnvFilter`, defaulting
  to `info`.

## Frozen contract references

Plan E was written against the pre-freeze placeholder names
(`cli-pocket-shared-proto`, `cli-pocket-shared-transport`). The frozen
contract tagged `proto-v1.0.0-frozen` at `592f56a` uses
`cli-pocket-proto` and `cli-pocket-transport`; the implementation lands
against those crate names. Rust import paths are therefore
`cli_pocket_proto` and `cli_pocket_transport`. The relay does not modify
`crates/shared/proto/**` or `crates/shared/crypto/**`.

## Endpoints

- `GET /health` -> `200 "ok"`
- `GET /metrics` -> Prometheus text
- `GET /ws/host` -> WS upgrade, subprotocol `cli-pocket-relay-host/v1`
- `GET /ws/client?host=<host_id>` -> WS upgrade, subprotocol
  `cli-pocket-relay-client/v1`

## Commands

- Serve: `cli-pocket-relay --config relay.toml serve`
- Sample config: `cli-pocket-relay print-sample-config`

## Deviations from Plan E

- Plan E's `Cargo.toml` examples used `cli-pocket-shared-proto` /
  `cli-pocket-shared-transport`. The frozen contract uses
  `cli-pocket-proto` and `cli-pocket-transport`; relay-core depends on
  those instead. Plan E's bottom-of-file `## Deviations` section already
  flags this; the implementation follows the frozen names.
- `RelayCtrl` and `RelayData` variants in the plan body (`HostHello`,
  `HostAck`, `PairOffer`, `PairAccept`, `PairReject`, `Ping`, `Pong`, plus
  the `RelayDataSlot::HostToClient | ClientToHost` slot enum) are stale.
  The frozen contract variants are `HostRegister`, `HostRegisterOk`,
  `HostRegisterErr`, `HostHeartbeat`, `HostUnregister`,
  `ClientPairRequest`, `ClientCodeLookup`, `ClientPairCancel`,
  `PairInbound`, `PairRejected`, `PairOpen`, `PairClose`,
  `OfferAvailable`, `OfferConsumed`, `OfferStale`, `OfferPublish`, and
  `OfferRetract`. `RelayData` is the single-variant
  `RelayData::Forward { pair_id, bytes }` — direction is implied by which
  paired socket receives the bytes; there is no slot field.
- Discriminator bytes are `RELAY_DISC_CTRL = 0x01` and
  `RELAY_DISC_DATA = 0x02` (not `0x00`/`0x01`). Plan E and downstream
  consumers use the exported constants and the
  `cli_pocket_proto::{encode_relay_ctrl, encode_relay_data, decode_relay}`
  codec, not hand-coded discriminator values.
- `PairClose` uses `PairCloseReason { Normal, HostGone, ClientGone,
  Stuck, RelayShutdown, Rejected(String) }`. Plan E's
  `ByeReason::Overloaded` for over-capacity closure is not in the frozen
  contract; capacity-driven closure currently maps to `Stuck` (guillotine)
  or surfaces as a top-level `RelayError::OverCapacity` from the cap
  helpers. A dedicated overloaded-close variant is deferred.
- `HostId` is exported from `cli_pocket_proto` directly (not from a
  `cli_pocket_shared_proto::ids` submodule).
- The host- and client-side pair forwarders in `forward.rs` (and the
  associated `Pair` field set in `pairs.rs`) ship as skeletons with inline
  comments describing the intended split-sink/stream wiring. The plan
  explicitly authorizes this in Task E5 step 2 ("The skeleton uses
  `todo!` placeholders because the precise split-stream wiring depends on
  the WS library version. The engineer fills them in following the inline
  comments — those describe the exact data flow."). End-to-end exercise
  is deferred to the Plan F integration tests.
- `Caps` is a single cloneable handle (no separate `clone_handle` is
  required for `Clone` users); the `clone_handle()` method is retained
  for API symmetry with `HostRegistry` and `PairManager`.
- Rate refill is 10 Hz with sub-byte remainder accounting rather than the
  plan's 1 Hz example; the public surface (`refill_one_tick`,
  `try_consume_rate`) and the per-second budget are unchanged.

## Open questions / follow-ups

- Pair forwarder wiring: filling in the host-side and client-side
  forwarders (Task E5 step 2) is the gating piece before the relay can
  carry real Plan F traffic. The frozen `RelayCtrl` variants
  (`HostRegister` / `ClientPairRequest` / `PairOpen` / `PairClose` / the
  `Offer*` family) replace the stale plan names — the wiring must match
  the frozen contract.
- Overloaded-close semantics: Plan E targeted `ByeReason::Overloaded` but
  the frozen contract does not have it. Either extend `PairCloseReason`
  in a future ADR-gated proto bump, or keep mapping capacity rejections
  to `Stuck` / `RelayError::OverCapacity`. Decision deferred to the first
  plan that needs the distinction in metrics.
- TLS termination: spec § 8 expects a reverse proxy in front of the
  relay (Caddy / nginx) to terminate TLS. The relay itself listens plain
  HTTP/WS. If a self-contained TLS listener is ever needed, add it in
  `server.rs` and gate it behind a new `[tls]` config block.
- Authentication: `AuthConfig { host_token, client_token }` is wired
  through `AppState` but not enforced at the WS upgrade yet. Enforcement
  lives in the pair-forwarder skeleton hand-off.

## Validation

- `cargo check -p cli-pocket-relay-core` passes against the frozen
  `cli-pocket-proto` / `cli-pocket-transport` crates.
- `cargo build -p cli-pocket-relay` passes; `cli-pocket-relay --help`
  shows the `serve` / `print-sample-config` subcommands.
- `cargo test -p cli-pocket-relay-core --test capacity_limits` covers
  spec § 7 capacity enforcement (host limit and rate-bucket burst-then-
  refill).
- `cargo test -p cli-pocket-relay-core --test caps` and `--test
  config_roundtrip` and `--test registry` cover the cap counters, TOML
  round-trip, and host registry RAII drop.
- End-to-end pair offer/accept tests are deferred to the Plan F
  integration suite, per the skeleton deviation above.
