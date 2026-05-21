# Plan B — Shared Contract Layer (proto + crypto + transport)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the wire protocol types (`shared/proto`), the Noise XK + SPAKE2 wrappers (`shared/crypto`), and the WebSocket transport abstraction (`shared/transport`) so every downstream plan (C, D, E, F) writes against frozen contracts.

**Architecture:** Three crates form the contract layer. `proto` owns serializable types only — no I/O, no async. `crypto` wraps `snow` and `spake2` into single-purpose builders that return either a handshake-mode or transport-mode state, plus an identity file format. `transport` defines a `Transport` trait with a tokio-tungstenite native impl and an in-memory pair impl for tests; the wasm impl lives in client-core-wasm (Plan F).

**Tech Stack:** `postcard` 1.x for serialization, `snow` 0.9.x for Noise XK, `spake2` 0.4.x (RustCrypto), `tokio-tungstenite` 0.24.x, `proptest` for round-trip tests.

**Spec reference:** § Section 2 (wire format), § Section 5 (crypto), § Section 3 (resume/ResumeToken).

**Upstream plan:** Plan A (scaffold). Read `docs/superpowers/handoff/A.md` before starting — confirm crate paths and `[workspace.dependencies]` table layout match what's below.

**Downstream:** When this plan completes, the maintainer tags `proto-v1.0.0-frozen` and pushes the tag. Plans C, D, E, F all reference frozen `proto` types.

---

## Definition of Done

- `crates/shared/proto` exports every `FrameBody` variant from the spec; `postcard` round-trip is property-tested over the whole `Frame` enum.
- `crates/shared/crypto` exposes `NoiseInitiator`, `NoiseResponder`, `Spake2Side` builders with known-answer tests against `snow`'s test vectors and an end-to-end SPAKE2 round-trip test.
- `crates/shared/crypto` exports `Identity::{generate, load, save}` for `host_identity.json` / client identity, with file-mode enforcement on Unix.
- `crates/shared/transport` defines `Transport` trait and `TokioWsTransport`; `InMemoryTransport` pair passes a 1 MiB exchange test in <1 s.
- `just check` and `just test` still pass with no `unsafe` and no clippy `-D warnings` issues.
- `git tag proto-v1.0.0-frozen` ready to push; tag message references this plan.
- Handoff note `docs/superpowers/handoff/B.md` written.

## File Structure

```
crates/shared/
├── proto/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # re-exports
│       ├── frame.rs            # FrameBody enum + Frame wrapper
│       ├── hello.rs            # Hello, HelloOk, HelloErr, ResumeToken, Capabilities
│       ├── terminal.rs         # TerminalId, StreamId, StreamSeq, TerminalCreateParams, ExitInfo, TerminalInfo
│       ├── snapshot.rs         # Snapshot, AnchorState, SgrAttrs, TerminalModes, CharsetState
│       ├── error.rs            # ProtocolError, ByeReason
│       ├── relay.rs            # RelayCtrl, RelayData, PairId, OfferId, Endpoint
│       └── codec.rs            # encode_frame / decode_frame helpers (postcard)
├── crypto/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── identity.rs         # Identity struct, generate/load/save, file-mode checks
│       ├── noise.rs            # NoiseInitiator/Responder builders, transport-mode handles
│       ├── spake2.rs           # Spake2Side wrapper, two-message exchange
│       └── redact.rs           # Secret<T> wrapper with redacted Debug/Serialize
└── transport/
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── transport.rs        # Transport trait + Frame send/recv helpers
        ├── tokio_ws.rs         # tokio-tungstenite native impl
        └── memory.rs           # InMemoryTransport pair for tests
```

---

## Task B1 — Pre-flight: read handoff, populate `[workspace.dependencies]`

**Files:**
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Read upstream handoff**

Run: `cat docs/superpowers/handoff/A.md`
Expected: Plan A's actual outputs. Note any deviations from the assumptions below:
- Workspace `members` includes the 10 crates from Plan A.
- `[workspace.dependencies]` is reserved but empty.
- Rust edition is `2021`, MSRV is `1.84`.

If Plan A deviated (different MSRV, different crate names), update task references in this plan inline before proceeding.

- [ ] **Step 2: Populate `[workspace.dependencies]`**

Edit the workspace `Cargo.toml`, replace the empty `[workspace.dependencies]` block (comments + table) with:

```toml
[workspace.dependencies]
# Serialization
postcard       = { version = "1", features = ["use-std"] }
serde          = { version = "1", features = ["derive"] }
serde_bytes    = "0.11"
serde_json     = "1"

# Crypto
snow           = { version = "0.9", default-features = false, features = ["default-resolver"] }
spake2         = "0.4"
rand_core      = { version = "0.6", features = ["std"] }

# Async runtime / transport
tokio              = { version = "1", features = ["rt", "macros", "sync", "time", "io-util", "net"] }
tokio-tungstenite  = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
futures-util       = "0.3"
async-trait        = "0.1"

# Errors / logging / id
thiserror      = "1"
tracing        = "0.1"
uuid           = { version = "1", features = ["v7", "serde"] }
bytes          = { version = "1", features = ["serde"] }
base64         = "0.22"

# Utilities used by plan C+ but pinned now for version stability
notify         = "6"
vte            = "0.13"
portable-pty   = "0.8"
clap           = { version = "4", features = ["derive", "env"] }

# Dev-only (not in regular deps)
proptest       = "1"
```

(If any version above is unavailable on the registry when the engineer runs this — minor versions move — bump to the nearest available compatible version. Document the bumps in handoff.)

- [ ] **Step 3: Verify workspace still resolves**

Run: `cargo check --workspace`
Expected: success. No crate consumes the new deps yet, so this only validates the version selector syntax.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "chore(workspace): populate [workspace.dependencies] for shared contract layer"
```

---

## Task B2 — `proto`: ID/seq/uuid newtypes

**Files:**
- Modify: `crates/shared/proto/Cargo.toml`
- Modify: `crates/shared/proto/src/lib.rs`
- Create: `crates/shared/proto/src/terminal.rs`

- [ ] **Step 1: Update `crates/shared/proto/Cargo.toml`**

```toml
[package]
name = "cli-pocket-proto"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Wire protocol contracts for cli-pocket."

[dependencies]
serde       = { workspace = true }
serde_bytes = { workspace = true }
postcard    = { workspace = true }
thiserror   = { workspace = true }
uuid        = { workspace = true }
bytes       = { workspace = true }

[dev-dependencies]
proptest    = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/shared/proto/src/terminal.rs`**

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalId(pub Uuid);

impl TerminalId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TerminalId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamSeq(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HostId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCreateParams {
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub cmd: Vec<String>,           // empty = login shell
    pub env: Vec<(String, String)>,
    pub scrollback_bytes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub terminal: TerminalId,
    pub cols: u16,
    pub rows: u16,
    pub created_at_unix_ms: u64,
    pub label: Option<String>,
    pub attached_clients: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitInfo {
    pub code: Option<i32>,          // None if killed by signal
    pub signal: Option<u32>,
    pub at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KillSignal {
    Term,
    Hup,
    Kill,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip_through_postcard() {
        let t = TerminalId::new();
        let bytes = postcard::to_allocvec(&t).unwrap();
        let back: TerminalId = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(t, back);
    }
}
```

- [ ] **Step 3: Update `crates/shared/proto/src/lib.rs`**

```rust
//! Wire protocol contracts. See docs/superpowers/specs/2026-05-21-cross-platform-remote-terminal-design.md § 2.

pub mod terminal;

pub use terminal::{
    ClientId, ExitInfo, HostId, KillSignal, SessionId, StreamId, StreamSeq, TerminalCreateParams,
    TerminalId, TerminalInfo,
};

/// Wire protocol version negotiated in `Hello`.
pub const PROTOCOL_VERSION: u32 = 1;
```

- [ ] **Step 4: Test**

Run: `cargo test -p cli-pocket-proto`
Expected: `test terminal::tests::ids_roundtrip_through_postcard ... ok`.

- [ ] **Step 5: Commit**

