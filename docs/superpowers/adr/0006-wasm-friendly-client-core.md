# 0006. Wasm-friendly client-core via four traits

Date: 2026-05-23
Status: Accepted
Owners: cli-pocket

## Context

The browser client must speak the same `Frame` + Noise XK protocol as the
desktop and mobile clients. The two realistic shapes are:

1. Ship one Rust implementation and compile it to both native and
   `wasm32-unknown-unknown`.
2. Ship two implementations: Rust for native, TypeScript for browser.

(2) doubles the protocol-drift surface — every spec change has to land in
two places — and any subtle difference (frame ordering, resume token
encoding, Noise misuse) becomes a divergence bug that is hard to track
across language boundaries.

## Decision

Ship one Rust crate (`crates/client/client-core`) parameterised over four
small traits:

- `Transport` (`?Send`) — `send`/`recv`/`close` for opaque bytes.
- `Clock` (`?Send`) — `now_ms` + `sleep_ms`.
- `Rng` — `fill(&mut [u8])`.
- `KeyValueStore` (`?Send`) — `get`/`put`/`delete` for the identity blob.

The native side wires these to tokio/std/file. The wasm side
(`crates/client/client-core-wasm`) wires them to `web-sys::WebSocket`,
`Performance.now()`, `Crypto.getRandomValues()`, and IndexedDB. The
`client-core` crate is the single source of truth for the connection state
machine, resume/reconnect, and frame routing.

## Consequences

- Positive: zero protocol-drift surface between platforms — bug fixes in
  resume/reconnect land in one place.
- Positive: the same property tests cover both targets.
- Negative: every API in `client-core` must be `?Send`-friendly (browsers are
  single-threaded). This rules out `tokio::task::JoinHandle<()>` in
  signatures and forces `Rc<RefCell<…>>` rather than `Arc<Mutex<…>>` in the
  wasm side.
- Negative: the wasm bundle includes the full Frame + Noise codec.
- Risk accepted: initial wasm bundle ~250 KB gzipped is acceptable for v1;
  revisit only if it grows beyond 500 KB.
