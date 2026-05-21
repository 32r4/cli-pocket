# Cross-Platform Remote Terminal — Design

Status: complete, pending user approval
Date: 2026-05-21
Positioning: OSS, self-hosted. No SaaS, no central telemetry, no managed billing.

## Decision Summary

| Decision | Choice |
|---|---|
| Project positioning | OSS / self-hosted |
| SSH compatibility | Replace SSH with our own protocol |
| Host daemon language | Rust (`tokio` + `portable-pty` + `tokio-tungstenite`) |
| Desktop shell | Tauri |
| Mobile shell | Tauri Mobile |
| Web client | First-class v1 surface, relay-only, `client-core` compiled to wasm |
| Relay | Self-hosted Rust (trait-abstracted; CF DO can be added later) |
| Wire encoding | `postcard` (Rust `serde`), fully binary |
| Crypto | Noise Protocol via the `snow` crate, ChaCha20-Poly1305 |
| Predictive local echo | Not implemented (standard round-trip echo + reconnect snapshot) |
| Scrollback | Daemon-side in-memory ring buffer with configurable cap |
| Daemon storage | JSON files (MVP) |

## Section 1 — System Topology and Component Boundaries

### Top-level Topology

```
┌──────────────┐      Noise+postcard over WSS       ┌──────────────┐
│ Host machine │ ◄────────────────────────────────► │  Relay node  │
│              │                                    │              │
│  ┌────────┐  │                                    │  Rust binary │
│  │ daemon │──┤  (LAN: also accepts direct WSS)    │  (axum +     │
│  │ (Rust) │  │                                    │   tungsten)  │
│  └────────┘  │                                    │              │
│      │       │                                    │  zero-trust: │
│      ▼       │                                    │  forwards    │
│  PTY (portable-pty)                               │  ciphertext  │
│      │                                            │  only        │
│      └─ shell processes                           └──────┬───────┘
└──────────────┘                                           │
                                                           │ 1 host : N clients
                                              ┌────────────┼─────────────┐
                                              ▼                          ▼
                              ┌──────────────────────────┐   ┌──────────────────────┐
                              │  Tauri clients           │   │  Web client          │
                              │  ┌────────────────────┐  │   │  (browser, relay-    │
                              │  │ Desktop            │  │   │   only)              │
                              │  │  (Win/macOS/Linux) │  │   │                      │
                              │  └────────────────────┘  │   │  React + xterm.js +  │
                              │  ┌────────────────────┐  │   │  client-core wasm    │
                              │  │ Mobile (iOS/AOS)   │  │   │                      │
                              │  └────────────────────┘  │   │  identity in         │
                              │  webview: xterm.js       │   │  IndexedDB           │
                              │  shell:   client-core    │   │                      │
                              │           (native Rust)  │   └──────────────────────┘
                              └──────────────────────────┘
```

### Connection Modes

The same daemon simultaneously supports three connection modes. Selection is performed by the client; the daemon is passive and registers with the relay for inbound clients.

1. **Direct LAN** — Tauri client connects directly to the daemon over WSS (same Wi-Fi, Tailscale, or localhost reachable). Web client does **not** use this path.
2. **Relay** — client and daemon both attach to a relay; the relay pairs the two sides and forwards bytes. The web client only ever uses this mode.
3. **Loopback** — desktop Tauri client and daemon on the same machine over `127.0.0.1`. Still uses WS (no special-cased transport).

Tauri clients prefer Direct, fall back to Relay. The web client is relay-only by design — see Section 6 for rationale (browser TLS rules vs daemon self-signed certs).

### Top-level Cargo Workspace

```
cli-pocket/
├── crates/
│   ├── shared/                  # contract layer — everyone depends on these
│   │   ├── proto/               # postcard wire types, version negotiation, Frame enum
│   │   ├── crypto/              # Noise XK wrapper, key derivation, key store
│   │   └── transport/           # WS frame abstraction (server + client side)
│   ├── server/                  # daemon process
│   │   ├── pty/                 # portable-pty wrapper + ring-buffer scrollback
│   │   ├── daemon-core/         # session manager, dispatch, pairing
│   │   └── daemon-bin/          # daemon executable + CLI entrypoint
│   ├── relay/                   # relay process
│   │   ├── relay-core/          # session pairing, byte forwarding, health checks
│   │   └── relay-bin/           # relay executable
│   └── client/                  # client-side shared logic
│       ├── client-core/         # state machine, reconnect, snapshot cache —
│       │                        # generic over Transport / Clock / Rng / Kv traits.
│       │                        # Compiles native (Tauri) and to wasm32 (web).
│       └── client-core-wasm/    # wasm-bindgen wrapper exposing client-core to JS;
│                                # only consumed by apps/web.
├── apps/
│   ├── desktop/                 # Tauri desktop (depends on client-core natively)
│   ├── mobile/                  # Tauri Mobile (depends on client-core natively)
│   └── web/                     # Vite + React app, depends on webview/ + client-core-wasm
├── webview/
│   └── terminal/                # Vite + xterm.js + IPC bridge.
│                                # Shared by Tauri webview AND apps/web.
└── docs/
```

The Cargo workspace `members` glob is `crates/*/*` (the role directories aren't crates themselves, they're just folders).

### Boundary Rules

These boundaries are load-bearing. Violating them will compound costs as the system grows.

- **`proto` is the single contract layer.** Daemon, relay, and client all depend on it. All version negotiation lives here. No `match` arms over protocol versions are allowed in daemon or client modules.
- **`relay-core` does not depend on `proto` semantics.** It sees only opaque ciphertext and session IDs. The relay's responsibility is exactly: "forward bytes between host slot and client slot for a given session." This is the physical guarantee behind the zero-trust property.
- **`pty` is protocol-agnostic.** Its API is `spawn / write_input / read_output / resize / kill / snapshot`, with `Vec<u8>` payloads. Embedding protocol parsing here would couple PTY lifecycle to protocol version churn.
- **`client-core` is wasm-friendly.** No tokio multi-thread, no `mio`, no `std::net` directly — all I/O goes through a `Transport` trait that is implemented twice: once natively for Tauri (`tokio-tungstenite`) and once for wasm (browser `WebSocket`). Time and randomness also go through traits with native + wasm impls. This is the rule that lets one logic layer serve all four clients.
- **`webview/terminal` is a standalone web project.** Tauri desktop, Tauri mobile, and the web app all embed the same built artifact. xterm.js upgrades, hotkey changes, and IME debugging happen in one place.
- **`apps/web` only depends on `client-core-wasm`, never on `client-core` directly.** This forces the wasm boundary to stay narrow and serializable.

## Section 2 — Protocol Wire Format

### Layering

```
┌─────────────────────────────────────────────────────────────┐
│ Application: Frame (postcard-encoded enum)                  │
├─────────────────────────────────────────────────────────────┤
│ Crypto: Noise XK transport (ChaCha20-Poly1305 AEAD)         │
├─────────────────────────────────────────────────────────────┤
│ Transport: WebSocket binary frames (length-delimited)       │
├─────────────────────────────────────────────────────────────┤
│ Network: TLS for relay/Internet; plain TCP for loopback     │
└─────────────────────────────────────────────────────────────┘
```

Each WebSocket binary message carries exactly one Noise transport message. Each Noise plaintext is exactly one `postcard`-encoded `Frame`. No concatenation, no fragmentation at this layer — WebSocket handles framing.

### Single Frame Type

There is **one** wire type. The `paseo` opcode-plus-slot layout is intentionally not adopted: a `u8` slot ceiling would become a protocol-versioning headache the moment a power user opens a 257th terminal. The `postcard` enum tag plus a `u32` stream id removes that ceiling without enlarging the steady-state byte count for typical traffic.

```rust
// crates/proto/src/frame.rs

pub struct Frame {
    pub body: FrameBody,
}

pub enum FrameBody {
    // ---- Connection control ----
    Hello(Hello),
    HelloOk(HelloOk),
    HelloErr(HelloErr),
    Ping { nonce: u32 },
    Pong { nonce: u32 },
    Bye { reason: ByeReason },

    // ---- Terminal lifecycle (request/response, request_id paired) ----
    TerminalCreate     { request_id: u32, params: TerminalCreateParams },
    TerminalCreateOk   { request_id: u32, terminal: TerminalId, stream: StreamId },
    TerminalCreateErr  { request_id: u32, error: ProtocolError },

    TerminalAttach     { request_id: u32, terminal: TerminalId, since: Option<StreamSeq> },
    TerminalAttachOk   { request_id: u32, snapshot: Snapshot, head_seq: StreamSeq, stream: StreamId },
    TerminalAttachErr  { request_id: u32, error: ProtocolError },

    TerminalDetach     { stream: StreamId },
    TerminalKill       { request_id: u32, terminal: TerminalId },
    TerminalKillOk     { request_id: u32 },

    TerminalList       { request_id: u32 },
    TerminalListOk     { request_id: u32, terminals: Vec<TerminalInfo> },

    TerminalExit       { terminal: TerminalId, exit: ExitInfo },

    // ---- Data plane (per terminal stream) ----
    Output { stream: StreamId, seq: StreamSeq, bytes: Bytes },
    Input  { stream: StreamId, bytes: Bytes },
    Resize { stream: StreamId, cols: u16, rows: u16 },

    // ---- Flow control ----
    Window { stream: StreamId, credit: u32 },
}

pub type TerminalId = Uuid;          // stable across reconnects
pub type StreamId   = u32;           // per-connection attachment id
pub type StreamSeq  = u64;           // monotonic per-terminal output seq
pub type Bytes      = Vec<u8>;       // postcard length-prefixed
```

`TerminalId` is stable across reconnects and identifies a long-lived PTY-backed session. `StreamId` is allocated per-attachment and is what data-plane frames reference; the indirection lets a single client attach to one terminal twice (e.g., a follower window) and lets the daemon GC the routing table cleanly when a client goes away.

### Field Conventions

- All integers are `postcard` varints unless byte alignment matters; this keeps `seq`, `request_id`, `nonce` cheap when small.
- `Bytes` payloads are passed through unchanged. The daemon never looks inside terminal output — it just buffers and forwards.
- `Snapshot` carries: terminal dimensions, scrollback bytes, and the cursor/parser state needed to resume xterm.js cleanly. See Section 4 for shape.
- `ProtocolError` is a typed enum (e.g., `UnknownTerminal`, `Unauthorized`, `BackpressureExceeded`, `ProtocolMismatch`), not a free-form string. Strings are reserved for `error: ProtocolError` cases that wrap `Other(String)` for unknown forward-compat errors.

### Version Negotiation

```rust
pub struct Hello {
    pub protocol_min: u32,        // lowest version this peer can speak
    pub protocol_max: u32,        // highest version this peer prefers
    pub capabilities: Capabilities,
    pub client_kind: ClientKind,  // Daemon, Desktop, Mobile, Cli
    pub resume: Option<ResumeToken>,
}

pub struct HelloOk {
    pub protocol: u32,            // chosen version (must be in [min,max] of both)
    pub server_info: ServerInfo,
    pub session_id: SessionId,
    pub resumed: bool,            // true if ResumeToken was honored
}
```

Rules:
- Server picks `min(client.protocol_max, server.protocol_max)`. If that is below either side's `protocol_min`, server returns `HelloErr { reason: ProtocolMismatch }` and closes.
- `Capabilities` is an additive bitfield/struct. New optional features are advertised here; both peers must observe and gate behavior on it. Capabilities never gate things load-bearing for correctness.
- Breaking changes bump `protocol`. Additive changes use capability bits.

### Resume Semantics

Two distinct kinds of "resume" — keep them separate.

**Connection resume** is implicit: when a WebSocket drops, the client opens a new WebSocket and sends `Hello { resume: Some(ResumeToken { session_id, attachments: [(terminal, last_seq), …] }) }`. The daemon either honors it (replaying buffered output for each attached terminal whose `last_seq` is still inside the ring buffer) or rejects it with `HelloErr { reason: ResumeStale }`. On reject, the client falls back to issuing fresh `TerminalAttach` calls and accepting a full snapshot.

**Terminal reattach** is explicit: `TerminalAttach { since: Some(seq) }` requests "everything after `seq`." If `seq` is still in the ring buffer the server returns the delta; otherwise it returns the current snapshot and the client renders from scratch. The `head_seq` in `TerminalAttachOk` tells the client where the post-snapshot output stream resumes.