```bash
git add crates/shared/proto/Cargo.toml crates/shared/proto/src/lib.rs crates/shared/proto/src/terminal.rs
git commit -m "feat(proto): add Terminal/Stream/Session/Host/Client id newtypes and lifecycle params"
```

---

## Task B3 — `proto`: Error and Bye enums

**Files:**
- Create: `crates/shared/proto/src/error.rs`
- Modify: `crates/shared/proto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/proto/src/error.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ProtocolError {
    #[error("unknown terminal")]
    UnknownTerminal,
    #[error("unauthorized")]
    Unauthorized,
    #[error("backpressure exceeded")]
    BackpressureExceeded,
    #[error("protocol mismatch")]
    ProtocolMismatch,
    #[error("resource exhausted")]
    ResourceExhausted,
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[error("resume stale")]
    ResumeStale,
    #[error("rate limited")]
    RateLimited,
    /// Forward-compat catchall. Peers that don't recognize a future variant
    /// can fall back to this; older peers see only `Other`.
    #[error("other: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByeReason {
    Normal,
    Revoked,
    ServerShutdown,
    ProtocolError(ProtocolError),
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

Edit `crates/shared/proto/src/lib.rs` — add `pub mod error;` after `pub mod terminal;` and `pub use error::{ByeReason, ProtocolError};` after the existing re-exports.

- [ ] **Step 3: Test**

Run: `cargo build -p cli-pocket-proto`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/proto/src/error.rs crates/shared/proto/src/lib.rs
git commit -m "feat(proto): add ProtocolError and ByeReason"
```

---

## Task B4 — `proto`: Hello/HelloOk/HelloErr + Capabilities + ResumeToken

**Files:**
- Create: `crates/shared/proto/src/hello.rs`
- Modify: `crates/shared/proto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/proto/src/hello.rs`**

```rust
use crate::error::ProtocolError;
use crate::terminal::{ClientId, SessionId, StreamSeq, TerminalId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub capabilities: Capabilities,
    pub client_kind: ClientKind,
    pub resume: Option<ResumeToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOk {
    pub protocol: u32,
    pub server_info: ServerInfo,
    pub session_id: SessionId,
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloErr {
    pub error: ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub server_version: String,        // human-readable, e.g. "cli-pocket-daemon 0.1.0"
    pub host_label: Option<String>,    // user-supplied display name for the host
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientKind {
    Daemon,                            // daemon ↔ daemon (unused at v1, reserved)
    DesktopTauri,
    MobileTauri,
    Web,
    Cli,
}

/// Bitfield-style additive capabilities. Reserved bits MUST be zero on the
/// wire so a future peer that sets them can be detected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub bits: u32,
}

impl Capabilities {
    pub const NONE: Self = Self { bits: 0 };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeToken {
    pub session_id: SessionId,
    pub attachments: Vec<ResumeAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeAttachment {
    pub terminal: TerminalId,
    pub last_seq: StreamSeq,
}

/// What the server attaches into `HelloOk` is the SessionId — what the client
/// presents on reconnect is a fresh ResumeToken built from the per-terminal
/// last_seq values it has observed. ClientId is conveyed out-of-band via
/// Noise static-key authentication; it doesn't ride in Hello.
#[allow(dead_code)]
fn _client_id_only_via_static_key(_: ClientId) {}
```

- [ ] **Step 2: Re-export from `lib.rs`**

Add `pub mod hello;` and `pub use hello::{Capabilities, ClientKind, Hello, HelloErr, HelloOk, ResumeAttachment, ResumeToken, ServerInfo};`.

- [ ] **Step 3: Test**

