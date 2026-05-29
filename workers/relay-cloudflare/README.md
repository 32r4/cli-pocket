# relay-cloudflare

Cloudflare Workers + Durable Objects implementation of the cli-pocket relay.

Build and deploy commands:

- `just build-relay` runs `npm --prefix workers/relay-cloudflare run build`
- `just deploy-relay-cloudflare` runs `npm --prefix workers/relay-cloudflare run deploy`

Local relay development command:

- `just dev-relay` runs `cli-pocket-relay` with [`crates/relay/relay-bin/relay.dev.toml`](../../crates/relay/relay-bin/relay.dev.toml)

## Surface

- deployable Worker entrypoint
- Durable Object binding and migration config
- `/health`, `/ws/server`, `/ws/client` routing
- per-server Durable Object sharding via `relay-server:<server_id>`
- `Authorization: Bearer <token>` enforcement on `/ws/server` when `SERVER_AUTH_TOKEN` is configured
- relay caps and admission control via:
  - `MAX_SERVERS`
  - `MAX_PAIRS`
  - `MAX_BYTES_PER_SEC`
  - `MAX_QUEUED_BYTES`
  - `IDLE_SECONDS`
- relay protocol support for:
  - `ServerRegister` / `ServerRegisterOk` / `ServerRegisterErr`
  - `ServerHeartbeat`
  - `ClientConnect`
  - `PairInbound` / `PairOpen` / `PairClose`
  - server->client and client->server byte forwarding

## Relay contract

- `GET /ws/server?server=<server_id>`
- `GET /ws/client?server=<server_id>`
- optional `Authorization: Bearer <token>` on `/ws/server` only
- relay control frames defined in [`crates/shared/proto/src/relay.rs`](../../crates/shared/proto/src/relay.rs)
- opaque byte forwarding via `RelayData::Forward`

Relay responsibilities:

- admit or reject server registration
- allocate and close pairs
- route opaque bytes by `pair_id`
- enforce caps, queue limits, and idle cleanup

Out of scope for relay:

- terminal/session semantics
- Noise payload parsing
- pairing UX
- daemon authorization beyond relay admission
- client feature negotiation beyond the relay wire contract

Evolution rules:

- do not change the v1 routes, query parameters, or existing frame meanings just to fit daemon/client changes
- additive extensions are fine when they do not change existing semantics
- breaking behavior changes should go through a new relay protocol version, not a silent v1 rewrite

Implementation notes:

- behavior is defined against [`crates/relay/relay-core`](../../crates/relay/relay-core/src/lib.rs)
- pair state is not restored from Durable Object storage after hibernation/restart
- caps and queue accounting are Worker-side enforcement, not a byte-for-byte port of the Rust relay behavior
- Worker runtime integration coverage is separate from the Rust relay tests

## Configuration

Static Worker configuration lives in [`wrangler.toml`](./wrangler.toml).

Runtime vars:

- `MAX_SERVERS`
- `MAX_PAIRS`
- `MAX_BYTES_PER_SEC`
- `MAX_QUEUED_BYTES`
- `IDLE_SECONDS`

`SERVER_AUTH_TOKEN` is intentionally not stored in `wrangler.toml`; it is injected at deploy time.

## Deployment

GitHub Actions workflow:

- [`.github/workflows/deploy-relay-cloudflare.yml`](../../.github/workflows/deploy-relay-cloudflare.yml)

Custom domain route:

- `relay.cli-pocket.32r4.asia`

Required GitHub secrets:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

Optional GitHub secret:

- `CLOUDFLARE_RELAY_SERVER_AUTH_TOKEN`
  - only needed when you want `/ws/server` bearer-token enforcement