The two share an underlying mechanism (the daemon's per-terminal output ring buffer plus its monotonic `StreamSeq`) but the connection-level form is opportunistic and avoids re-sending snapshots for short network blips.

### Flow Control

Per-stream credit window. The reader (the side draining `Output`) grants credit:

```
Daemon                                 Client
  |        TerminalAttachOk             |
  |  (initial credit = INITIAL_WINDOW)  |
  | ----------------------------------> |
  |                                     |
  |        Output { seq=1, bytes }      |
  | ----------------------------------> |
  |        Output { seq=2, bytes }      |
  | ----------------------------------> |
  |                                     |
  |             Window { +N }           |
  | <---------------------------------- |
```

- `INITIAL_WINDOW`: 256 KiB (tunable). Credit is in bytes of `Output.bytes`.
- Daemon stops reading from the PTY when the credit hits zero; PTY back-pressures the producing process at the OS level. This avoids unbounded memory growth on a fast producer / slow client.
- Window updates are coalesced: the client sends a `Window` after consuming roughly half the granted credit, not on every byte.
- `Input` is not credit-controlled. Keystrokes are tiny and must never block on flow control — input must always be deliverable.

### What Is Not in the Wire Format

These were considered and explicitly excluded:

- **Predictive echo / SSP-style state sync** — not implemented (see Section 1 decisions). No client-side speculative state, no rollback.
- **Multiplexed control vs data channels** — single frame type carries both. The "JSON control + binary data" split was rejected to avoid maintaining two encoders, two version paths, and two test surfaces. If wire debugging gets painful, a `postcard`-aware dump tool is cheap to write.
- **Compression** — out of scope for v1. Terminal output is highly compressible but per-message compression complicates Noise framing. Revisit if traffic profiling justifies it.

### Versioning Plan

- v1 ships with the `FrameBody` variants above.
- Adding a new variant is a **breaking change** in `postcard` enum encoding only if the new variant is sent by a peer that does not first verify the negotiated version. Rule: never emit a variant introduced after `protocol = N` to a peer that negotiated `protocol = N`.
- Capabilities cover smaller forward-compat needs without bumping `protocol`.

## Section 3 — Connection State Machine

This section covers how the three parties (daemon, relay, client) get from "nothing" to "frames flowing", and how they recover when the network drops. Pairing and identity are referenced here but specified in Section 5.

### Endpoints and URIs

A reachable daemon is described by one or more endpoints. Each endpoint is one of:

```
direct://<host>:<port>           # LAN, Tailscale, mDNS
loopback://<port>                # localhost only
relay://<relay_host>/<host_id>   # relay-mediated
```

The client is given a host descriptor (via QR pairing or saved record) that contains the host's public identity key plus a list of reachable endpoints in preference order. Direct first, relay last.

### Daemon State Machine

```
                    ┌──────────┐
                    │  Init    │
                    └────┬─────┘
                         │  load identity, scrollback caps, config
                         ▼
                    ┌──────────┐
                    │ Listening│  (accept WSS on local port; optionally
                    └────┬─────┘   maintain control socket to relay)
                         │
        ┌────────────────┼────────────────┐
        │                │                │
        ▼                ▼                ▼
  ┌──────────┐    ┌────────────┐    ┌───────────┐
  │ Accepting│    │ RelayCtrl  │    │  Serving  │
  │ direct   │    │ registered │    │  N peers  │
  │ peer     │    │            │    │           │
  └────┬─────┘    └─────┬──────┘    └─────┬─────┘
       │                │                 │
       │ Noise XK       │ relay opens     │
       │ handshake      │ data socket on  │
       │                │ pair request    │
       └────────────────┴─────────────────┘
                         │
                         ▼
                    ┌──────────┐
                    │ HelloOk  │  per-peer session enters Serving
                    └──────────┘
```

The daemon never initiates a connection to a client. It only:

1. Accepts inbound WSS on its local port (LAN/loopback path).
2. Holds a long-lived control connection to each configured relay; on a relay `pair` event, it opens a new data connection to the relay for that specific client.

Multiple peers are served concurrently. Each peer gets its own task; they share the per-terminal state via the session manager.

### Client State Machine

```
              ┌─────────────┐
              │   Idle      │
              └──────┬──────┘
                     │  user picks host
                     ▼
              ┌─────────────┐
              │  Resolving  │  iterate endpoints in preference order
              └──────┬──────┘
                     │  for each endpoint: try, give up after timeout
                     ▼
              ┌─────────────┐
              │ Connecting  │  WSS open + Noise XK initiator
              └──────┬──────┘
                     │  Hello → HelloOk
                     ▼
              ┌─────────────┐
              │  Connected  │ ◄─────────────┐
              └──────┬──────┘               │
                     │ frames flowing       │ resume succeeds,
                     │ ping/pong every Hb   │ replay any deltas
        ┌────────────┼────────────┐         │
        │            │            │         │
   user closes   peer Bye    transport drop │
        │            │            │         │
        ▼            ▼            ▼         │
  ┌──────────┐ ┌──────────┐ ┌─────────────┐ │
  │ Closing  │ │ Closed   │ │ Reconnecting│─┘ (with backoff)
  └──────────┘ └──────────┘ └──────┬──────┘
                                   │ all endpoints exhausted
                                   ▼
                            ┌─────────────┐
                            │ Disconnected│
                            └─────────────┘
```

`Resolving` tries endpoints in order with a per-attempt timeout (default 1.5 s for direct, 4 s for relay). The first to reach `Connected` wins; the rest are cancelled.

`Reconnecting` reuses the host descriptor and goes back through `Resolving`. The client carries its `ResumeToken` (session id plus per-terminal `last_seq`) into the new `Hello`. If the daemon honors it, the client transitions back to `Connected` and replays buffered output without re-rendering snapshots. If not, the client issues fresh `TerminalAttach` calls per visible terminal.

Reconnect backoff: exponential with jitter, base 500 ms, cap 15 s. An explicit user action ("Reconnect now") resets backoff.

### Pairing (First-Time Trust)

Out-of-band trust is established once per host/client pair. Three flows are supported. Pick the one whose OOB channel matches the situation.

**Same-room QR (default when a screen is available)**

1. Daemon shows a QR encoding `{ host_id, host_pubkey, endpoints, pairing_token, ttl }`. `pairing_token` is a one-shot, short-lived (default 120 s) credential.
2. Client scans, presents `pairing_token` in `Hello`. Daemon verifies token, records the client's long-term public key, and emits a `HelloOk` with a permanent `client_id`.
3. Subsequent connections from this client present a Noise static key the daemon already trusts; no token needed.

**Six-digit pair code (when QR is impractical)**

For situations where the host's screen can't be scanned (reading the code aloud, ssh'd shell, accessibility, second device on the same desk without a camera). Six digits has only ~20 bits of entropy, so it cannot be a bearer credential. The code is used as the password input to a PAKE (SPAKE2) so it is *never transmitted in a recoverable form*.

```
Daemon (host)                 Relay (rendezvous)            Client
                                   │
generate code = "493 152"          │
generate offer_id (random)         │
                                   │
  ── OfferPublish { offer_id,      │
       host_pubkey, endpoints,     │
       spake2_M_share, ttl } ──────►
                                   │  store, TTL=90s, attempts=3
                                   │
                                   │   user types "493 152"
                                   │
                                   ◄── ClientCodeLookup { hint }
                                   │
                                   ── OfferAvailable { offer_id,
                                                       host_pubkey,
                                                       endpoints } ──►
                                   │
        SPAKE2 round (code as password) carried in RelayData::Forward       │
        derives session key K iff both used the same code                   │
                                   │
              long-term key exchange under K (AEAD-protected)               │
                                   │
        records each other as trusted; daemon sends OfferRetract            │
```

Concrete rules:

1. Daemon generates a uniform random `pair_code ∈ [000000, 999999]`, displayed grouped as `### ###` in the daemon UI/log.
2. Daemon registers an `OfferPublish` with the configured relay (or, in LAN-only mode, advertises via mDNS). The offer carries the daemon's static public key and the SPAKE2 first message; *not* the code itself.
3. Offer TTL: 90 s. Attempt cap: 3 wrong codes against the same offer; on the third failure the offer is destroyed and a fresh code must be generated. Both bounds are enforced relay-side and daemon-side independently.
4. Client enters the code; SPAKE2 with `pair_code` as the password yields a shared session key K iff both sides used the same code. A wrong code yields uncorrelated keys and the AEAD on the next message fails — neither side learns whether the code was off-by-one or completely wrong, only that it didn't match.
5. Once K is derived, the same long-term-key exchange used in QR pairing runs inside that AEAD-protected channel. After success the daemon stores the client's static Noise key and the offer is destroyed.
6. The 6-digit code is single-use even on success; subsequent reconnects use the static-key trust from step 5.

Why SPAKE2 here and not "send the code through Noise":
- Noise XK requires the client to already know the daemon's static public key. In QR/fingerprint flows the OOB channel (camera, copy-paste) carries that key. Six digits cannot encode a public key, so the public key must come over the relay — and at that moment a MITM relay could substitute its own. SPAKE2 turns the low-entropy code into the binding factor that prevents substitution.
- Off-the-shelf: the Rust `spake2` crate is small, audited, and used by Magic Wormhole. The dependency is justified by closing this gap.

Threat model and limits, written down honestly:
- The OOB channel for the digits must itself be trusted (showing the screen, voice over a known phone call). If an attacker observes the code, they have one of three attempts within 90 s. Don't say the code aloud in a public channel.
- Online brute-force is bounded to 3 tries per offer; resource attacker who keeps generating offers gets nowhere because they don't see the daemon's UI/log to learn the new code.
- This is *not* a remote-pairing-over-the-Internet feature for unknown parties. It's an OOB-anchored shortcut. The spec calls this out; the UI surfaces a one-line warning when generating a code for non-LAN use.

**Manual fingerprint (headless servers)**

For headless servers without a screen and where typing 6 digits on the host is awkward, the daemon writes its public key fingerprint to stdout and to `~/.config/cli-pocket/host-fingerprint`. The client takes the fingerprint via copy-paste or `--fingerprint <hex>` and pins it on first connect (TOFU, but with explicit human verification).

---

Pairing is *not* per-connection. After any of the three flows succeeds, the client has a record `(host_id, host_pubkey, our_keypair)` and uses it forever, until the user revokes.

Revocation: the daemon stores known clients in `clients.json`. Removing an entry causes that client's next `Hello` to fail with `HelloErr { reason: Unauthorized }`. There is no online revocation push — the daemon is the source of truth, and a revoked client cannot connect again.

### Heartbeat and Liveness

- `Ping`/`Pong` every 15 s on an idle connection.
- 3 missed pongs (45 s) → transport considered dead; client transitions to `Reconnecting`, daemon drops the peer task.
- Active data traffic counts as liveness; no separate ping needed if frames are flowing.

The relay tracks its own per-leg liveness (Section 7) and tears down a paired session if either side goes silent past its threshold.

### Connection-Level Backpressure

Section 2 specifies per-stream byte credit. There are also two connection-level limits:

- **Send queue cap per peer**: 4 MiB. If the WebSocket sink cannot drain and the queue exceeds this, the daemon closes the peer with `Bye { reason: Backpressure }`. The client treats this exactly like a transport drop and reconnects with `ResumeToken`. This converts a stuck slow consumer into a clean reconnect rather than unbounded memory growth.
- **Inflight handshakes per relay**: 16. Beyond this the daemon defers new pair events. Protects against pairing-storm exhaustion of file descriptors.

### Multiple Clients on One Host

A single daemon serves N clients concurrently. Two clients can attach to the *same* `TerminalId` — they each get their own `StreamId`. Output is fanned out from the per-terminal ring buffer to all attached `StreamId`s. Input is multiplexed: any attached client can write, and the daemon's PTY sees a single byte stream. This is intentional ("multiple windows on the same shell"), but the desktop UI surfaces a clear indicator when more than one client is attached.

Last-attached-wins for resize: whichever client most recently sent a `Resize` sets the PTY dimensions. A future capability bit can opt clients into shared-resize negotiation; not in v1.

### Failure Modes Summary

| Failure | Detection | Action |
|---|---|---|
| Transport TCP/TLS drop | Socket error | Client `Reconnecting`; daemon drops peer task. |
| Heartbeat timeout | 3 missed pongs | Same as transport drop. |
| Resume token stale | `HelloErr { ResumeStale }` | Client falls back to fresh `TerminalAttach` with snapshot. |
| Auth failure | `HelloErr { Unauthorized }` | Client surfaces re-pair prompt; no retry. |
| Protocol mismatch | `HelloErr { ProtocolMismatch }` | Client surfaces upgrade prompt; no retry. |
| Send queue overflow | Local sink stuck | Peer closed with `Bye { Backpressure }`; client reconnects. |
| Relay unreachable | TCP error on relay endpoint | Try next endpoint; if all relay endpoints fail, surface offline. |
| Daemon unreachable from any endpoint | All endpoints exhausted | Client enters `Disconnected`; user must trigger reconnect. |

## Section 4 — PTY and Scrollback

### PTY Crate

`portable-pty` (from the `wezterm` project) is the PTY abstraction. It handles ConPTY on Windows, openpty on macOS/Linux, and presents a uniform API. Its sub-features in use:

- `PtyPair { master, slave }` from `native_pty_system().openpty(PtySize)`.
- `slave.spawn_command(CommandBuilder)` for spawning the user shell.
- `master.try_clone_reader()` for the output side (`Read`).
- `master.take_writer()` for the input side (`Write`).
- `master.resize(PtySize)` for resize.
- `child.wait()` (from the spawn) for exit detection.

Why not raw `nix::pty` or rolling our own ConPTY: ConPTY is its own beast (handle lifetimes, hidden console window suppression, ANSI translation quirks), and `portable-pty` already absorbs every Windows footgun the wezterm team hit. The dependency cost is justified.

### `pty` Crate API

```rust
// crates/pty/src/lib.rs

pub struct Terminal {
    id: TerminalId,
    cols: u16,
    rows: u16,
    cwd: PathBuf,
    cmd: CommandBuilder,
    // …
}

impl Terminal {
    pub fn spawn(params: TerminalCreateParams) -> Result<Self>;

    /// Non-blocking. Bytes go to PTY master writer.
    pub fn write_input(&self, bytes: &[u8]) -> Result<()>;

    /// Subscribe to output. Each subscriber gets every byte from the
    /// time of subscription onward. The ring buffer plus snapshot is
    /// how late subscribers catch up.
    pub fn subscribe(&self) -> OutputStream;

    /// Snapshot at the current moment. Suitable for rendering a
    /// freshly-attaching client.
    pub fn snapshot(&self) -> Snapshot;

    /// Bytes after the given seq, if still in the ring buffer.
    pub fn since(&self, seq: StreamSeq) -> Option<DeltaSlice>;

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()>;

    pub fn kill(&self, signal: KillSignal) -> Result<()>;

    pub fn wait(&self) -> ExitInfo;
}

pub struct OutputStream { /* tokio::sync::broadcast<Vec<u8>> */ }
pub struct DeltaSlice  { pub bytes: Vec<u8>, pub head_seq: StreamSeq }
```

Note that `Terminal` knows nothing about `Frame`, `StreamId`, attached clients, or the network. `daemon-core` owns those concerns. This is the "PTY is protocol-agnostic" boundary from Section 1.

### Scrollback Ring Buffer

A scrollback is a per-terminal in-memory ring buffer of raw output bytes plus periodic anchor markers.

```
┌─────────────────────────── capacity (default 4 MiB) ───────────────────────────┐
│ ░░░░░░ anchor_0 ── bytes ── anchor_1 ── bytes ── anchor_2 ── bytes ── HEAD     │
└────────────────────────────────────────────────────────────────────────────────┘
   ▲                                                                       ▲
   tail                                                                    head
   (oldest byte still retained)                                            (current StreamSeq)
```

Why bytes and not a parsed grid: a parsed grid (mosh / alacritty_terminal) is the only fully correct way to express "current screen state" for arbitrary terminal programs. It's also a lot of code, doubles every test surface, and ties our daemon to a vt100 implementation we'd have to keep current. The byte-stream-plus-anchors approach gives correct rendering for every case where the *initial* terminal state at the anchor is known, which is what xterm.js needs to resume cleanly.

#### Anchors

An **anchor** is a `(byte_offset, parser_state)` pair recorded at a safe split point. Properties:

- A safe split point is any byte position where the `vte` parser is in the ground state — i.e., no half-consumed CSI/OSC/DCS sequence.
- `parser_state` captures what the client needs to start rendering correctly from that byte: cursor position, current SGR (foreground/background/attrs), character set, modes (DECCKM, autowrap, alt-screen, etc.).
- Anchors are placed every `ANCHOR_INTERVAL` (default 64 KiB) of output, by feeding output through the `vte` parser as it streams to the ring buffer. If no safe split occurs within `2 * ANCHOR_INTERVAL`, the next safe split is taken even if it's farther out.

The parser pass is mandatory and cheap. `vte` is a state machine that runs at hundreds of MB/s; running it inline with PTY reads is invisible against any realistic terminal output rate.

#### Buffer Operations

| Operation | Behavior |
|---|---|
| `push(bytes)` | Append to ring; advance `head_seq`. If capacity exceeded, drop from `tail` up to the **second-oldest anchor**. The oldest anchor is preserved so a "since head_seq - capacity + 1" attach has a starting point. |
| `snapshot()` | Returns `Snapshot { anchor_state, bytes: [oldest_anchor.byte_offset .. head], cols, rows }`. The client feeds `anchor_state` into xterm.js as the initial state, then writes `bytes`. |
| `since(seq)` | If `seq >= tail_seq`, return `DeltaSlice { bytes: [seq .. head], head_seq }`. Otherwise return `None`, signaling the caller to use `snapshot()`. |

There is **no on-disk persistence**. Daemon restart means scrollback is gone; this is the v1 decision. The cost is real — long-running tmux-style sessions don't survive a daemon upgrade — but the alternative (encrypted, capped, GC'd disk store) is its own chapter and not worth it for v1. Section 9 lists this as an explicit follow-up.

#### Capacity

- Default cap: 4 MiB per terminal.
- Configurable per-terminal via `TerminalCreateParams.scrollback_bytes`.
- Hard upper bound: 64 MiB. Above that, daemon refuses creation with `ProtocolError::InvalidParam`.
- Aggregate cap across all terminals: configurable, default 256 MiB. When exceeded, the daemon stops creating new terminals (`ProtocolError::ResourceExhausted`) until existing ones are killed.

### `Snapshot` Wire Type

```rust
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub anchor_state: AnchorState,  // what xterm.js initial state to set
    pub bytes: Bytes,               // replay from anchor to head
    pub head_seq: StreamSeq,
}

pub struct AnchorState {
    pub cursor: (u16, u16),
    pub sgr: SgrAttrs,
    pub modes: TerminalModes,       // DECCKM, autowrap, alt-screen flag, …
    pub charset: CharsetState,
    pub title: Option<String>,
}
```

`AnchorState` is the minimum sufficient set for xterm.js to render `bytes` correctly. It is not "everything xterm.js's parser tracks" — only the subset that affects how the *next* byte renders. New fields require a protocol bump or a capability bit.

### Resize Semantics

- Resize is a control frame, not part of the byte stream.
- On `Resize { cols, rows }` from a client, daemon calls `Terminal::resize(cols, rows)` which calls `master.resize`. This delivers `SIGWINCH` to the foreground process group.
- The new `(cols, rows)` is recorded on the terminal struct and included in subsequent `Snapshot`s.
- Last-attached-wins (Section 3). When two clients are attached and one resizes, the other gets a `Resize` frame on its stream and adjusts its xterm.js viewport. Visual size mismatch is unavoidable when two clients of different sizes attach to the same shell — the spec's stance is "honest letterboxing" on the smaller-viewport client rather than re-flowing.
- Resize during disconnect: if a client reconnects with a different viewport size than it last reported, it sends `Resize` immediately after `TerminalAttach`. The snapshot it just received reflects whatever size was current at snapshot time; xterm.js handles the visual transition.

### Subscribe Mechanics

`Terminal::subscribe()` returns an `OutputStream` backed by `tokio::sync::broadcast`. Properties:

- Each subscriber has its own bounded channel (default 1024 messages, ~16 MiB at typical chunk sizes).
- Lagging subscribers (slow client, slow network) overflow their channel and receive a `Lagged(skipped)` notification.
- On `Lagged`, `daemon-core` does *not* try to catch up by replaying — it sends the subscriber a fresh `Snapshot` and resets the stream to `head_seq`. From the client's perspective this looks identical to a brief reconnect.
- This avoids the "broadcaster blocked by slowest subscriber" problem and keeps the PTY drain fast.

### Exit Handling

- `Child::wait()` runs in a dedicated task per terminal.
- On exit, daemon emits `TerminalExit { terminal, exit }` to all attached streams.
- The terminal is *not* immediately destroyed. Its scrollback remains attached for `EXIT_RETENTION` (default 60 s) so a reconnecting client can still see the final output. After retention elapses, the terminal is removed.
- Explicit `TerminalKill` removes immediately on exit (no retention); the client requesting the kill is signaling intent.

## Section 5 — Crypto

### Library

`snow` (Rust) is the Noise Protocol implementation. It is small, has no `unsafe` outside its `default-resolver` feature, exposes a clean state-machine API, and is what `quinn`, `wireguard-rs`, and other production Rust networking projects use.

For SPAKE2 (used only in 6-digit pair-code flow, Section 3): the `spake2` crate from RustCrypto.

For random: `rand_core::OsRng` everywhere. No PRNG seeding from clocks, no userspace RNG.

### Noise Pattern Choice

**Noise_XK_25519_ChaChaPoly_BLAKE2s** for all post-pairing connections.

Why XK:

- **K (responder static known to initiator)**: the client always knows the daemon's static public key after pairing. This is the protocol-level expression of "client has trusted this host." No pattern that omits this property is acceptable.
- **X (initiator static transmitted, encrypted, then authenticated)**: the daemon learns the client's static key during the handshake. The daemon then checks the key against `clients.json`. If unknown → `Unauthorized`. The X half makes this check authenticated rather than a self-claim.
- Forward secrecy on every session via the ephemeral exchange. Long-term key compromise does not retroactively decrypt prior sessions.

Why not IK, KK, or NK:

| Pattern | Why rejected |
|---|---|
| NK | Initiator has no static key — daemon cannot authenticate the client. Reduces to TOFU on the client side only. |
| KK | Both static keys pre-known to both peers — works, but requires the daemon to advertise its full known-clients set somewhere out-of-band, or accept any key. We want the daemon to gate on its known-clients list, which X gives us cleanly. |
| IK | Initiator static sent in first message, transmitted encrypted to responder static, but with weaker forward-secrecy properties for the initiator's identity than XK if the responder's static key is later compromised. XK delays initiator-static disclosure until after the ephemeral-ephemeral exchange. |

Cipher and hash: ChaCha20-Poly1305 + BLAKE2s. ChaCha is the right pick for ARM and any platform without AES-NI (most mobile devices). BLAKE2s matches the suite Noise specifies for these primitives and is what `snow`'s default resolver provides.

### Handshake Layering

```
1. WebSocket open (TLS terminates at relay or LAN endpoint)
2. Noise XK handshake (3 messages: e, ee+es+s+se, finish)
3. Noise transport mode active
4. First plaintext frame sent through transport: postcard-encoded `Hello`
5. Daemon validates client static key against clients.json
6. Daemon replies HelloOk or HelloErr
```

The TLS layer is for transport hygiene (HTTPS proxies, browser compatibility for any future web client) and to make the relay deployable behind nginx with Let's Encrypt. It is **not** part of the trust model. The trust model is Noise XK end-to-end. A compromised TLS layer (rogue CA, MITM proxy) cannot decrypt or forge frames — Noise sees raw ciphertext blobs from the relay.

For LAN direct connections we still use WSS (self-signed cert is fine — Noise is doing the actual work). Avoiding TLS on LAN was considered and rejected: one transport path is simpler than two, and `rustls` with self-signed accepts adds a dozen lines, not a chapter.

### Identity

**Daemon identity**: a single static Curve25519 keypair, generated on first run, stored at:

- Linux: `$XDG_CONFIG_HOME/cli-pocket/host_identity.json` (default `~/.config/cli-pocket/host_identity.json`)
- macOS: `~/Library/Application Support/cli-pocket/host_identity.json`
- Windows: `%APPDATA%\cli-pocket\host_identity.json`

File contents:

```json
{
  "version": 1,
  "host_id": "01HZ6Y8JKQR2X9V7WAB5C3D4E6",
  "created_at": "2026-05-21T10:30:00Z",
  "static_secret_key": "<base64 32 bytes>",
  "static_public_key": "<base64 32 bytes>"
}
```

File mode `0600` on Unix; `icacls` removes inherited ACEs and grants only the current user on Windows. Daemon refuses to start if mode/ACL is wrong and prints a fix instruction. This is non-negotiable — relaxed identity files are how leaks happen.

**Client identity**: same shape, stored in the per-platform Tauri app config dir. Generated lazily on first pairing attempt.

**Per-host client subkey** *(considered, rejected for v1)*: storing one client keypair per paired host would let revocation of one host not affect others. The cost is a more complex key store and the user-facing question of "are you still the same client to host A after re-pairing host B?" V1 uses one client static key across all hosts; revocation is the daemon side dropping the client's pubkey from `clients.json`.

### `clients.json` (daemon-side known clients)

```json
{
  "version": 1,
  "clients": [
    {
      "client_id": "01HZ6Y8JKQR2XCLIENTABCD12345",
      "static_public_key": "<base64 32 bytes>",
      "label": "ezra-iphone",
      "added_at": "2026-05-21T10:35:00Z",
      "last_seen": "2026-05-21T15:02:11Z",
      "paired_via": "qr"          // qr | code | fingerprint
    }
  ]
}
```

`label` is user-supplied during pairing for display ("Phone", "Work laptop") — the daemon UI lists clients by label. `last_seen` is updated on each successful `Hello`. `paired_via` is informational.

The file is the source of truth. To revoke, edit the file (or use a CLI command) and remove the entry. The daemon picks up changes on the next `Hello` — there's no online push, but there's also no live session to interrupt unless the user wants to.

Active-session revocation: if a client is currently connected when revoked, the daemon also drops that peer task with `Bye { reason: Revoked }`. This requires watching the file for changes (via `notify` crate); on each change, the daemon re-reads `clients.json` and disconnects any active peer whose key is no longer present.

### Pre-Shared Key (PSK) Mode for Self-Hosted Relays

Noise patterns can incorporate a 32-byte PSK as an additional secret. Use case: a self-hosted relay operator wants to gate access to the relay itself, not just the daemon. With `PSK` mode, an attacker who somehow guesses or steals a daemon static key still cannot connect through the operator's relay without the relay's PSK.

This is **opt-in**: by default no PSK is used. A relay operator can configure `relay_psk = "<32 bytes base64>"` and distribute it to allowed daemon operators out-of-band. Daemons configured to use that relay include the PSK in their `snow` builder.

PSK is a relay-local concern, not part of the daemon-client trust model. The daemon-client Noise XK runs as before; the PSK only gates whether the relay accepts the connection.

### Frame-Level AEAD Mechanics

After the handshake, every `Frame` is sent as one Noise transport message:

1. `postcard::to_allocvec(&frame)` → plaintext bytes.
2. `noise.write_message(plaintext, &mut ciphertext)` → ciphertext (plaintext + 16-byte Poly1305 tag).
3. `WebSocket::send(Binary(ciphertext))`.

Receive is the mirror. Noise transport-mode counter is per-direction and managed by `snow`; nonces are never reused. After 2^60 messages (a vastly comfortable bound) the daemon initiates a clean Bye-and-rehandshake. In practice this never triggers in v1.

### Key Rotation

- **Per-session ephemeral** is automatic via Noise.
- **Daemon static key**: rotation is a destructive operation — every paired client must re-pair. v1 does not implement online rotation. The CLI will provide `cli-pocket regenerate-identity` which prints the consequences and requires `--confirm-revoke-all-clients`.
- **Client static key**: same — the user re-pairs with each host. CLI: `cli-pocket-client regenerate-identity`.

A future capability bit can carry a "rotation announcement" frame so a daemon can pre-announce a new public key and let clients update without scanning a new QR. Out of scope for v1.

### Cryptographic Threat Model (One Page)

What is protected:

- Confidentiality and integrity of all post-handshake traffic against any party other than the two paired peers (including a fully malicious relay).
- Forward secrecy: long-term key compromise does not decrypt earlier sessions.
- Mutual authentication: the daemon knows which client connected; the client knows it reached the right host.
- Pairing without QR via 6-digit code is bound to the code via SPAKE2; a wrong-code attempt does not reveal which digits were correct.

What is **not** protected:

- A compromised client device (root access, malware) trivially extracts the client static key. That client identity then has full access to every paired host until revoked. We do not implement device-attestation, hardware-backed keystores, or biometric unlock in v1. Mobile platforms could store the static key in Keychain/Keystore in a future version; v1 stores it in the app data directory with appropriate file mode.
- A compromised daemon machine likewise leaks all session contents to whoever controls the box. This is intrinsic to a "remote terminal to my own machine" product.
- Side-channel attacks against `snow`/`spake2`/`postcard`. We rely on upstream's posture; we do not roll our own primitives.
- Traffic analysis. The relay can see message sizes and timing. Frame padding is not implemented in v1.

What we explicitly avoid:

- No raw key material in logs. `tracing` filters on the `secret` field (a custom `serde::Serialize` impl on key types redacts to `"<redacted>"` outside debug-only feature flags).
- No `--insecure-skip-verify` style escape hatch in `release` builds. Debug builds have one, gated on `cfg(debug_assertions)`, and emit a loud warning on use.

## Section 6 — Client Architecture

There are three client surfaces: Tauri desktop, Tauri mobile, and a browser web app. All three share the same `client-core` Rust crate and the same `webview/terminal` Vite bundle. The wasm boundary plus the I/O trait abstraction is what makes that possible.

### Process Layout (Tauri)

```
┌─────────────────────────── Tauri app (single OS process) ───────────────────────────┐
│                                                                                     │
│  ┌─────────────────────────────────┐         ┌──────────────────────────────────┐   │
│  │ Rust core                       │  IPC    │ Webview (one per window)         │   │
│  │  - client-core state machine    │ ◄─────► │  - React shell (host list,       │   │
│  │  - transport (WSS + Noise)      │  Tauri  │     settings, pairing UI)        │   │
│  │  - reconnect, snapshot cache    │ command │  - xterm.js + addons             │   │
│  │  - pairing flows (QR/code/fp)   │  + event│  - terminal viewport(s)          │   │
│  │  - host store, client identity  │  bus    │                                  │   │
│  └─────────────────────────────────┘         └──────────────────────────────────┘   │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### Process Layout (Web)

```
┌─────────────────────────── Browser tab ────────────────────────────────┐
│                                                                        │
│  ┌──────────────────────┐         ┌──────────────────────────────┐     │
│  │ client-core (wasm)   │   JS    │ React + xterm.js             │     │
│  │  - state machine     │ ◄─────► │  (same Vite bundle as Tauri) │     │
│  │  - Noise via snow    │  bind   │                              │     │
│  │  - postcard          │         │  identity in IndexedDB       │     │
│  │  - WebSocket impl    │         │                              │     │
│  └──────────────────────┘         └──────────────────────────────┘     │
│            │                                                           │
│            ▼                                                           │
│      Browser WebSocket → relay (TLS terminated by relay's nginx)       │
└────────────────────────────────────────────────────────────────────────┘
```

The two layouts present the same `client-core` API to the React app; only the IPC mechanism differs (Tauri commands vs. JS↔wasm function calls).

### `client-core` Crate — Wasm Friendly From Day One

The crate is structured so the same source compiles native (Tauri) and to `wasm32-unknown-unknown` (web). This is a hard constraint, not an aspiration.

Concrete rules:

- **No `tokio` multi-thread runtime.** The single-threaded `tokio` (current-thread executor) is used; the wasm build uses `wasm-bindgen-futures` to drive the same futures on the browser microtask queue. Spawned tasks use `tokio::task::spawn_local`.
- **No raw `std::net` or `mio`.** All network I/O goes through a `Transport` trait with two impls: `tokio_tungstenite` for native, browser `WebSocket` (via `web-sys`) for wasm.
- **Time and randomness through traits.** `Clock` and `Rng` traits with native (`tokio::time`, `OsRng`) and wasm (`Performance.now`, `crypto.getRandomValues`) implementations. No direct `std::time::Instant` outside the trait impls.
- **Persistence through a `KeyValueStore` trait.** Native impl writes JSON files. Wasm impl uses IndexedDB (`idb` crate). One trait, two backends.
- **No filesystem access in the core.** `client-core` itself never names a path.

The crypto crate (`snow`, `spake2`, `chacha20poly1305`) all build to wasm cleanly — they're pure Rust, no `std::fs`, no `tokio`. Confirmed against current upstream releases.

### `client-core` Public API

```rust
// crates/client-core/src/lib.rs

pub struct Client<T: Transport, K: KeyValueStore, C: Clock, R: Rng> {
    // owns: identity, host store, active connections, snapshot caches
}

impl<T, K, C, R> Client<T, K, C, R> { /* generic over the four traits */ }

// Native binding
pub type NativeClient = Client<TokioTransport, FileKvStore, TokioClock, OsRngRng>;

// Wasm binding (in client-core-wasm crate)
pub type WasmClient = Client<BrowserWsTransport, IndexedDbStore, BrowserClock, BrowserRng>;
```

```rust
impl<T, K, C, R> Client<T, K, C, R> {
    pub async fn new(config: ClientConfig) -> Result<Self>;

    // ---- Host store ----
    pub fn list_hosts(&self) -> Vec<HostRecord>;
    pub fn add_host(&self, descriptor: HostDescriptor) -> Result<HostRecord>;
    pub fn remove_host(&self, host_id: HostId) -> Result<()>;

    // ---- Pairing ----
    pub async fn pair_via_qr(&self, qr_payload: &str) -> Result<HostRecord>;
    pub async fn pair_via_code(&self, relay: &str, code: &str) -> Result<HostRecord>;
    pub async fn pair_via_fingerprint(&self, descriptor: HostDescriptor, fingerprint: &str) -> Result<HostRecord>;

    // ---- Connections ----
    pub async fn connect(&self, host_id: HostId) -> Result<ConnectionHandle>;
    pub async fn disconnect(&self, host_id: HostId) -> Result<()>;

    // ---- Per-connection terminal ops ----
    pub async fn list_terminals(&self, conn: &ConnectionHandle) -> Result<Vec<TerminalInfo>>;
    pub async fn create_terminal(&self, conn: &ConnectionHandle, params: TerminalCreateParams) -> Result<Attachment>;
    pub async fn attach_terminal(&self, conn: &ConnectionHandle, terminal: TerminalId) -> Result<Attachment>;
    pub async fn kill_terminal(&self, conn: &ConnectionHandle, terminal: TerminalId) -> Result<()>;

    // ---- Event subscription ----
    pub fn subscribe(&self) -> EventStream;
}

pub struct Attachment {
    pub stream: StreamId,
    pub terminal: TerminalId,
}

pub enum Event {
    HostStoreChanged,
    ConnectionStateChanged { host_id: HostId, state: ConnState },
    TerminalOutput  { stream: StreamId, bytes: Vec<u8> },
    TerminalSnapshot { stream: StreamId, snapshot: Snapshot },
    TerminalResized { stream: StreamId, cols: u16, rows: u16 },
    TerminalExited  { stream: StreamId, exit: ExitInfo },
    PairingProgress { stage: PairingStage },
    Error { context: ErrorContext, error: ClientError },
}
```

### Tauri Command Surface (Tauri Clients Only)

The Rust↔webview boundary is a small, explicit set of commands. New commands require a design note; the surface is meant to stay narrow.

```rust
#[tauri::command] async fn host_list() -> Vec<HostRecord>;
#[tauri::command] async fn host_add(qr: Option<String>, code_input: Option<CodeInput>) -> Result<HostRecord>;
#[tauri::command] async fn host_remove(host_id: HostId) -> Result<()>;

#[tauri::command] async fn connection_open(host_id: HostId) -> Result<ConnectionView>;
#[tauri::command] async fn connection_close(host_id: HostId) -> Result<()>;

#[tauri::command] async fn terminal_list(host_id: HostId) -> Result<Vec<TerminalInfo>>;
#[tauri::command] async fn terminal_create(host_id: HostId, params: TerminalCreateParams) -> Result<AttachmentView>;
#[tauri::command] async fn terminal_attach(host_id: HostId, terminal: TerminalId) -> Result<AttachmentView>;
#[tauri::command] async fn terminal_input(stream: StreamId, bytes: Vec<u8>) -> Result<()>;
#[tauri::command] async fn terminal_resize(stream: StreamId, cols: u16, rows: u16) -> Result<()>;
#[tauri::command] async fn terminal_kill(host_id: HostId, terminal: TerminalId) -> Result<()>;

#[tauri::command] fn diagnostics_export() -> Result<String>;
```

Events go the other direction via Tauri's event system. The webview subscribes to channels:

| Channel | Payload |
|---|---|
| `event://connection_state/<host_id>` | `ConnState` |
| `event://terminal_output/<stream>` | `Vec<u8>` (binary) |
| `event://terminal_snapshot/<stream>` | `Snapshot` |
| `event://terminal_resized/<stream>` | `(cols, rows)` |
| `event://terminal_exited/<stream>` | `ExitInfo` |
| `event://pairing_progress/<flow_id>` | `PairingStage` |
| `event://error` | `{ context, error }` |

Output bytes are pushed as binary payloads, not base64-encoded JSON. Tauri 2 supports binary event payloads natively.

### Wasm Binding Surface (Web Client Only)

`client-core-wasm` exposes the same `Client` operations as `wasm-bindgen` exports. The shape mirrors the Tauri command set so the React app's `ipc/` layer has the same call graph in both clients.

```rust
// crates/client-core-wasm/src/lib.rs

#[wasm_bindgen]
pub struct WebClient(WasmClient);

#[wasm_bindgen]
impl WebClient {
    #[wasm_bindgen(constructor)]
    pub async fn new(config: JsValue) -> Result<WebClient, JsValue>;

    pub async fn host_list(&self) -> JsValue;                  // serialized as JS array
    pub async fn host_add(&self, qr: JsValue, code_input: JsValue) -> Result<JsValue, JsValue>;
    pub async fn host_remove(&self, host_id: String) -> Result<(), JsValue>;

    pub async fn connection_open(&self, host_id: String) -> Result<JsValue, JsValue>;
    pub async fn connection_close(&self, host_id: String) -> Result<(), JsValue>;

    pub async fn terminal_list(&self, host_id: String) -> Result<JsValue, JsValue>;
    pub async fn terminal_create(&self, host_id: String, params: JsValue) -> Result<JsValue, JsValue>;
    pub async fn terminal_attach(&self, host_id: String, terminal: String) -> Result<JsValue, JsValue>;

    /// Input bytes arrive as Uint8Array on the JS side and `Vec<u8>` after wasm-bindgen.
    pub async fn terminal_input(&self, stream: u32, bytes: Vec<u8>) -> Result<(), JsValue>;
    pub async fn terminal_resize(&self, stream: u32, cols: u16, rows: u16) -> Result<(), JsValue>;
    pub async fn terminal_kill(&self, host_id: String, terminal: String) -> Result<(), JsValue>;

    /// Subscribe to events; returns a callback registration handle.
    /// JS provides a function that receives `{ kind, payload }` objects.
    pub fn subscribe(&self, on_event: js_sys::Function) -> SubscribeHandle;
}
```

Output `Vec<u8>` payloads cross the wasm/JS boundary as `Uint8Array`. `wasm-bindgen` handles this without extra copies on modern engines (it uses a shared `WebAssembly.Memory` view).

The React `ipc/` layer has two implementations selected at build time:

```ts
// webview/src/ipc/index.ts
export { default as ipc } from
  import.meta.env.VITE_CLIENT_KIND === "web"
    ? "./ipc-wasm"
    : "./ipc-tauri";
```

Same call signatures on both. Components don't know which transport is underneath.

### Shared Webview Code

The `webview/` Vite project is built **twice** by CI:

- **For Tauri**: with `VITE_CLIENT_KIND=tauri`. Output goes to `webview/dist/tauri/`. Both `apps/desktop/tauri.conf.json` and `apps/mobile/tauri.conf.json` point here.
- **For Web**: with `VITE_CLIENT_KIND=web`. Output goes to `webview/dist/web/`, plus the wasm bundle copied in. `apps/web/index.html` consumes this build.

This is one source tree, one xterm.js setup, one set of UI components, two build outputs. The only meaningful difference at runtime is the `ipc/` import.

### Web Client Specifics

Items that only apply to `apps/web`.

#### Connection Mode

Web client is **relay-only**. The browser's TLS rules forbid silent acceptance of self-signed certs, and we don't want to ask users to "click through scary security warnings" to reach a daemon on `192.168.x.x`. So the web client does not attempt direct LAN connections at all — when a paired host is on the same LAN, the web client still goes through the relay. The latency cost is real (one extra hop) but the UX consistency and security model are worth it.

This is the single most important reason the web client exists as a distinct surface, not just "Tauri without the shell."

#### Identity Persistence

The web client's static Noise key lives in IndexedDB under key `cli-pocket.identity`. Properties:

- Generated on first use, never leaves the browser.
- Cleared when the user clears site data; this destroys the client identity. Re-pairing is required. The pairing UI surfaces a banner the first time after clear: "this browser does not yet know any hosts."
- No cross-browser sync. Each browser is a separate paired client. This is intentional — syncing identity across browsers would require either a central account (which we don't have) or user-managed key export/import (which is its own UX project, deferred to v1.x).
- A "Export identity" button lets a power user save the keypair as an encrypted JSON blob and import it into another browser. The export is encrypted with a user-supplied passphrase using `argon2` + `chacha20poly1305`. Same blob shape on import.

#### Pairing on Web

Three flows from Section 3, with adaptations:

- **QR**: web client uses `getUserMedia` + a wasm QR decoder (`qrcode-decoder`) to scan from the device camera. Falls back to a "paste QR JSON" textbox on devices without a camera or where camera permission is denied.
- **6-digit code**: identical to Tauri. User reads the code from the daemon and types it.
- **Manual fingerprint**: identical. Paste hex into a textbox.

The web client cannot *show* a QR (the daemon does that). It can only consume.

#### xterm.js and Browser Differences

The same xterm.js setup as Tauri — WebGL renderer, fit/unicode/web-links/search addons. Browser-specific notes:

- **Firefox** sometimes lags WebGL renderer features behind Chrome/Edge. Test matrix flags any known issues; fallback is the canvas renderer addon (smaller bundle, slightly slower).
- **Safari** has historically had slow `term.write()` performance on large bursts. We rely on xterm.js's flow control (it batches via `requestAnimationFrame`) which Safari supports correctly as of recent versions.

#### Storage Posture

- Identity: IndexedDB, encrypted at rest under a key derived from a constant per-app salt + IndexedDB. Browsers are not a strong storage boundary, but this prevents trivial extraction by another tab on the same origin.
- Host store: same place. Records are small (a few hundred bytes per host).
- Scrollback: not persisted. Closing the tab loses the in-memory xterm.js scrollback. The daemon's ring buffer is still there on next attach.
- Logs: in-memory only, dumpable via the same `diagnostics_export` flow as Tauri. Browser console output is the operator-friendly view.

#### Hosting

- Web client is served as static files from the project's website (or any static host the user prefers — no backend).
- Self-hosters can serve `apps/web/dist/` from the same nginx that fronts their relay. The build is environment-agnostic; the relay endpoint is configured at runtime via a config endpoint or query param.
- No service worker / PWA in v1. PWA install would unlock offline pairing-form caching but introduces update coordination headaches; deferred to v1.x.

### Input Routing

Input crosses three layers: OS event → webview keyboard handler → xterm.js → IPC. Each layer has rules. Same logic on Tauri and Web; the IPC layer is what differs.

#### Desktop (Tauri or Web in a desktop browser)

- xterm.js owns key handling for terminal-bound keys. Its default `attachCustomKeyEventHandler` is used to capture **app-global** shortcuts (new tab, close pane, command palette) **before** xterm.js maps them. Everything else falls through.
- `Cmd/Ctrl+C` is context-dependent: if there's a selection, it copies; otherwise it emits `^C` to the PTY.
- `Cmd/Ctrl+V` paste: webview gets the clipboard string, asks the user to confirm if it contains newlines, then writes the bytes via the IPC layer. Bracketed paste is honored.
- IME composition: xterm.js fires `compositionstart/update/end`. During composition we render the composing string as a transient overlay; the final committed string is emitted on `compositionend`.

Web-specific notes for desktop browsers:

- `Cmd/Ctrl+W` and `Cmd/Ctrl+T` belong to the browser and we can't intercept them. The shell UI surfaces this in a tooltip on hover. Power users running the web client in a Chrome PWA or Firefox SSB get more interceptable shortcuts.
- Browser window-manager shortcuts always win. We document this rather than fighting it.

#### Mobile (Tauri Mobile)

The mobile virtual keyboard is the hard part. The plan:

- The terminal viewport is a normal scrollable region. Below it sits a **persistent virtual key bar**: `Esc`, `Tab`, `Ctrl`, `Alt`, arrow keys, function row toggle. These are native React (Tauri Mobile renders the shell with React the same as desktop) — no WebView keyboard plugin is required for these keys.
- For text entry, a hidden `<input>` element inside the WebView captures input events. Its `inputmode="text"` triggers the OS keyboard. Submitted/changed text is converted to terminal bytes via the xterm.js input pipeline.
- `Ctrl` and `Alt` from the key bar are sticky modifiers that apply to the next regular keystroke (long-press for "hold").
- Hardware keyboards on iPad/Android tablets bypass the bar entirely.
- Long-press in the terminal viewport opens a context menu (Copy / Paste / Select all / Send special keys).
- Swipe up from the bottom edge inside the terminal area shows the keyboard if hidden; swipe down hides it.

#### Mobile (Web in a phone browser)

Mobile browsers are not the v1 target — Tauri Mobile is. But the web client should not be actively broken on phones:

- Same virtual key bar as Tauri Mobile, rendered inside the browser viewport. Position-fixed at the bottom.
- `visualViewport` API used to detect virtual-keyboard show/hide and reflow the terminal viewport above it.
- No native gesture interception (browser owns swipes). Long-press for context menu still works.
- Performance is noticeably worse than the Tauri Mobile app — surface this in the shell UI ("for the best mobile experience, install the app").

#### Common (all)

- All input bytes go through `terminal_input` as `Vec<u8>`. We never send `String`.
- Modifier-only keypresses are filtered.

### State and Caching

The Rust core (Tauri) or wasm (Web) holds the source of truth. The webview holds a UI mirror that is reset on every `connection_state` change.

- **Snapshot cache**: per-attachment, last received `Snapshot`. Used so a webview window that opens after a connection is already up gets a snapshot immediately on attach without waiting for one from the daemon.
- **Webview-side scrollback**: xterm.js keeps its own scrollback (default 1000 lines, bumped to 10000 via opts). Display-only; durable scrollback is the daemon's.
- **No caching of input**: keystrokes are forwarded immediately.

### Connection State UX

The shell UI surfaces three connection states distinctly:

- **Connected** — solid indicator, full input.
- **Reconnecting** — spinner, **input still accepted**. Keystrokes are queued in `client-core` for up to 5 s; if reconnect succeeds within that window they're flushed. Beyond 5 s the UI surfaces "still trying — your last keystrokes were not delivered" and stops accepting input until reconnect resolves.
- **Disconnected** — banner with a "Reconnect now" button. xterm.js viewport stays visible with the last rendered output, dimmed.

### Tauri Mobile Risk Notes

- Tauri Mobile is in beta as of this writing. The plugin ecosystem is thinner than Expo's. Specific risks:
  - **System WebView quirks for xterm.js**: WKWebView (iOS) and Android System WebView render xterm.js differently from Chromium for ligatures, IME compose layout, and scroll inertia. Known-issues panel surfaces visible deltas.
  - **Background/foreground lifecycle**: iOS suspends the app aggressively. Tested by a snapshot-on-foreground integration test.
  - **Builds**: builds run via `cargo tauri ios build` and `cargo tauri android build`, requiring local Xcode and Android Studio. CI uses self-hosted macOS runners for iOS.
- Fallback if Tauri Mobile becomes a blocker: replace `apps/mobile` with an Expo project that links the same `client-core` via Rust→FFI (uniffi-rs). The crate is intentionally framework-agnostic so this swap is feasible without rewriting protocol or state-machine code. The web client is unaffected by this contingency — it always uses wasm.

## Section 7 — Relay

### Goal and Non-Goal

The relay's job is to forward opaque bytes between a host (daemon) leg and one or more client legs that have agreed to be paired with that host. Anything beyond that is intentionally not the relay's concern.

In scope:

- Accept inbound WebSocket connections on a public TLS endpoint.
- Maintain `host_id → host_socket` registrations.
- Forward bytes between paired legs.
- Hold short-lived `PairOffer` records for the 6-digit pair-code flow (Section 3).
- Per-leg liveness, per-pair backpressure, fair scheduling.

Out of scope:

- Decrypting any traffic. Relay never has Noise keys.
- Knowing what `TerminalId`s, `StreamId`s, or even `Frame` shapes exist. Those live above the Noise layer; the relay sees ciphertext.
- User accounts, billing, telemetry, abuse-mitigation databases. This is a self-hosted OSS relay; operators add what they need.
- Persistence beyond memory. Restart of relay disconnects active pairs cleanly; clients reconnect.

### Wire Protocol (Relay-Specific)

The relay uses its own thin framing layer between WS frame and Noise transport. This is the only place `relay-core` parses bytes.

```rust
// crates/proto/src/relay_frame.rs

pub enum RelayCtrl {
    // host → relay
    HostRegister { host_id: HostId, host_pubkey: PubKey, signature: Signature },
    HostHeartbeat,
    HostUnregister,

    // client → relay
    ClientPairRequest { host_id: HostId, attempt_token: u32 },
    ClientCodeLookup { hint: Bytes },     // for 6-digit pair-code pairing
    ClientPairCancel,

    // relay → host
    PairInbound { pair_id: PairId, attempt_token: u32 },

    // relay → both, after pair is set up
    PairOpen { pair_id: PairId },
    PairClose { pair_id: PairId, reason: PairCloseReason },

    // relay → client (during pair-code rendezvous)
    OfferAvailable { offer_id: OfferId, host_pubkey: PubKey, endpoints: Vec<Endpoint> },
    OfferConsumed,
    OfferStale,

    // host → relay (during pair-code rendezvous)
    OfferPublish { offer_id: OfferId, spake2_M_share: Bytes, host_pubkey: PubKey, endpoints: Vec<Endpoint>, ttl_secs: u32 },
    OfferRetract { offer_id: OfferId },
}

pub enum RelayData {
    Forward { pair_id: PairId, bytes: Bytes },
}
```

Two channel types per WebSocket: control (`RelayCtrl`, `postcard`-encoded) and data (`RelayData`, `postcard`-encoded). The first byte of each WS message is a `0x01` (ctrl) or `0x02` (data) discriminator. After that comes the postcard payload.

`RelayData::Forward` is the only frame that carries Noise ciphertext. The relay does not parse the bytes — it routes by `pair_id`.

### Host Registration

```
1. Daemon connects to relay WSS.
2. Daemon sends RelayCtrl::HostRegister { host_id, host_pubkey, signature }.
   - signature = Ed25519 over (host_id || timestamp || relay_url)
   - host_pubkey is the daemon's Noise static key, also used here as Ed25519
     identity (via the conversion documented in Noise spec).
3. Relay verifies signature, replaces any existing registration for host_id
   (only one daemon per host_id at a time), and acks.
4. Daemon sends RelayCtrl::HostHeartbeat every 20 s.
5. On heartbeat timeout (60 s) or socket drop, relay drops the registration.
```

Host registration is **not authenticated** by the relay against any allowlist by default. The relay is a public meeting point. Authentication-of-pairing happens between daemon and client via Noise XK — the relay does not need to know who the daemon "really is", only that whoever later requests pairing for `host_id` will fail Noise unless they're the right peer.

Self-hosted operators who want to gate registration can configure `relay_psk` (Section 5) and a `host_allowlist` of accepted `host_id`s. Both are off by default.

### Direct Pairing Flow (Existing Trust)

Used after the client and daemon have already paired (any of the three flows in Section 3).

```
Client                            Relay                            Daemon
  │                                 │  HostRegister                   │
  │                                 │ ◄────────────────────────────── │
  │                                 │                                 │
  │  ClientPairRequest{host_id}     │                                 │
  │ ──────────────────────────────► │  PairInbound{pair_id, token}    │
  │                                 │ ──────────────────────────────► │
  │                                 │                                 │
  │  PairOpen{pair_id}              │  PairOpen{pair_id}              │
  │ ◄────────────────────────────── │ ──────────────────────────────► │
  │                                                                   │
  │     RelayData::Forward{pair_id, ciphertext} ◄──relayed──►         │
  │       (Noise XK handshake, then Noise transport frames)           │
  │                                                                   │
```

`pair_id` is allocated by the relay and is opaque to both endpoints. It's only used as the routing key inside the relay.

### Pair-Code Flow (Bootstrapping Trust)

Layered on top of host registration (Section 3 has the cryptographic detail; this is the relay-side mechanics).

- Daemon, after generating the 6-digit code, sends `OfferPublish { offer_id, spake2_M_share, host_pubkey, endpoints, ttl_secs }`. Relay stores this in an offer table keyed by `offer_id`. TTL ≤ 90 s, capped at 90 s by relay regardless of request.
- For code-based pairing the client doesn't yet know `host_id`, so it uses `ClientCodeLookup { hint }` instead of `ClientPairRequest`. `hint` is a short prefix the user might be told ("relay tag" or first 2 digits of `offer_id`) to avoid scanning all offers. If no hint, the relay rate-limits.
- Relay returns `OfferAvailable { offer_id, host_pubkey, endpoints }` for offers matching the hint. Client uses this to seed its SPAKE2 round through the relay (carried as `RelayData::Forward` to a transient `pair_id` the relay allocates between this offer-publishing daemon and this client).
- After SPAKE2 succeeds (off-relay; relay sees only the SPAKE2 messages flowing through), the daemon sends `OfferRetract`. Relay deletes the offer.
- Bounded brute-force: each `offer_id` has at most 3 concurrent or sequential SPAKE2 attempts. After the third failure the relay deletes the offer and responds `OfferStale` to further attempts. The daemon also tracks attempts independently and tears down its side on the third failure.

The relay is a passive carrier here. It does not validate codes; it only enforces TTL and attempt count.

### Slot Allocation and Routing

Inside the relay process, an active pair is:

```rust
struct Pair {
    pair_id: PairId,
    host_socket: WeakConnHandle,
    client_socket: WeakConnHandle,
    created_at: Instant,
    last_activity: Instant,
    inflight_bytes: AtomicU64,
}
```

Map: `pair_id → Pair`. Per-leg map: `conn_id → Vec<pair_id>` so a leg disconnect can clean up all pairs that depended on it.

A single client WebSocket connection can carry multiple pairs (e.g., a client paired with two different daemons via the same relay). The `pair_id` discriminator inside `RelayData::Forward` is what the relay routes on; the WS connection is just transport.

### Backpressure

Per-pair byte budget and per-pair pending queue. The pair's two legs can have asymmetric capacity (a slow mobile client paired with a fast daemon).

Rules:

- Each direction (host→client, client→host) has an independent send buffer of up to `PAIR_BUFFER_BYTES` (default 1 MiB).
- If the slow side's WebSocket sink can't keep up and the buffer fills, the relay applies WS-level backpressure to the *fast* side: it stops reading from the fast side's socket until the slow side drains below half-full.
- TCP/WS flow control then propagates back to the daemon or client, which sees a slow `WebSocket::send` and applies its own per-stream credit (Section 2). The chain is:

```
PTY → daemon stream credit → daemon WS send →
relay buffer (slow if downstream slow) → client WS recv →
client stream credit → xterm.js
```

- If a buffer stays full for `PAIR_STUCK_TIMEOUT` (default 30 s), the relay closes the pair with `PairClose { reason: Stuck }`. Both legs see `PairClose` and tear down the Noise session; the client treats it like a transport drop (Section 3).

This deliberately does not try to be smarter. The OS TCP/WS stack already implements correct backpressure; the relay just doesn't get in its way.

### Liveness

Per leg:

- WebSocket pings every 20 s from the relay.
- 3 missed pongs (60 s) → drop the leg, close all pairs that included it.
- Active pairs reset both legs' liveness counters on data flow.

Per pair:

- Idle pairs (no `RelayData::Forward` in either direction) live for `PAIR_IDLE_TIMEOUT` (default 30 min), after which they're closed. Active terminal sessions easily emit a heartbeat at the daemon-client layer; truly idle pairs are closed to free relay memory.

### Fairness and DoS

- Per source IP: max 64 concurrent connections, max 16 host registrations. New attempts beyond the limit get `429`-equivalent close codes.
- Per host registration: max 32 simultaneous paired clients. Beyond that, `ClientPairRequest` returns `RelayCtrl::PairInbound`-rejected.
- Per relay process: `MAX_PAIRS` (default 4096) and `MAX_REGISTRATIONS` (default 1024). Above these, new registrations and pair requests are rejected.
- All limits are configurable via env or config file. The defaults aim at a small VPS (1 vCPU, 1 GB RAM) being able to host a few hundred users comfortably.

These are not anti-abuse, they're capacity guards. A relay operator who needs real DoS mitigation puts the relay behind Cloudflare or a similar L7 layer.

### Deployment

Target shape: a single Rust binary, 5–10 MiB, depending only on system libc. Configuration via env vars or a TOML file:

```toml
# /etc/cli-pocket-relay/config.toml
listen = "127.0.0.1:8443"
external_origin = "wss://relay.example.com"

# Optional gating
relay_psk = ""                  # base64 32 bytes; empty disables PSK gating
host_allowlist_path = ""        # newline-delimited host_ids; empty disables

# Capacity
max_pairs = 4096
max_registrations = 1024
pair_buffer_bytes = 1048576

# Logging
log_level = "info"
log_format = "json"             # json | pretty
```

Reverse proxy: relay listens on plain HTTP/WS on `listen`. nginx/caddy terminates TLS in front. Same shape as paseo's documented self-hosted relay (their docs are reused; nginx config block in Section 1 of the paseo README applies almost verbatim — credit them, don't fork their copy).

Systemd unit, container image (Docker + Compose example), and Nix flake are all shipped in the repo. None of these are exotic; the value is in shipping all three so users pick the one that matches their infra without negotiating with a `Dockerfile` themselves.

### Operator Observability

Self-hosted operator needs visibility into capacity and health, but **not** into terminal contents.

- `/metrics` Prometheus endpoint:
  - `cli_pocket_relay_registrations_total` (gauge)
  - `cli_pocket_relay_pairs_total` (gauge)
  - `cli_pocket_relay_bytes_forwarded_total{direction}` (counter)
  - `cli_pocket_relay_pair_stuck_drops_total` (counter)
  - `cli_pocket_relay_handshake_failures_total{reason}` (counter)
- `/health` returns 200 if the listener is alive and below 90 % of `MAX_PAIRS`.
- Structured JSON logs include `host_id` (last 8 chars), `pair_id`, byte counts, durations. They never include payload bytes.

### What the Relay Cannot Do

Written out explicitly so future contributors don't try:

- It cannot resume sessions across its own restart. Pairs are in-memory; restart drops them. Client-side reconnect logic is what makes this user-invisible.
- It cannot prove to the daemon which client connected. Authentication is between daemon and client via Noise XK. The relay just hands them ciphertext.
- It cannot enforce per-user quotas without state outside the relay (no user accounts in scope).
- It cannot decrypt logs, replay sessions, or do session forensics. By design.

### Future: Cloudflare Durable Objects Adapter

Already-mentioned (Section 1, Relay decision). The trait that `relay-core` exposes — roughly `RelayBackend` with `register`, `pair`, `forward_to`, `close` methods — is implementable on top of CF DO with WebSocket hibernation. That adapter is its own crate (`crates/relay-cf-adapter`), out of scope for v1, listed as future work in Section 9.

## Section 8 — Build, Packaging, Signing, Release

### Build Topology

```
                        ┌──── crates/* ──── cargo build ──────────► daemon-bin, relay-bin
Source repo (monorepo) ─┤
                        ├──── crates/client-core-wasm
                        │       └─ wasm-pack build ────────────────► pkg/ (.wasm + .js bindings)
                        │
                        ├──── webview/    ── npm + vite (×2) ──────► dist/tauri/  (VITE_CLIENT_KIND=tauri)
                        │                                            dist/web/    (VITE_CLIENT_KIND=web, embeds wasm)
                        │
                        ├──── apps/desktop, apps/mobile
                        │       └─ cargo tauri build ───────────────► installers (embed dist/tauri/)
                        │
                        └──── apps/web
                                └─ vite build ─────────────────────► static site (consumes dist/web/)
```

Four independent build outputs:

1. **Server-side binaries**: `daemon-bin`, `relay-bin`. Plain `cargo build --release`.
2. **Wasm bundle**: `client-core-wasm`. `wasm-pack build --target web` produces a `.wasm` file plus generated JS bindings.
3. **Webview bundle, two flavors**: `webview/dist/tauri/` and `webview/dist/web/`. Same source, two `VITE_CLIENT_KIND` values, two `ipc/` impls. The web flavor copies the wasm bundle into its assets.
4. **Tauri apps and Web app**: `apps/desktop`, `apps/mobile`, `apps/web`. Tauri apps embed `dist/tauri/`; the web app serves `dist/web/`.

### Build Tooling

- **Rust toolchain**: `rust-toolchain.toml` pins to a specific stable. Updated on a quarterly cadence.
- **Node toolchain**: `.nvmrc` pins to LTS. Webview only.
- **Cargo workspace**: top-level `Cargo.toml` defines `[workspace]` with all crates under `crates/`. App crates (`apps/desktop`, `apps/mobile`) are also in the workspace so they share `Cargo.lock`.
- **Just**: a `justfile` at repo root provides developer entry points: `just build`, `just dev-daemon`, `just dev-desktop`, `just check`, `just test`, `just dist`. Just is a hard dependency for the developer experience but not for users.
- **No `make`** (Windows hostile), **no `npm` for top-level orchestration** (we're a Rust-first project; npm is scoped to `webview/`).

### Per-Target Build

| Target | Command (developer) | Output |
|---|---|---|
| daemon (any host OS) | `just build-daemon` | `target/release/cli-pocket-daemon{,.exe}` |
| relay | `just build-relay` | `target/release/cli-pocket-relay{,.exe}` |
| wasm | `just build-wasm` | `crates/client-core-wasm/pkg/` |
| webview (tauri) | `just build-webview-tauri` | `webview/dist/tauri/` |
| webview (web) | `just build-webview-web` | `webview/dist/web/` (includes wasm) |
| desktop (current OS) | `just build-desktop` | `apps/desktop/target/release/bundle/...` |
| mobile (iOS) | `just build-ios` | `.ipa` under `apps/mobile/target/...` |
| mobile (Android) | `just build-android` | `.apk` and `.aab` under `apps/mobile/target/...` |
| web app | `just build-web` | `apps/web/dist/` (deployable static site) |

Cross-compilation:

- daemon and relay cross-compile for `aarch64-unknown-linux-musl`, `x86_64-unknown-linux-musl`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`. `cross` (the cargo-cross project) handles Linux musl from any host. Apple targets require macOS runners. Windows targets cross-compile from Linux via `cargo-xwin`.
- desktop apps build on the matching host OS (Tauri does not cross-compile installers).
- iOS builds require macOS + Xcode CLT.
- Android builds require Android SDK + NDK; can run on any host OS.

### CI

GitHub Actions, with self-hosted macOS runners only for iOS. Everything else uses GitHub-hosted runners.

```
.github/workflows/
├── ci.yml          # PR gate: typecheck, clippy, test, webview lint, on x86_64-linux only
├── release.yml     # tag push: cross-compile all, sign, attach to GitHub release
└── docs.yml        # build and deploy mdBook docs site on tag push
```

CI gates (must pass before merge):

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo deny check` (license, advisory database, banned-source list)
- `cargo build --target wasm32-unknown-unknown -p client-core-wasm` (catches non-wasm-friendly code in `client-core`)
- `npm --prefix webview run lint`
- `npm --prefix webview run test`
- `npm --prefix webview run build:tauri` (catches Vite/TS errors)
- `npm --prefix webview run build:web`
- `npm --prefix apps/web run build`

The release workflow runs on tag push (`v*.*.*`) and produces:

- daemon and relay binaries for the cross-compile matrix above, each in a `.tar.gz` (Unix) or `.zip` (Windows), with a `SHA256SUMS` file alongside.
- desktop installers per platform (`.dmg`, `.msi`, `.AppImage`, `.deb`).
- mobile artifacts (`.ipa`, `.aab`).
- web app `cli-pocket-web-vX.Y.Z.tar.gz` containing static `apps/web/dist/` for self-hosters; also auto-deployed to `web.cli-pocket.<domain>` for users without infra.
- A signed `cli-pocket-vX.Y.Z.tar.gz` source archive.

### Signing

The OSS positioning makes "we don't have $99/yr Apple Developer ID" a real possibility. The release pipeline assumes commodity-grade signing only.

| Artifact | Signing | Verification by user |
|---|---|---|
| daemon, relay tarballs | `minisign` signature alongside, public key in repo + on website | `minisign -V -p cli-pocket.pub -m foo.tar.gz` |
| `SHA256SUMS` | `minisign` signed | same |
| Source archive | `minisign` + git tag GPG-signed (release manager's key) | `git verify-tag` and `minisign` |
| macOS desktop `.dmg` | Apple Developer ID *if available*; otherwise unsigned with a documented `xattr -d com.apple.quarantine` step | Gatekeeper or manual override |
| Windows desktop `.msi` | Authenticode *if available*; otherwise unsigned, with notes on SmartScreen warning | SmartScreen or manual override |
| Linux desktop `.AppImage`/`.deb` | `minisign` alongside, plus optional GPG signature on `.deb` | per-distro |
| Web app `.tar.gz` | `minisign` alongside | `minisign -V` |
| iOS `.ipa` | Apple Developer required (no alternative on iOS) — released only when an org has the cert; otherwise sideload-only via TestFlight | TestFlight or sideload |
| Android `.apk`/`.aab` | Self-signed release key, fingerprint published in repo. Play Store optional. | Per-app fingerprint |

`minisign` (jedisct1) is the lowest-friction choice for OSS releases — it's what Bitcoin Core, Algorand, and many others use. The public key sits in the repo and on the website; users `cargo install minisign` and verify in 10 seconds.

The signing private key never enters CI directly. Release flow:

1. Tag push triggers CI build of all artifacts.
2. CI uploads artifacts to a draft GitHub release.
3. Release manager downloads `SHA256SUMS` and the artifacts list, runs `minisign -S` locally with the offline-stored secret key, uploads the signature(s) to the release, and publishes.

For users who want unattended verification, the website serves the public key over HTTPS at a stable URL. Pinning is documented but not enforced.

### Auto-update Posture

Explicitly **no built-in auto-update in v1**. Reasons:

- OSS positioning: we don't operate a signed update server; building one well (rollback, channels, signature pinning, code-signing differences across OS) is a chapter on its own.
- Tauri's bundled updater works, but is tied to a config we'd then have to host and key-pin. For an OSS project, that's a long-term maintenance burden out of proportion to the value at v1.
- The daemon is the long-running process most likely to want auto-update. For v1, daemon updates require a manual `cli-pocket-daemon update` command that downloads the current release tarball, verifies its `minisign` signature, and replaces the binary. The user runs it; nothing happens silently.

This is called out in the user-facing docs: "this is an OSS, self-hosted product; you update it like other CLI tools."

A future commercial fork or a plugin can add auto-update without touching v1 internals.

### Versioning

- **SemVer at the project level** (`v1.2.3`). User-facing.
- **Protocol version** (Section 2) is a separate `u32`. Bumped on breaking wire changes only. Multiple project releases can share a protocol version.
- **`Capabilities` bits** for additive protocol features. No version bump.
- The release notes for every release include "protocol version supported" and "capabilities added." This is the single place a user can check whether two of their installs talk to each other.

### Reproducible Builds

Goal: any developer with the same toolchain and source can produce byte-identical Rust binaries. Bit-identical webview bundles are harder (timestamps in archives, JIT-cache differences across npm versions) and not chased.

Concrete measures:

- `Cargo.lock` checked in. `--locked` on all CI builds and release builds.
- `RUSTFLAGS="-C target-cpu=generic -C codegen-units=1"` on release builds for the daemon and relay binaries (small slowdown, big determinism win).
- `SOURCE_DATE_EPOCH` set from the git commit timestamp for archive timestamps.
- `cargo-vet` (or `cargo-crev`) tracked in repo for supply-chain vetting on dependency upgrades.

Reproducibility is a goal, not a guarantee. If an external researcher reports a divergent build, that's a bug we fix. We do not invest in `repro-env`-style hermetic infra at v1.

### Package Distribution Channels

- **GitHub Releases**: primary distribution. All artifacts plus `minisign` signatures.
- **Homebrew tap**: `32r4/cli-pocket` (or similar). Daemon, relay, CLI. Desktop app available as a `cask`. The formula installs `cli-pocket-daemon` to `/usr/local/bin` and a launchd plist for auto-start.
- **Linux packages**: `.deb` for Debian/Ubuntu, `.rpm` for Fedora-family, AUR for Arch, `nix flake` for Nix users. Not all of these maintained equally; the repo's release notes call out which are official and which are community.
- **Windows**: `.msi` installer + Winget manifest. Chocolatey if a contributor adopts it; not officially maintained at v1.
- **Web**: `apps/web/dist/` in the release tarball, plus an auto-deployed canonical instance for users who don't self-host. Deployment target is a static host (Cloudflare Pages, GitHub Pages, or any nginx) — chosen by the maintainer.
- **iOS**: TestFlight while Tauri Mobile remains beta; App Store only after stability is proven.
- **Android**: GitHub release `.apk` and Play Store. F-Droid eventually (their inclusion process is its own thing).

### Repository Layout for Releases

```
.github/workflows/
release/
├── minisign.pub
├── checksum.sh                # produces SHA256SUMS deterministically
├── package-deb.sh
├── package-rpm.sh
└── notarize-macos.sh          # only used when an Apple cert is available
```

The `release/` directory holds packaging scripts, not artifacts. Artifacts live only in GitHub releases and any user's local cache.

## Section 9 — Testing and Observability

### Test Pyramid

```
                        ┌─────────────────────┐
                        │  Manual UX passes   │  rare; before each release
                        └─────────────────────┘
                  ┌──────────────────────────────┐
                  │ End-to-end real-PTY tests    │  ~20 cases
                  └──────────────────────────────┘
            ┌──────────────────────────────────────┐
            │ Integration: in-process daemon+client│  ~100 cases
            └──────────────────────────────────────┘
       ┌────────────────────────────────────────────────┐
       │ Per-crate unit tests + property tests          │  hundreds
       └────────────────────────────────────────────────┘
```

The shape is dictated by what is cheap to test versus what catches the bugs that matter.

### Per-Crate Unit and Property Tests

- **`proto`**: Property tests with `proptest` for round-trip encoding/decoding of every `Frame` variant. Generators biased toward edge cases (empty `Bytes`, max varint, unicode in titles).
- **`crypto`**: Known-answer tests against Noise XK test vectors. Negative tests for tampering: flip a byte in ciphertext, expect Poly1305 failure. SPAKE2 round-trip with known passwords; mismatched passwords yield uncorrelated keys.
- **`pty`**: Spawn `cat` / `printf` / a small test shim, write input, read output, assert on bytes. These tests are platform-aware (Windows ConPTY behaves differently from openpty); per-OS test guards via `#[cfg(target_os = ...)]`.
- **`pty` ring buffer**: property tests on `(cap, push pattern, since(seq))`. Invariants: `since(head_seq) == empty`, `since(seq < tail_seq) == None`, `snapshot ++ output == snapshot' for any seq < head_seq` (for some equivalence over anchor reset).
- **`transport`**: WebSocket framing wrapper tested against a tungstenite mock; malformed messages produce typed errors, not panics.
- **`relay-core`**: Pair allocation, slot routing, capacity limits — pure-state-machine tests, no real sockets.
- **`client-core`**: State machine tests with a fake transport. Drive `Connecting → Connected → Reconnecting → Connected` with `ResumeToken`, assert that the right `TerminalAttach` calls or replays happen.

### Integration: In-Process Daemon + Client

These tests link `daemon-core` and `client-core` in the same process, connect them via an in-memory duplex channel that masquerades as a WebSocket, and exercise the protocol end-to-end without real sockets, real TLS, or real PTYs (a `pty::TestTerminal` mock substitutes).

Cases that justify integration tests rather than unit tests:

- Full handshake: pair, then connect, then `TerminalCreate`, then exchange data, then graceful close.
- Reconnect with `ResumeToken`: drop the duplex midway, reconnect, assert no rendering regression and that the resumed `head_seq` matches.
- Snapshot fallback: arrange for `since(seq)` to fall outside the ring buffer, assert client gets `Snapshot` and renders correctly.
- Multi-client: two clients on the same `TerminalId`, one types, both observe the output. One disconnects; the other keeps streaming.
- Backpressure: slow consumer client; daemon's send queue saturates at 4 MiB; daemon emits `Bye { Backpressure }`; client reconnects with resume.
- Lagged subscriber: fast PTY producer + slow client; client receives a snapshot reset rather than blocking the PTY drain.
- Auth: unknown client static key → `HelloErr { Unauthorized }` and connection close. Revoked-while-active: drop `clients.json` entry, file watcher fires, active peer dropped with `Bye { Revoked }`.

These run on `tokio::test` with `tokio::time::pause()` for any test that depends on heartbeats or timeouts. No wall-clock dependence in the suite.

### End-to-End: Real PTY, Real Sockets

A smaller suite (~20 cases) that spawns the actual daemon binary and connects via `client-core` over a real WebSocket on `127.0.0.1`. These catch:

- ConPTY/openpty integration bugs that the mock doesn't.
- TLS handshake regressions.
- Process-spawn lifetime: daemon shutdown closes child shells.
- Signal handling: `SIGINT` to daemon flushes scrollback to clients, then exits cleanly.
- Resource leaks: 1000 create/kill cycles on the same daemon, assert RSS stays bounded.

Run on every CI run that's gated by OS (Linux on every PR, macOS on PR merge, Windows on PR merge — to keep PR latency low).

### Cross-Platform Coverage

The test matrix on CI:

| Test scope | Linux x86_64 | macOS x86_64 | macOS arm64 | Windows x86_64 |
|---|---|---|---|---|
| Per-crate unit | every PR | nightly | nightly | nightly |
| Integration | every PR | every PR | nightly | every PR |
| End-to-end | every PR | every PR | nightly | nightly |
| Webview unit (vitest) | every PR | — | — | — |
| Webview e2e (Playwright, tauri build) | nightly | nightly | — | — |
| Web app e2e (Playwright, all browsers) | every PR (Chromium), nightly (Firefox, WebKit) | — | — | — |
| Wasm unit (`wasm-bindgen-test`) | every PR | — | — | — |
| Tauri desktop smoke | nightly | nightly | nightly | nightly |
| Tauri mobile smoke | manual | nightly (iOS sim) | manual | manual (Android emu) |

PR latency stays under 15 minutes; nightly catches the long tail.

### Webview Tests

- **Vitest** for `webview/src/ipc/**` and pure logic.
- **Playwright** for end-to-end UI: pair via mock daemon (a small Node script that speaks the `proto` over a WebSocket using a Noise mock), open a terminal, type, observe output. The mock daemon avoids needing a Rust binary in the webview test pipeline.
- Visual regression for the shell UI (host list, pairing wizard) via Playwright screenshots. Tolerated diff: 1 px / 0.1 % per region.
- xterm.js rendering correctness is not separately tested by us — we trust upstream. We do test that `AnchorState` → xterm.js prelude round-trips correctly via a snapshot test that captures the rendered cell grid after replay.

### Web App Tests

The web app gets its own Playwright suite that runs against all three engines (Chromium, Firefox, WebKit). Cases:

- Identity round-trip: pair, store identity in IndexedDB, reload page, confirm reconnect to known host.
- Camera-based QR scan: tested against a fixed PNG fed via `getUserMedia` mock.
- Identity export / import: round-trip an encrypted blob through a fresh browser context.
- IndexedDB clear: simulates "user clears site data," confirms the pairing UI surfaces the "no known hosts" banner correctly.
- Cross-browser xterm.js rendering: a single golden frame rendered on each engine; allowable diff documented per browser.

### Wasm Tests

`wasm-bindgen-test` for `client-core-wasm`:

- State machine on a mocked `BrowserWsTransport` that round-trips frames in-process.
- IndexedDB persistence layer end-to-end (uses real IndexedDB in the test browser).
- Crypto: re-runs a subset of the native `crypto` crate's known-answer tests under wasm to catch any wasm-specific regression in `snow`/`spake2`.

### Manual UX Passes

Pre-release checklist, run by a human. Captured as a markdown checklist in `docs/release-qa.md`. The list intentionally short; manual passes are reserved for things automation cannot judge:

- IME compose: type Chinese / Japanese / Korean in a real terminal app (e.g., `vim`, `emacs`) on each desktop OS and on iOS/Android.
- Ligatures: render Powerline, Nerd Font, programming-ligature font on each desktop platform.
- Mobile virtual keyboard: stickiness of `Ctrl`/`Alt`, long-press paste, swipe gestures.
- Reconnect from background on iOS after 30 minutes suspended.
- Slow-network feel: tc-throttled link to relay; subjective input responsiveness, snapshot-on-reconnect smoothness.
- Clipboard: copy from terminal, paste into another app and back, both on desktop and mobile.

### Fuzz Testing

`cargo-fuzz` targets:

- `fuzz_proto_decode`: arbitrary bytes → `postcard::from_bytes::<Frame>(...)`. Must never panic. Errors are fine.
- `fuzz_relay_ctrl`: same for the relay control frame type.
- `fuzz_pty_input`: arbitrary byte streams written to a `pty::TestTerminal`'s input. Must never panic; must terminate.
- `fuzz_anchor_split`: arbitrary byte streams fed to the `vte`-driven anchor finder. Must produce monotonic anchors and never split inside an escape sequence.

Fuzz targets run nightly. A failing case minimizes to a regression test.

### Observability — Daemon and Relay

Both daemon and relay use `tracing` with structured logs and ship a `tracing-subscriber` JSON formatter. Operator-facing metrics via Prometheus on a localhost-bound endpoint by default.

Daemon metrics:

- `cli_pocket_daemon_terminals_total` (gauge)
- `cli_pocket_daemon_clients_total` (gauge)
- `cli_pocket_daemon_bytes_in_total{terminal_id}` (counter)
- `cli_pocket_daemon_bytes_out_total{terminal_id}` (counter)
- `cli_pocket_daemon_handshake_failures_total{reason}` (counter)
- `cli_pocket_daemon_resume_outcomes_total{outcome}` (counter; outcomes: honored, stale, fresh)
- `cli_pocket_daemon_pty_spawn_failures_total{kind}` (counter)
- `cli_pocket_daemon_uptime_seconds` (gauge)

Relay metrics are listed in Section 7.

Logging principles:

- Structured fields, no string-formatted secrets. The custom `serde::Serialize` redaction on key types (Section 5) carries through `tracing`'s field recording.
- `host_id` is logged as last 8 chars; full IDs only at `trace` level behind a feature flag.
- Terminal payload bytes never appear in logs at any level.
- No telemetry to any third party. Logs and metrics are local; the operator chooses where they go.

### Observability — Client

Clients are user-facing, not operated. The observability story is "diagnostics export," not metrics:

- `diagnostics_export()` Tauri command (Section 6) writes a redacted bundle to a temp file: recent log lines, version info, OS info, platform, redacted host store. The user attaches it to a bug report.
- `tracing` ring buffer in memory, default 10 MiB, dumped on `diagnostics_export()`. No file logging by default — clients shouldn't accumulate state on disk silently.
- An "Enable verbose logging" toggle in the client UI flips the log level to `debug` for the current session. Off by default.

### Performance Targets

These are concrete numbers the test suite asserts on, not aspirations.

- **Local-loopback throughput**: daemon → client over loopback, single terminal, sustained 100 MB/s of output without lag. Asserted via `bytes_out` rate during a `yes` benchmark.
- **Reconnect latency**: detect drop → `Connected` again with a working terminal under 800 ms on a healthy network (loopback or LAN). Asserted in integration tests with `tokio::time::pause`.
- **Cold-start daemon**: from process spawn to listening on its WSS port: under 200 ms on a modern laptop. CI asserts under 500 ms (CI runners are slower).
- **Client cold-start**: Tauri app launch to "host list rendered" under 1.5 s on a modern laptop. Manual benchmark, not gated in CI.
- **Memory**: idle daemon under 30 MiB RSS. Daemon with 8 active terminals each at full 4 MiB scrollback under 80 MiB RSS. Asserted via end-to-end resource leak test.

### Bug-Reporting Loop

OSS project, no telemetry. The flow is:

1. User hits a bug.
2. User clicks "Diagnostics" in the app, gets a redacted bundle.
3. User opens a GitHub issue, attaches bundle.
4. Maintainer reads, reproduces, fixes.

There is intentional friction (no auto-submit). The reason: any auto-submit path requires us to operate a server and a privacy posture that contradict the OSS-self-hosted positioning. The bundle is small and easy to attach manually.

### Future Work (Out of Scope for v1)

Listed here so they're not forgotten:

- On-disk encrypted scrollback persistence (Section 4 noted this).
- Cloudflare Durable Objects relay adapter (Section 7).
- Per-host client subkeys for fine-grained revocation (Section 5).
- Frame padding for traffic-analysis resistance.
- Native keystore integration (Keychain/Keystore) for client static keys on mobile.
- Auto-update channel for the daemon with signed manifests.
- Online key-rotation announcements without re-pairing.
- mosh-style predictive local echo (Section 1 deferred this).
- Web client direct-LAN connection mode (would require user-managed CA or `mkcert`-style local CA bootstrap).
- Web app PWA / service worker for offline pairing-form caching.
- Cross-browser identity sync (encrypted blob + user-managed sync, deferred from web client v1).

These are recorded in `docs/roadmap.md` with one-line rationale each.

## Glossary

- **Daemon** — long-running process on the host machine. Owns PTYs, scrollback, identity, and serves clients.
- **Client** — Tauri desktop app, Tauri mobile app, or web app the user interacts with.
- **Tauri client** — desktop or mobile Tauri build. Uses native `client-core` and supports direct LAN + relay connections.
- **Web client** — browser-based React app using `client-core` compiled to wasm. Relay-only.
- **Relay** — public WebSocket forwarder paired clients use when direct connection is unavailable. Zero-trust: forwards ciphertext, never decrypts.
- **Pair** — daemon + client trust relationship (long-term Noise static keys exchanged once via QR / 6-digit code / fingerprint).
- **Pair offer** — short-lived (~90 s) record on the relay during 6-digit code pairing.
- **Pair ID** — opaque relay-allocated identifier for a paired session of WebSocket legs.
- **Terminal** — one PTY-backed shell session on the daemon. Identified by `TerminalId` (UUID, stable across reconnects).
- **Stream** — one client's attachment to a terminal, identified by `StreamId`. A terminal can have multiple streams.
- **Snapshot** — the bytes plus parser state needed to render a terminal's current display.
- **Anchor** — a safe point in the byte stream where parser state was captured; used to bound snapshot replay cost.
- **Resume token** — opaque value carried in `Hello` to attempt continuation of a previous session.
- **Frame** — single `postcard`-encoded enum value, the unit of the application protocol.
- **`proto` crate** — the contract-layer crate. The only place protocol versions are defined.
- **`client-core` crate** — the wasm-friendly client logic crate, generic over `Transport` / `KeyValueStore` / `Clock` / `Rng` traits. Compiled native for Tauri and to wasm32 for web.
- **`client-core-wasm` crate** — wasm-bindgen wrapper exposing `client-core` to JS. Only consumed by `apps/web`.