Run: `cargo build -p cli-pocket-proto`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/proto/src/hello.rs crates/shared/proto/src/lib.rs
git commit -m "feat(proto): add Hello / HelloOk / HelloErr / Capabilities / ResumeToken"
```

---

## Task B5 — `proto`: Snapshot + AnchorState

**Files:**
- Create: `crates/shared/proto/src/snapshot.rs`
- Modify: `crates/shared/proto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/proto/src/snapshot.rs`**

```rust
use crate::terminal::StreamSeq;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub anchor_state: AnchorState,
    pub bytes: ByteBuf,                // replay from anchor to head
    pub head_seq: StreamSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorState {
    pub cursor: (u16, u16),
    pub sgr: SgrAttrs,
    pub modes: TerminalModes,
    pub charset: CharsetState,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SgrAttrs {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub reverse: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    /// Standard 16-color palette index (0..=15).
    Palette(u8),
    /// 256-color extended palette (0..=255).
    Indexed(u8),
    /// 24-bit truecolor.
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalModes {
    pub deccmm_cursor_keys: bool,      // DECCKM
    pub autowrap: bool,                // DECAWM
    pub alt_screen: bool,              // 1049
    pub bracketed_paste: bool,         // 2004
    pub mouse_reporting: MouseMode,
    pub origin_mode: bool,             // DECOM
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseMode {
    #[default]
    Off,
    X10,
    Normal,
    ButtonEvent,
    AnyEvent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharsetState {
    /// G0..G3 character set designations as raw final-bytes per ECMA-35.
    /// Default ('B','B','B','B') = US-ASCII.
    pub g: [u8; 4],
    /// Active GL set index (0..=3).
    pub gl: u8,
    /// Active GR set index (0..=3).
    pub gr: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaSlice {
    pub bytes: ByteBuf,
    pub head_seq: StreamSeq,
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

```rust
pub mod snapshot;
pub use snapshot::{
    AnchorState, CharsetState, Color, DeltaSlice, MouseMode, SgrAttrs, Snapshot, TerminalModes,
};
```

- [ ] **Step 3: Test**

Run: `cargo build -p cli-pocket-proto`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/proto/src/snapshot.rs crates/shared/proto/src/lib.rs
git commit -m "feat(proto): add Snapshot, AnchorState, SGR, modes, charset"
```

---

## Task B6 — `proto`: Frame enum

**Files:**
- Create: `crates/shared/proto/src/frame.rs`
- Modify: `crates/shared/proto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/proto/src/frame.rs`**

```rust
use crate::error::{ByeReason, ProtocolError};
use crate::hello::{Hello, HelloErr, HelloOk};
use crate::snapshot::Snapshot;
use crate::terminal::{ExitInfo, StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub body: FrameBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameBody {
    // ---- Connection control ----
    Hello(Hello),
    HelloOk(HelloOk),
    HelloErr(HelloErr),
    Ping { nonce: u32 },
    Pong { nonce: u32 },
    Bye { reason: ByeReason },

    // ---- Terminal lifecycle (request/response, request_id paired) ----
    TerminalCreate { request_id: u32, params: TerminalCreateParams },
    TerminalCreateOk { request_id: u32, terminal: TerminalId, stream: StreamId },
    TerminalCreateErr { request_id: u32, error: ProtocolError },

    TerminalAttach { request_id: u32, terminal: TerminalId, since: Option<StreamSeq> },
    TerminalAttachOk {
        request_id: u32,
        snapshot: Snapshot,
        head_seq: StreamSeq,
        stream: StreamId,
        initial_window: u32,
    },
    TerminalAttachErr { request_id: u32, error: ProtocolError },

    TerminalDetach { stream: StreamId },
    TerminalKill { request_id: u32, terminal: TerminalId },
    TerminalKillOk { request_id: u32 },
    TerminalKillErr { request_id: u32, error: ProtocolError },

    TerminalList { request_id: u32 },
    TerminalListOk { request_id: u32, terminals: Vec<TerminalInfo> },

    TerminalExit { terminal: TerminalId, exit: ExitInfo },

    // ---- Data plane (per terminal stream) ----
    Output { stream: StreamId, seq: StreamSeq, bytes: ByteBuf },
    Input { stream: StreamId, bytes: ByteBuf },
    Resize { stream: StreamId, cols: u16, rows: u16 },

    // ---- Flow control ----
    Window { stream: StreamId, credit: u32 },
}

impl Frame {
    pub fn body(body: FrameBody) -> Self {
        Self { body }
    }
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

```rust
pub mod frame;
pub use frame::{Frame, FrameBody};
```

- [ ] **Step 3: Test**

Run: `cargo build -p cli-pocket-proto`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/proto/src/frame.rs crates/shared/proto/src/lib.rs
git commit -m "feat(proto): add Frame and FrameBody covering full v1 surface"
```

---

## Task B7 — `proto`: Relay frames

**Files:**
- Create: `crates/shared/proto/src/relay.rs`
- Modify: `crates/shared/proto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/proto/src/relay.rs`**

```rust
use crate::terminal::HostId;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OfferId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Endpoint {
    Direct { host: String, port: u16 },
    Loopback { port: u16 },
    Relay { relay_url: String, host_id: HostId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairCloseReason {
    Normal,
    HostGone,
    ClientGone,
    Stuck,
    RelayShutdown,
    Rejected(String),
}

/// Relay control-plane messages. Carried as postcard inside a WS frame whose
/// first byte is 0x01 (control discriminator). See § Section 7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayCtrl {
    // host → relay
    HostRegister { host_id: HostId, host_pubkey: ByteBuf, signature: ByteBuf },
    HostRegisterOk,
    HostRegisterErr { reason: String },
    HostHeartbeat,
    HostUnregister,

    // client → relay
    ClientPairRequest { host_id: HostId, attempt_token: u32 },
    ClientCodeLookup { hint: ByteBuf },
    ClientPairCancel,

    // relay → host
    PairInbound { pair_id: PairId, attempt_token: u32 },
    PairRejected { reason: String },

    // relay → both
    PairOpen { pair_id: PairId },
    PairClose { pair_id: PairId, reason: PairCloseReason },

    // pair-code rendezvous
    OfferAvailable { offer_id: OfferId, host_pubkey: ByteBuf, endpoints: Vec<Endpoint> },
    OfferConsumed,
    OfferStale,
    OfferPublish {
        offer_id: OfferId,
        spake2_m_share: ByteBuf,
        host_pubkey: ByteBuf,
        endpoints: Vec<Endpoint>,
        ttl_secs: u32,
    },
    OfferRetract { offer_id: OfferId },
}

/// Relay data-plane message. Carried as postcard inside a WS frame whose
/// first byte is 0x02. The `bytes` payload is opaque Noise ciphertext to the
/// relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayData {
    Forward { pair_id: PairId, bytes: ByteBuf },
}

pub const RELAY_DISC_CTRL: u8 = 0x01;
pub const RELAY_DISC_DATA: u8 = 0x02;
```

- [ ] **Step 2: Re-export from `lib.rs`**

```rust
pub mod relay;
pub use relay::{
    Endpoint, OfferId, PairCloseReason, PairId, RelayCtrl, RelayData, RELAY_DISC_CTRL,
    RELAY_DISC_DATA,
};
```

- [ ] **Step 3: Test**

Run: `cargo build -p cli-pocket-proto`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/proto/src/relay.rs crates/shared/proto/src/lib.rs
git commit -m "feat(proto): add RelayCtrl / RelayData and discriminator bytes"
```

---

## Task B8 — `proto`: Codec + `Frame` round-trip proptest

**Files:**
- Create: `crates/shared/proto/src/codec.rs`
- Modify: `crates/shared/proto/src/lib.rs`
- Create: `crates/shared/proto/tests/roundtrip.rs`

- [ ] **Step 1: Write `crates/shared/proto/src/codec.rs`**

```rust
use crate::frame::Frame;
use crate::relay::{RelayCtrl, RelayData, RELAY_DISC_CTRL, RELAY_DISC_DATA};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("empty frame")]
    Empty,
    #[error("unknown discriminator {0:#x}")]
    UnknownDiscriminator(u8),
}

pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, CodecError> {
    Ok(postcard::to_allocvec(frame)?)
}

pub fn decode_frame(bytes: &[u8]) -> Result<Frame, CodecError> {
    Ok(postcard::from_bytes(bytes)?)
}

pub enum RelayWire {
    Ctrl(RelayCtrl),
    Data(RelayData),
}

pub fn encode_relay_ctrl(ctrl: &RelayCtrl) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(64);
    out.push(RELAY_DISC_CTRL);
    out.extend_from_slice(&postcard::to_allocvec(ctrl)?);
    Ok(out)
}

pub fn encode_relay_data(data: &RelayData) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(64);
    out.push(RELAY_DISC_DATA);
    out.extend_from_slice(&postcard::to_allocvec(data)?);
    Ok(out)
}

pub fn decode_relay(bytes: &[u8]) -> Result<RelayWire, CodecError> {
    let (disc, rest) = bytes.split_first().ok_or(CodecError::Empty)?;
    match *disc {
        RELAY_DISC_CTRL => Ok(RelayWire::Ctrl(postcard::from_bytes(rest)?)),
        RELAY_DISC_DATA => Ok(RelayWire::Data(postcard::from_bytes(rest)?)),
        other => Err(CodecError::UnknownDiscriminator(other)),
    }
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

```rust
pub mod codec;
pub use codec::{
    decode_frame, decode_relay, encode_frame, encode_relay_ctrl, encode_relay_data, CodecError,
    RelayWire,
};
```

- [ ] **Step 3: Write the proptest at `crates/shared/proto/tests/roundtrip.rs`**

```rust
use cli_pocket_proto::*;
use proptest::prelude::*;
use serde_bytes::ByteBuf;
use uuid::Uuid;

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<[u8; 16]>().prop_map(Uuid::from_bytes)
}

fn arb_bytes(max_len: usize) -> impl Strategy<Value = ByteBuf> {
    prop::collection::vec(any::<u8>(), 0..max_len).prop_map(ByteBuf::from)
}

fn arb_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        Just(FrameBody::Ping { nonce: 7 }),
        Just(FrameBody::Pong { nonce: 7 }),
        (arb_uuid(), 0u32..1_000_000).prop_map(|(u, rid)| FrameBody::TerminalCreate {
            request_id: rid,
            params: TerminalCreateParams {
                cols: 80,
                rows: 24,
                cwd: None,
                cmd: vec![],
                env: vec![],
                scrollback_bytes: None,
            },
        }).boxed(),
        (arb_uuid(), 0u64..u64::MAX, arb_bytes(4096)).prop_map(|(u, seq, b)| FrameBody::Output {
            stream: StreamId(1),
            seq: StreamSeq(seq),
            bytes: b,
        }).boxed(),
        (arb_bytes(4096)).prop_map(|b| FrameBody::Input {
            stream: StreamId(1),
            bytes: b,
        }).boxed(),
        Just(FrameBody::Resize { stream: StreamId(1), cols: 120, rows: 40 }),
        any::<u32>().prop_map(|c| FrameBody::Window { stream: StreamId(1), credit: c }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..Default::default() })]

    #[test]
    fn frame_roundtrips_through_postcard(body in arb_frame_body()) {
        let frame = Frame::body(body);
        let bytes = encode_frame(&frame).unwrap();
        let back = decode_frame(&bytes).unwrap();
        prop_assert_eq!(frame, back);
    }
}

#[test]
fn relay_ctrl_roundtrip() {
    let ctrl = RelayCtrl::HostRegister {
        host_id: HostId(Uuid::nil()),
        host_pubkey: ByteBuf::from(vec![0u8; 32]),
        signature: ByteBuf::from(vec![0u8; 64]),
    };
    let wire = encode_relay_ctrl(&ctrl).unwrap();
    let back = decode_relay(&wire).unwrap();
    match back {
        RelayWire::Ctrl(c) => assert_eq!(c, ctrl),
        _ => panic!("expected Ctrl"),
    }
}

#[test]
fn relay_data_roundtrip() {
    let data = RelayData::Forward {
        pair_id: PairId(Uuid::nil()),
        bytes: ByteBuf::from(vec![1, 2, 3]),
    };
    let wire = encode_relay_data(&data).unwrap();
    let back = decode_relay(&wire).unwrap();
    match back {
        RelayWire::Data(d) => assert_eq!(d, data),
        _ => panic!("expected Data"),
    }
}

#[test]
fn relay_unknown_discriminator_errors() {
    let bytes = vec![0xFFu8, 0x00, 0x00];
    assert!(decode_relay(&bytes).is_err());
}
```

- [ ] **Step 4: Run proptests**

Run: `cargo test -p cli-pocket-proto`
Expected: all tests pass, including 256 proptest cases.

- [ ] **Step 5: Commit**

```bash
git add crates/shared/proto/src/codec.rs crates/shared/proto/src/lib.rs crates/shared/proto/tests/roundtrip.rs
git commit -m "feat(proto): add codec helpers + Frame postcard round-trip proptest"
```

---

## Task B9 — `crypto`: `Secret<T>` redaction wrapper

**Files:**
- Modify: `crates/shared/crypto/Cargo.toml`
- Create: `crates/shared/crypto/src/redact.rs`
- Modify: `crates/shared/crypto/src/lib.rs`

- [ ] **Step 1: Update `crates/shared/crypto/Cargo.toml`**

```toml
[package]
name = "cli-pocket-crypto"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Noise XK + SPAKE2 + identity for cli-pocket."

[dependencies]
snow         = { workspace = true }
spake2       = { workspace = true }
rand_core    = { workspace = true }
serde        = { workspace = true }
serde_json   = { workspace = true }
serde_bytes  = { workspace = true }
base64       = { workspace = true }
thiserror    = { workspace = true }
tracing      = { workspace = true }
uuid         = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/shared/crypto/src/redact.rs`**

```rust
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Wraps secret bytes (private keys, PSKs, SPAKE2 shares mid-flight).
/// `Debug`/`Display` redact; `Serialize` writes the raw bytes since we DO
/// need to persist them in `host_identity.json`. The redaction protection is
/// against accidental `tracing` / `eprintln!` leaks, not against serializers.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
    pub fn expose(&self) -> &T {
        &self.0
    }
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted>")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted>")
    }
}

impl<T: Serialize> Serialize for Secret<T> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(Self(T::deserialize(de)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts() {
        let s = Secret::new(vec![1u8, 2, 3]);
        assert_eq!(format!("{s:?}"), "<redacted>");
    }

    #[test]
    fn serialize_preserves_payload() {
        let s = Secret::new(vec![1u8, 2, 3]);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "[1,2,3]");
    }
}
```

- [ ] **Step 3: Update `crates/shared/crypto/src/lib.rs`**

```rust
//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod identity;
pub mod noise;
pub mod redact;
pub mod spake2;

pub use identity::{Identity, IdentityError, KeyPair};
pub use noise::{NoiseError, NoiseInitiator, NoiseResponder, NoiseSession};
pub use redact::Secret;
pub use spake2::{Spake2Error, Spake2Outcome, Spake2Side};
```

Note: `identity`, `noise`, and `spake2` modules don't exist yet — Tasks B10–B12 create them. This stub causes `cargo build` to fail until those tasks land. That's fine; commit only this file plus `redact.rs` for now, and revert the missing `pub use`s temporarily:

For this commit, write a minimal lib.rs that only re-exports `Secret`:

```rust
//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod redact;
pub use redact::Secret;
```

The full re-exports get added back in Task B12.

- [ ] **Step 4: Test**

Run: `cargo test -p cli-pocket-crypto`
Expected: redact tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/shared/crypto/Cargo.toml crates/shared/crypto/src/lib.rs crates/shared/crypto/src/redact.rs
git commit -m "feat(crypto): add Secret<T> redaction wrapper"
```

---

## Task B10 — `crypto`: Identity file (`host_identity.json` / client identity)

**Files:**
- Create: `crates/shared/crypto/src/identity.rs`

- [ ] **Step 1: Write `crates/shared/crypto/src/identity.rs`**

```rust
use crate::redact::Secret;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde_json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("identity file has wrong permissions (expected mode 0600): {0}")]
    BadPermissions(String),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid key length: expected 32 bytes, got {0}")]
    WrongKeyLength(usize),
}

/// X25519 keypair used both as the Noise static key and (via
/// Ed25519↔X25519 conversion) as a signing identity for relay registration.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyPair {
    pub public: [u8; 32],
    pub secret: Secret<[u8; 32]>,
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &B64.encode(self.public))
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl KeyPair {
    pub fn generate() -> Self {
        let builder = snow::Builder::new(noise_params());
        let kp = builder
            .generate_keypair()
            .expect("snow generate_keypair must succeed");
        let mut public = [0u8; 32];
        let mut secret = [0u8; 32];
        public.copy_from_slice(&kp.public);
        secret.copy_from_slice(&kp.private);
        Self {
            public,
            secret: Secret::new(secret),
        }
    }
}

fn noise_params() -> snow::params::NoiseParams {
    "Noise_XK_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .expect("static valid noise params string")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub version: u32,
    pub id: Uuid,
    pub created_at: String, // RFC3339
    #[serde(rename = "static_public_key", with = "key32_b64")]
    pub static_public: [u8; 32],
    #[serde(rename = "static_secret_key")]
    pub static_secret: Secret<KeyBytes32>,
}

/// Newtype carrying 32 raw bytes, base64-serialized.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyBytes32(pub [u8; 32]);

impl Serialize for KeyBytes32 {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&B64.encode(self.0))
    }
}

impl<'de> Deserialize<'de> for KeyBytes32 {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        let bytes = B64.decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "expected 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(KeyBytes32(arr))
    }
}

mod key32_b64 {
    use super::*;

    pub fn serialize<S: Serializer>(key: &[u8; 32], ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&B64.encode(key))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<[u8; 32], D::Error> {
        let kb = KeyBytes32::deserialize(de)?;
        Ok(kb.0)
    }
}

impl Identity {
    pub fn from_keypair(kp: &KeyPair) -> Self {
        Self {
            version: 1,
            id: Uuid::now_v7(),
            created_at: now_rfc3339(),
            static_public: kp.public,
            static_secret: Secret::new(KeyBytes32(*kp.secret.expose())),
        }
    }

    pub fn keypair(&self) -> KeyPair {
        KeyPair {
            public: self.static_public,
            secret: Secret::new(self.static_secret.expose().0),
        }
    }

    pub fn load(path: &Path) -> Result<Self, IdentityError> {
        check_mode(path)?;
        let bytes = fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save(&self, path: &Path) -> Result<(), IdentityError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        fs::write(path, json)?;
        set_mode_600(path)?;
        Ok(())
    }
}

fn now_rfc3339() -> String {
    // Minimal RFC3339 without pulling chrono. Uses std::time only.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (year, month, day, hour, minute, second) = epoch_to_ymd_hms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn epoch_to_ymd_hms(mut s: u64) -> (i32, u32, u32, u32, u32, u32) {
    let second = (s % 60) as u32;
    s /= 60;
    let minute = (s % 60) as u32;
    s /= 60;
    let hour = (s % 24) as u32;
    let mut days = (s / 24) as i64;
    let mut year: i32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let mlen = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month: u32 = 1;
    for m in mlen {
        if days < m as i64 {
            break;
        }
        days -= m as i64;
        month += 1;
    }
    let day = days as u32 + 1;
    (year, month, day, hour, minute, second)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(unix)]
fn check_mode(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(path)?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(IdentityError::BadPermissions(format!(
            "{}: got 0o{:o}, expected 0o600. Fix with: chmod 600 {}",
            path.display(),
            mode,
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_mode(_path: &Path) -> Result<(), IdentityError> {
    // Windows: rely on inherited ACL — the file lives in the per-user app
    // data directory which already restricts to the owning user. Tighter
    // ACL enforcement via `icacls` is in Plan D (daemon-bin), not here.
    Ok(())
}

#[cfg(unix)]
fn set_mode_600(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(path)?.permissions();
    perm.set_mode(0o600);
    fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode_600(_path: &Path) -> Result<(), IdentityError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn keypair_generates_32_byte_public() {
        let kp = KeyPair::generate();
        assert_eq!(kp.public.len(), 32);
        assert_eq!(kp.secret.expose().len(), 32);
    }

    #[test]
    fn identity_roundtrips_through_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("host_identity.json");
        let kp = KeyPair::generate();
        let id = Identity::from_keypair(&kp);
        id.save(&path).unwrap();
        let back = Identity::load(&path).unwrap();
        assert_eq!(back.static_public, id.static_public);
        assert_eq!(back.static_secret.expose().0, id.static_secret.expose().0);
    }

    #[cfg(unix)]
    #[test]
    fn load_rejects_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let path = dir.path().join("host_identity.json");
        let id = Identity::from_keypair(&KeyPair::generate());
        id.save(&path).unwrap();
        let mut perm = fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o644);
        fs::set_permissions(&path, perm).unwrap();
        let err = Identity::load(&path).unwrap_err();
        assert!(matches!(err, IdentityError::BadPermissions(_)));
    }

    #[test]
    fn epoch_to_ymd_works_for_known_value() {
        // 2026-05-21T00:00:00Z = 1779580800
        let (y, m, d, h, mi, s) = epoch_to_ymd_hms(1_779_580_800);
        assert_eq!((y, m, d, h, mi, s), (2026, 5, 21, 0, 0, 0));
    }
}
```

- [ ] **Step 2: Add `tempfile` as a dev-dep**

Edit `crates/shared/crypto/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Update `crates/shared/crypto/src/lib.rs`**

```rust
//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod identity;
pub mod redact;

pub use identity::{Identity, IdentityError, KeyBytes32, KeyPair};
pub use redact::Secret;
```

- [ ] **Step 4: Test**

Run: `cargo test -p cli-pocket-crypto`
Expected: identity tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/shared/crypto/Cargo.toml crates/shared/crypto/src/identity.rs crates/shared/crypto/src/lib.rs
git commit -m "feat(crypto): add Identity, KeyPair, file-mode-checked load/save"
```

---

## Task B11 — `crypto`: Noise XK wrapper

**Files:**
- Create: `crates/shared/crypto/src/noise.rs`
- Modify: `crates/shared/crypto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/crypto/src/noise.rs`**

```rust
use crate::identity::KeyPair;
use crate::redact::Secret;

const NOISE_PARAMS: &str = "Noise_XK_25519_ChaChaPoly_BLAKE2s";
const NOISE_MAX_MSG_LEN: usize = 65535;
const NOISE_TAG_LEN: usize = 16;
const NOISE_HANDSHAKE_MSG_BUF: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum NoiseError {
    #[error("snow: {0}")]
    Snow(#[from] snow::Error),
    #[error("payload exceeds Noise message size limit")]
    PayloadTooLarge,
    #[error("handshake not finished")]
    HandshakeNotFinished,
    #[error("psk required but not configured")]
    PskMissing,
}

/// Client side of Noise XK. The initiator knows the responder's static
/// public key out of band (from pairing).
pub struct NoiseInitiator {
    state: Option<snow::HandshakeState>,
}

impl NoiseInitiator {
    pub fn new(local: &KeyPair, remote_static_public: &[u8; 32], psk: Option<&[u8; 32]>) -> Result<Self, NoiseError> {
        let mut builder = snow::Builder::new(NOISE_PARAMS.parse().expect("static"));
        builder = builder
            .local_private_key(local.secret.expose())
            .remote_public_key(remote_static_public);
        if let Some(psk) = psk {
            builder = builder.psk(2, psk);
        }
        let state = builder.build_initiator()?;
        Ok(Self { state: Some(state) })
    }

    /// Write the next handshake message into the returned buffer.
    pub fn write_handshake(&mut self) -> Result<Vec<u8>, NoiseError> {
        let state = self.state.as_mut().ok_or(NoiseError::HandshakeNotFinished)?;
        let mut buf = vec![0u8; NOISE_HANDSHAKE_MSG_BUF];
        let n = state.write_message(&[], &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    pub fn read_handshake(&mut self, msg: &[u8]) -> Result<(), NoiseError> {
        let state = self.state.as_mut().ok_or(NoiseError::HandshakeNotFinished)?;
        let mut scratch = vec![0u8; NOISE_HANDSHAKE_MSG_BUF];
        state.read_message(msg, &mut scratch)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<NoiseSession, NoiseError> {
        let state = self
            .state
            .take()
            .ok_or(NoiseError::HandshakeNotFinished)?
            .into_transport_mode()?;
        Ok(NoiseSession { transport: state })
    }

    pub fn is_handshake_finished(&self) -> bool {
        self.state.as_ref().map(|s| s.is_handshake_finished()).unwrap_or(false)
    }
}

/// Server side of Noise XK.
pub struct NoiseResponder {
    state: Option<snow::HandshakeState>,
}

impl NoiseResponder {
    pub fn new(local: &KeyPair, psk: Option<&[u8; 32]>) -> Result<Self, NoiseError> {
        let mut builder = snow::Builder::new(NOISE_PARAMS.parse().expect("static"));
        builder = builder.local_private_key(local.secret.expose());
        if let Some(psk) = psk {
            builder = builder.psk(2, psk);
        }
        let state = builder.build_responder()?;
        Ok(Self { state: Some(state) })
    }

    pub fn read_handshake(&mut self, msg: &[u8]) -> Result<(), NoiseError> {
        let state = self.state.as_mut().ok_or(NoiseError::HandshakeNotFinished)?;
        let mut scratch = vec![0u8; NOISE_HANDSHAKE_MSG_BUF];
        state.read_message(msg, &mut scratch)?;
        Ok(())
    }

    pub fn write_handshake(&mut self) -> Result<Vec<u8>, NoiseError> {
        let state = self.state.as_mut().ok_or(NoiseError::HandshakeNotFinished)?;
        let mut buf = vec![0u8; NOISE_HANDSHAKE_MSG_BUF];
        let n = state.write_message(&[], &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// After handshake completes, the responder learns the initiator's static
    /// public key — this is what the daemon checks against `clients.json`.
    pub fn remote_static_public(&self) -> Option<[u8; 32]> {
        let state = self.state.as_ref()?;
        let rs = state.get_remote_static()?;
        if rs.len() != 32 {
            return None;
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(rs);
        Some(out)
    }

    pub fn finish(mut self) -> Result<NoiseSession, NoiseError> {
        let state = self
            .state
            .take()
            .ok_or(NoiseError::HandshakeNotFinished)?
            .into_transport_mode()?;
        Ok(NoiseSession { transport: state })
    }

    pub fn is_handshake_finished(&self) -> bool {
        self.state.as_ref().map(|s| s.is_handshake_finished()).unwrap_or(false)
    }
}

/// Transport-mode Noise. Encrypts/decrypts whole frames; nonces are managed
/// internally by `snow`. Each direction has its own counter; never reuse a
/// single `NoiseSession` between two directions.
pub struct NoiseSession {
    transport: snow::TransportState,
}

impl NoiseSession {
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if plaintext.len() + NOISE_TAG_LEN > NOISE_MAX_MSG_LEN {
            return Err(NoiseError::PayloadTooLarge);
        }
        let mut out = vec![0u8; plaintext.len() + NOISE_TAG_LEN];
        let n = self.transport.write_message(plaintext, &mut out)?;
        out.truncate(n);
        Ok(out)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let mut out = vec![0u8; ciphertext.len()];
        let n = self.transport.read_message(ciphertext, &mut out)?;
        out.truncate(n);
        Ok(out)
    }
}

impl std::fmt::Debug for NoiseSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoiseSession").field("state", &"<active>").finish()
    }
}

// Suppress unused-import warning for Secret in this module — we hold it via
// KeyPair but don't reach in directly.
#[allow(dead_code)]
fn _secret_marker(_: Secret<[u8; 32]>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::KeyPair;

    /// Drive a full XK handshake between initiator and responder, then send
    /// a round-trip ciphertext both ways.
    #[test]
    fn noise_xk_full_handshake_and_transport() {
        let server = KeyPair::generate();
        let client = KeyPair::generate();

        let mut init = NoiseInitiator::new(&client, &server.public, None).unwrap();
        let mut resp = NoiseResponder::new(&server, None).unwrap();

        // 1. e
        let m1 = init.write_handshake().unwrap();
        resp.read_handshake(&m1).unwrap();
        // 2. e, ee, s, es
        let m2 = resp.write_handshake().unwrap();
        init.read_handshake(&m2).unwrap();
        // 3. s, se
        let m3 = init.write_handshake().unwrap();
        resp.read_handshake(&m3).unwrap();

        assert!(init.is_handshake_finished());
        assert!(resp.is_handshake_finished());

        // The responder now learns the initiator's static key.
        let learned = resp.remote_static_public().unwrap();
        assert_eq!(learned, client.public);

        let mut c2s = init.finish().unwrap();
        let mut s2c = resp.finish().unwrap();

        // Round-trip a payload in each direction.
        let plaintext_c = b"hello daemon";
        let ct_c = c2s.encrypt(plaintext_c).unwrap();
        let pt_c = s2c.decrypt(&ct_c).unwrap();
        assert_eq!(pt_c, plaintext_c);

        let plaintext_s = b"hello client";
        let ct_s = s2c.encrypt(plaintext_s).unwrap();
        let pt_s = c2s.decrypt(&ct_s).unwrap();
        assert_eq!(pt_s, plaintext_s);
    }

    #[test]
    fn psk_mismatch_rejects_handshake() {
        let server = KeyPair::generate();
        let client = KeyPair::generate();
        let psk_a = [1u8; 32];
        let psk_b = [2u8; 32];

        let mut init = NoiseInitiator::new(&client, &server.public, Some(&psk_a)).unwrap();
        let mut resp = NoiseResponder::new(&server, Some(&psk_b)).unwrap();

        let m1 = init.write_handshake().unwrap();
        resp.read_handshake(&m1).unwrap();
        let m2 = resp.write_handshake().unwrap();
        init.read_handshake(&m2).unwrap();
        let m3 = init.write_handshake().unwrap();
        let res = resp.read_handshake(&m3);
        assert!(res.is_err(), "PSK mismatch must fail at message 3");
    }

    #[test]
    fn rejects_payload_over_message_limit() {
        let s = KeyPair::generate();
        let c = KeyPair::generate();
        let mut i = NoiseInitiator::new(&c, &s.public, None).unwrap();
        let mut r = NoiseResponder::new(&s, None).unwrap();
        let m1 = i.write_handshake().unwrap();
        r.read_handshake(&m1).unwrap();
        let m2 = r.write_handshake().unwrap();
        i.read_handshake(&m2).unwrap();
        let m3 = i.write_handshake().unwrap();
        r.read_handshake(&m3).unwrap();
        let mut sess = i.finish().unwrap();
        let big = vec![0u8; NOISE_MAX_MSG_LEN]; // +16 tag pushes over
        let err = sess.encrypt(&big).unwrap_err();
        assert!(matches!(err, NoiseError::PayloadTooLarge));
    }
}
```

- [ ] **Step 2: Update `crates/shared/crypto/src/lib.rs`**

```rust
//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod identity;
pub mod noise;
pub mod redact;

pub use identity::{Identity, IdentityError, KeyBytes32, KeyPair};
pub use noise::{NoiseError, NoiseInitiator, NoiseResponder, NoiseSession};
pub use redact::Secret;
```

- [ ] **Step 3: Test**

Run: `cargo test -p cli-pocket-crypto`
Expected: 3 new Noise tests pass alongside identity + redact tests.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/crypto/src/noise.rs crates/shared/crypto/src/lib.rs
git commit -m "feat(crypto): add Noise XK wrapper with handshake + transport modes + PSK support"
```

---

## Task B12 — `crypto`: SPAKE2 wrapper

**Files:**
- Create: `crates/shared/crypto/src/spake2.rs`
- Modify: `crates/shared/crypto/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/crypto/src/spake2.rs`**

The RustCrypto `spake2` crate exposes `Ed25519Group` and a `Spake2::start_*` API. We wrap it so callers don't need to know which group constant goes with which side.

```rust
use spake2::{Ed25519Group, Identity as Spake2Identity, Password, Spake2};

#[derive(Debug, thiserror::Error)]
pub enum Spake2Error {
    #[error("spake2 finish failed (likely wrong password): {0}")]
    Finish(String),
}

pub struct Spake2Side {
    state: Option<Spake2<Ed25519Group>>,
    outbound: Vec<u8>,
}

impl Spake2Side {
    /// "Host" side of pair-code flow. `code` is the 6-digit string the user reads.
    pub fn start_host(code: &str, host_id_bytes: &[u8], client_hint: &[u8]) -> Self {
        let (state, msg) = Spake2::<Ed25519Group>::start_a(
            &Password::new(code.as_bytes()),
            &Spake2Identity::new(host_id_bytes),
            &Spake2Identity::new(client_hint),
        );
        Self {
            state: Some(state),
            outbound: msg,
        }
    }

    /// "Client" side of pair-code flow.
    pub fn start_client(code: &str, host_id_bytes: &[u8], client_hint: &[u8]) -> Self {
        let (state, msg) = Spake2::<Ed25519Group>::start_b(
            &Password::new(code.as_bytes()),
            &Spake2Identity::new(host_id_bytes),
            &Spake2Identity::new(client_hint),
        );
        Self {
            state: Some(state),
            outbound: msg,
        }
    }

    /// The wire message this side sends to the other.
    pub fn outbound(&self) -> &[u8] {
        &self.outbound
    }

    /// Feed the peer's outbound message; returns the shared secret on success.
    pub fn finish(self, peer_msg: &[u8]) -> Result<Spake2Outcome, Spake2Error> {
        let state = self.state.expect("state moved on finish");
        let shared = state
            .finish(peer_msg)
            .map_err(|e| Spake2Error::Finish(format!("{e:?}")))?;
        if shared.len() < 32 {
            return Err(Spake2Error::Finish("derived key too short".into()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&shared[..32]);
        Ok(Spake2Outcome { shared })
    }
}

pub struct Spake2Outcome {
    /// The full SPAKE2-derived shared secret. Callers feed this as the PSK
    /// into a subsequent Noise XKpsk2 handshake.
    pub shared: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_codes_agree() {
        let host = Spake2Side::start_host("493152", b"host", b"client-hint");
        let client = Spake2Side::start_client("493152", b"host", b"client-hint");
        let h_msg = host.outbound().to_vec();
        let c_msg = client.outbound().to_vec();
        let h_out = host.finish(&c_msg).unwrap();
        let c_out = client.finish(&h_msg).unwrap();
        assert_eq!(h_out.shared, c_out.shared);
        assert!(h_out.shared.len() >= 32);
    }

    #[test]
    fn mismatched_codes_yield_disagreeing_keys() {
        // SPAKE2 doesn't "fail" on wrong code — it produces a key the peer can't
        // match. The downstream Noise XKpsk2 handshake is what rejects the
        // mismatched key. So this test asserts inequality rather than error.
        let host = Spake2Side::start_host("493152", b"host", b"client-hint");
        let client = Spake2Side::start_client("000000", b"host", b"client-hint");
        let h_msg = host.outbound().to_vec();
        let c_msg = client.outbound().to_vec();
        let h_out = host.finish(&c_msg).unwrap();
        let c_out = client.finish(&h_msg).unwrap();
        assert_ne!(h_out.shared, c_out.shared);
    }
}
```

- [ ] **Step 2: Update `crates/shared/crypto/src/lib.rs`**

```rust
//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod identity;
pub mod noise;
pub mod redact;
pub mod spake2;

pub use identity::{Identity, IdentityError, KeyBytes32, KeyPair};
pub use noise::{NoiseError, NoiseInitiator, NoiseResponder, NoiseSession};
pub use redact::Secret;
pub use spake2::{Spake2Error, Spake2Outcome, Spake2Side};
```

- [ ] **Step 3: Test**

Run: `cargo test -p cli-pocket-crypto`
Expected: SPAKE2 round-trip + mismatch tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/crypto/src/spake2.rs crates/shared/crypto/src/lib.rs
git commit -m "feat(crypto): add SPAKE2 host/client wrapper"
```

---

## Task B13 — `transport`: `Transport` trait + `Frame` codec helper

**Files:**
- Modify: `crates/shared/transport/Cargo.toml`
- Modify: `crates/shared/transport/src/lib.rs`
- Create: `crates/shared/transport/src/transport.rs`

- [ ] **Step 1: Update `crates/shared/transport/Cargo.toml`**

```toml
[package]
name = "cli-pocket-transport"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "WebSocket transport abstraction for cli-pocket."

[dependencies]
cli-pocket-proto  = { path = "../proto" }
async-trait       = { workspace = true }
tokio             = { workspace = true }
tokio-tungstenite = { workspace = true }
futures-util      = { workspace = true }
thiserror         = { workspace = true }
tracing           = { workspace = true }
bytes             = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/shared/transport/src/transport.rs`**

```rust
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection closed")]
    Closed,
    #[error("io: {0}")]
    Io(String),
    #[error("websocket: {0}")]
    WebSocket(String),
}

/// Bidirectional binary-framed transport. Every send is one logical message;
/// every recv yields one logical message (or `None` on close).
#[async_trait]
pub trait Transport: Send + 'static {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError>;
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
    async fn close(&mut self) -> Result<(), TransportError>;
}
```

- [ ] **Step 3: Update `crates/shared/transport/src/lib.rs`**

```rust
//! Binary-framed WebSocket transport abstraction. See § Section 3 / § Section 6.

pub mod memory;
pub mod tokio_ws;
pub mod transport;

pub use memory::{InMemoryTransport, InMemoryTransportPair};
pub use tokio_ws::TokioWsTransport;
pub use transport::{Transport, TransportError};
```

- [ ] **Step 4: Build (it'll fail on missing memory/tokio_ws — that's fine, the next tasks add them)**

For commit cleanliness, write the lib.rs incrementally. Commit only the trait first:

```rust
//! Binary-framed WebSocket transport abstraction. See § Section 3 / § Section 6.

pub mod transport;
pub use transport::{Transport, TransportError};
```

- [ ] **Step 5: Test**

Run: `cargo build -p cli-pocket-transport`
Expected: success (no tests yet).

- [ ] **Step 6: Commit**

```bash
git add crates/shared/transport/Cargo.toml crates/shared/transport/src/lib.rs crates/shared/transport/src/transport.rs
git commit -m "feat(transport): add Transport trait + TransportError"
```

---

## Task B14 — `transport`: In-memory transport pair (test helper)

**Files:**
- Create: `crates/shared/transport/src/memory.rs`
- Modify: `crates/shared/transport/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/transport/src/memory.rs`**

```rust
use crate::transport::{Transport, TransportError};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// One half of an in-memory bidirectional transport pair. Used by tests
/// across crates to drive client↔daemon code without spinning up sockets.
pub struct InMemoryTransport {
    tx: mpsc::Sender<Vec<u8>>,
    rx: mpsc::Receiver<Vec<u8>>,
    closed: bool,
}

pub struct InMemoryTransportPair {
    pub a: InMemoryTransport,
    pub b: InMemoryTransport,
}

impl InMemoryTransportPair {
    pub fn new(buffer: usize) -> Self {
        let (atx, brx) = mpsc::channel(buffer);
        let (btx, arx) = mpsc::channel(buffer);
        Self {
            a: InMemoryTransport {
                tx: atx,
                rx: arx,
                closed: false,
            },
            b: InMemoryTransport {
                tx: btx,
                rx: brx,
                closed: false,
            },
        }
    }
}

#[async_trait]
impl Transport for InMemoryTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        self.tx
            .send(bytes)
            .await
            .map_err(|_| TransportError::Closed)
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        if self.closed {
            return Ok(None);
        }
        Ok(self.rx.recv().await)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.closed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pair_exchanges_payload() {
        let mut pair = InMemoryTransportPair::new(4);
        pair.a.send(vec![1, 2, 3]).await.unwrap();
        let got = pair.b.recv().await.unwrap();
        assert_eq!(got, Some(vec![1, 2, 3]));
    }

    #[tokio::test]
    async fn close_propagates_to_recv() {
        let mut pair = InMemoryTransportPair::new(4);
        drop(pair.a);
        let got = pair.b.recv().await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn one_megabyte_exchange_completes_in_under_a_second() {
        let mut pair = InMemoryTransportPair::new(16);
        let start = std::time::Instant::now();
        // 256 chunks * 4 KiB = 1 MiB
        let chunk = vec![0u8; 4096];
        let producer = tokio::spawn(async move {
            for _ in 0..256 {
                pair.a.send(chunk.clone()).await.unwrap();
            }
        });
        let mut received = 0usize;
        while received < 1024 * 1024 {
            let bytes = pair.b.recv().await.unwrap().unwrap();
            received += bytes.len();
        }
        producer.await.unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
}
```

- [ ] **Step 2: Update `crates/shared/transport/src/lib.rs`**

```rust
//! Binary-framed WebSocket transport abstraction. See § Section 3 / § Section 6.

pub mod memory;
pub mod transport;

pub use memory::{InMemoryTransport, InMemoryTransportPair};
pub use transport::{Transport, TransportError};
```

(We add the `tokio_ws` re-export in B15.)

- [ ] **Step 3: Test**

Run: `cargo test -p cli-pocket-transport`
Expected: 3 tests pass; the 1 MiB exchange completes <1 s.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/transport/src/memory.rs crates/shared/transport/src/lib.rs
git commit -m "feat(transport): add in-memory transport pair for cross-crate tests"
```

---

## Task B15 — `transport`: tokio-tungstenite WebSocket impl

**Files:**
- Create: `crates/shared/transport/src/tokio_ws.rs`
- Modify: `crates/shared/transport/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/transport/src/tokio_ws.rs`**

```rust
use crate::transport::{Transport, TransportError};
use async_trait::async_trait;
use futures_util::{sink::SinkExt, stream::StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct TokioWsTransport {
    ws: WsStream,
}

impl TokioWsTransport {
    pub fn new(ws: WsStream) -> Self {
        Self { ws }
    }

    /// Open an outbound WS connection. Used by clients.
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| TransportError::WebSocket(e.to_string()))?;
        Ok(Self { ws })
    }
}

#[async_trait]
impl Transport for TokioWsTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError> {
        self.ws
            .send(Message::Binary(bytes.into()))
            .await
            .map_err(|e| TransportError::WebSocket(e.to_string()))
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        loop {
            let next = self.ws.next().await;
            match next {
                None => return Ok(None),
                Some(Err(e)) => return Err(TransportError::WebSocket(e.to_string())),
                Some(Ok(Message::Binary(b))) => return Ok(Some(b.to_vec())),
                Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
                Some(Ok(Message::Text(_))) => {
                    return Err(TransportError::WebSocket(
                        "unexpected text frame on binary transport".into(),
                    ))
                }
            }
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.ws
            .close(None)
            .await
            .map_err(|e| TransportError::WebSocket(e.to_string()))
    }
}
```

- [ ] **Step 2: Update `crates/shared/transport/src/lib.rs`**

```rust
//! Binary-framed WebSocket transport abstraction. See § Section 3 / § Section 6.

pub mod memory;
pub mod tokio_ws;
pub mod transport;

pub use memory::{InMemoryTransport, InMemoryTransportPair};
pub use tokio_ws::TokioWsTransport;
pub use transport::{Transport, TransportError};
```

- [ ] **Step 3: Build (we don't add an integration test here — exercising a real WS server is Plan D/E concern)**

Run: `cargo build -p cli-pocket-transport`
Expected: success.

Run: `cargo test -p cli-pocket-transport`
Expected: existing in-memory tests still pass.

- [ ] **Step 4: Commit**

```bash
git add crates/shared/transport/src/tokio_ws.rs crates/shared/transport/src/lib.rs
git commit -m "feat(transport): add tokio-tungstenite TokioWsTransport"
```

---

## Task B16 — Workspace-wide validation + ADR 0003

**Files:**
- Create: `docs/superpowers/adr/0003-noise-xk-over-tls-only-trust-model.md`

- [ ] **Step 1: Write ADR 0003**

```markdown
# 0003. Noise XK over JSON+TLS-only trust model

Date: <fill at commit time>
Status: Accepted
Owners: <fill at commit time>

## Context

We needed end-to-end mutual authentication between the daemon and each
client, surviving a fully malicious relay. The alternatives were:

1. TLS only (mTLS) — relies on the public CA system or on issuing client
   certs out of band. Browsers also won't silently accept self-signed certs
   for LAN endpoints.
2. JSON message signatures over TLS — re-invents authenticated transport;
   nonce handling and replay are easy to get wrong.
3. Noise Protocol XK pattern over a plain transport (WS/TCP).

## Decision

Use Noise_XK_25519_ChaChaPoly_BLAKE2s (via the `snow` crate) as the
end-to-end channel. TLS, where present (relay, future web exposure), is
purely transport hygiene — it lets `wss://` survive proxies and corporate
networks. The trust model is Noise, end-to-end. A compromised TLS layer
cannot read or forge frames; it sees Noise ciphertext.

XK specifically because:
- K: the client knows the daemon's static public key from pairing.
- X: the daemon learns the client's static key during the handshake and
  checks it against `clients.json` for authorization.

A PSK option (XKpsk2) is available for self-hosted relays that want to gate
relay-level access. PSK is **not** part of the daemon-client trust model.

## Consequences

- Positive: the relay is forced to be zero-trust at the protocol level.
  Operators can run a relay without being trusted by their users.
- Positive: a stolen TLS cert (or a rogue corporate proxy) cannot decrypt
  terminal sessions.
- Positive: well-audited primitives; we never roll our own AEAD or KDF.
- Negative: the handshake is 3 roundtrips, adding ~1–2 RTT to the
  cold-connect time compared to immediate TLS app-data.
- Negative: revoking a client requires editing `clients.json` and waiting
  for the file watcher to pick up the change (Plan D detail). There is no
  cryptographic short-circuit revocation in v1.
- Risks accepted: side-channel attacks against `snow` / `spake2`. We rely
  on upstream review and do not roll our own primitives.
```

(The engineer fills in `Date:` with the current date and `Owners:` with their handle when committing.)

- [ ] **Step 2: Run the full workspace gates**

Run: `just check`
Expected: pass.

Run: `just test`
Expected: pass. Total tests across `proto` + `crypto` + `transport` should be ≥10 (Frame proptest counts as 1 test name but runs 256 cases).

- [ ] **Step 3: Verify wasm target still builds**

Run: `cargo build --target wasm32-unknown-unknown -p cli-pocket-proto`
Expected: success.

Run: `cargo build --target wasm32-unknown-unknown -p cli-pocket-crypto`
Expected: success. (`snow` with `default-resolver` builds to wasm; if it doesn't, switch to `default-resolver` off and add `ring-resolver` or `default-resolver-no-rand` — note this in handoff.)

`cli-pocket-transport` is **not** wasm-friendly because of `tokio::net::TcpStream`. That's expected — the wasm transport lives in `client-core-wasm` (Plan F).

- [ ] **Step 4: Commit ADR**

```bash
git add docs/superpowers/adr/0003-noise-xk-over-tls-only-trust-model.md
git commit -m "docs(adr): 0003 Noise XK over TLS-only trust model"
```

---

## Task B17 — Handoff note + proto-freeze tag

**Files:**
- Create: `docs/superpowers/handoff/B.md`

- [ ] **Step 1: Write `docs/superpowers/handoff/B.md`**

```markdown
# Handoff — Plan B (Shared contract layer)

Date completed: YYYY-MM-DD
Implementer: <name>

## What was built

### `cli-pocket-proto` (0.1.0)

Public API surface (re-exported from `lib.rs`):

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

- `Secret<T>` redaction wrapper (Debug/Display redact; Serialize preserves).
- `KeyPair`, `Identity`, `IdentityError`, `KeyBytes32` — file-mode-checked load/save (Unix 0600; Windows ACL is daemon-bin's job).
- `NoiseInitiator`, `NoiseResponder`, `NoiseSession` — Noise_XK_25519_ChaChaPoly_BLAKE2s with optional PSK at position 2.
- `Spake2Side`, `Spake2Outcome` — RustCrypto SPAKE2 over Ed25519.

### `cli-pocket-transport` (0.1.0)

- `Transport` trait (`send`, `recv`, `close`).
- `TokioWsTransport` — tokio-tungstenite native impl.
- `InMemoryTransport` / `InMemoryTransportPair` — test helper used by C, D, E, F.

## Deviations from spec

<list any. Examples that might show up:>
- snow version: <if not 0.9>
- Did wasm32 target build for crypto? If not, what feature flag was used.

## Open questions / follow-ups

- The web client's wasm transport is added by Plan F (uses `web-sys::WebSocket`).
- Frame compression (mentioned as out-of-scope in spec § 2) is not represented in the contract; if a future capability bit adds it, the codec module is the place.
- `chrono` is intentionally not a dep — `Identity::created_at` uses a hand-rolled RFC3339 formatter. If a downstream plan needs richer time handling, add `time` or `chrono` to workspace.dependencies and migrate.

## Validation

- `cargo test --workspace` — passes; ~10 named tests across the three crates.
- `cargo build --target wasm32-unknown-unknown -p cli-pocket-proto` — passes.
- `cargo build --target wasm32-unknown-unknown -p cli-pocket-crypto` — passes.
- `just check` — passes.

## Proto freeze

The tag `proto-v1.0.0-frozen` is created at the commit that completes this plan. Run:

```bash
git tag -a proto-v1.0.0-frozen -m "Freeze cli-pocket-proto and cli-pocket-crypto for downstream plans C/D/E/F"
git push --tags
```

After this tag, changes to `crates/shared/proto/**` or `crates/shared/crypto/**` require an ADR.
```

- [ ] **Step 2: Fill in the placeholders and commit**

```bash
git add docs/superpowers/handoff/B.md
git commit -m "docs: add Plan B handoff note"
```

- [ ] **Step 3: Tag the proto freeze**

```bash
git tag -a proto-v1.0.0-frozen -m "Freeze cli-pocket-proto and cli-pocket-crypto for downstream plans C/D/E/F"
```

Push when ready:

```bash
git push --tags
```

---

## Self-Review Checklist (run after Task B17)

1. **Spec coverage:**
   - § 2 Frame enum: B6 ✓
   - § 2 Hello / HelloOk / ResumeToken / Capabilities: B4 ✓
   - § 2 Snapshot / AnchorState: B5 ✓
   - § 2 Window flow control: B6 (Window variant of FrameBody) ✓
   - § 5 Identity file format + mode check: B10 ✓
   - § 5 Noise XK pattern: B11 ✓
   - § 5 SPAKE2 6-digit: B12 ✓
   - § 7 RelayCtrl + RelayData + discriminator: B7 ✓
   - § 3 ResumeToken: B4 (`ResumeToken`, `ResumeAttachment`) ✓
   - § 6 Transport trait (wasm-friendly shape): B13 ✓

2. **Placeholder scan:** no "TODO / TBD / fill in" outside the explicit `<name>` / `YYYY-MM-DD` fields in ADR 0003 and the handoff note.

3. **Type consistency:**
   - `StreamSeq` used identically in `proto::terminal`, `proto::snapshot::DeltaSlice`, `proto::frame::FrameBody::{Output, TerminalAttach, TerminalAttachOk}`, `proto::hello::ResumeAttachment`. ✓
   - `HostId` defined in `proto::terminal`, used in `proto::relay::RelayCtrl::{HostRegister, ClientPairRequest}` and `proto::relay::Endpoint::Relay`. ✓
   - `ProtocolError` defined once, used in `HelloErr`, `TerminalCreateErr`, `TerminalAttachErr`, `TerminalKillErr`, `ByeReason::ProtocolError`. ✓
   - `Snapshot::head_seq` aligns with `TerminalAttachOk::head_seq` (both `StreamSeq`). ✓

4. **No-`unsafe` invariant:** the workspace `[lints.rust] unsafe_code = "forbid"` still holds — none of these crates need unsafe. (The PTY crate in Plan C is the first one that overrides.)

5. **Wasm compat:** `proto` and `crypto` build to `wasm32-unknown-unknown`. `transport` does not, by design — its job is the native side.

If any check fails, fix inline and re-run `just check && just test` before tagging proto-v1.0.0-frozen.
