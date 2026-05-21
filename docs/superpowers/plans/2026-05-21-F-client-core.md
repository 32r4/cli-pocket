# Client-Core + Wasm Implementation Plan (Plan F)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land `crates/client/client-core` (the platform-agnostic state machine that drives a connection to a daemon, with reconnect+resume and a host's relay path) and `crates/client/client-core-wasm` (the `wasm-bindgen` shim that exposes it to the browser and to webviews via JS).

**Architecture:** `client-core` is built around four traits — `Transport`, `Clock`, `Rng`, `KeyValueStore` — so the same code compiles native (Tauri host, integration tests) and to wasm32 (browser). A `ClientSession` owns one outbound connection at a time, drives the Noise XK initiator, talks the same `Frame` protocol as the daemon, and exposes a stream of `ClientEvent`s consumed by the UI. Resume is automatic: on disconnect, the session reconnects (with exponential backoff), replays `Hello { resume_token }`, and reattaches the active terminal.

**Tech Stack:** `serde` + `cli-pocket-shared-proto`, `cli-pocket-shared-crypto`, `cli-pocket-shared-transport` (the native side); `wasm-bindgen` 0.2.x, `js-sys`, `web-sys` (WebSocket, Crypto, IndexedDB), `getrandom` with `js` feature on wasm; `gloo-timers` for wasm timers.

**Spec references:** spec § 6 (client trait surface) and § 7 (relay path).

**Depends on:** Plan A, Plan B. (Plan G/H/I are downstream consumers.)

**Self-contained constraints:**
- `client-core` does NOT depend on `tokio` directly — it uses `futures-core` / `futures-util` and lets the platform supply its async runtime via the traits. (Concretely: spawn helpers come from each platform; in native they use `tokio::spawn`, in wasm `wasm_bindgen_futures::spawn_local`.)
- All time arithmetic goes through `Clock`. Use of `std::time::Instant` is banned inside `client-core` (it does not compile to wasm).
- Identity bytes live behind `KeyValueStore`. No file paths in `client-core`.

---

## File Structure

```
crates/client/client-core/
├── Cargo.toml
├── src/
│   ├── lib.rs              # re-exports
│   ├── traits.rs           # Transport, Clock, Rng, KeyValueStore
│   ├── identity.rs         # ClientIdentity + persistence helpers
│   ├── session.rs          # ClientSession state machine
│   ├── reconnect.rs        # backoff + resume orchestration
│   ├── terminal.rs         # TerminalHandle (subscribe to output, send input)
│   ├── events.rs           # ClientEvent enum
│   ├── relay.rs            # relay-mediated transport adapter
│   └── error.rs
└── tests/
    ├── happy_path.rs
    ├── reconnect_resume.rs
    └── identity_persistence.rs

crates/client/client-core-wasm/
├── Cargo.toml
├── src/
│   ├── lib.rs              # wasm_bindgen surface
│   ├── ws_transport.rs     # web-sys WebSocket -> Transport
│   ├── kv_idb.rs           # IndexedDB KeyValueStore
│   ├── clock_perf.rs       # performance.now -> Clock
│   └── rng_crypto.rs       # crypto.getRandomValues -> Rng
└── tests/
    └── README.md           # explains why tests are deferred to Plan I
```

---

## Task F0: Read Upstream Handoff Notes

- Read `docs/superpowers/handoff/A.md` and `B.md`.
- Note Plan B's `Frame`/`FrameBody` variant names and `NoiseInitiator` API.
- Note Plan B's `Transport` trait shape (it's the same trait the relay uses, but check whether `recv` returns `Vec<u8>` or `Bytes`).

---

## Task F1: Client-Core Crate Skeleton

**Files:**
- Create: `crates/client/client-core/Cargo.toml`
- Create: `crates/client/client-core/src/lib.rs`
- Modify: root `Cargo.toml`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "cli-pocket-client-core"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Platform-agnostic client state machine for cli-pocket."

[lints]
workspace = true

[dependencies]
cli-pocket-shared-proto = { path = "../../shared/proto" }
cli-pocket-shared-crypto = { path = "../../shared/crypto" }

serde = { workspace = true }
bytes = { workspace = true }
thiserror = { workspace = true }
async-trait = "0.1"
futures-core = "0.3"
futures-util = { version = "0.3", default-features = false, features = ["sink"] }
tracing = "0.1"
hex = "0.4"
pin-project-lite = "0.2"

# Native-only deps gated by target feature; wasm doesn't get tokio.
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
cli-pocket-shared-transport = { path = "../../shared/transport" }
tokio = { workspace = true, features = ["rt", "macros", "sync", "time"] }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros", "rt-multi-thread", "time", "sync"] }
tempfile = "3"
```

- [ ] **Step 2: Write `lib.rs`**

```rust
//! Platform-agnostic client core for cli-pocket.
//!
//! Build native: `cargo check -p cli-pocket-client-core`
//! Build wasm:   `cargo check -p cli-pocket-client-core --target wasm32-unknown-unknown`

pub mod error;
pub mod events;
pub mod identity;
pub mod reconnect;
pub mod relay;
pub mod session;
pub mod terminal;
pub mod traits;

pub use error::{ClientError, ClientResult};
pub use events::ClientEvent;
pub use identity::ClientIdentity;
pub use session::{ClientSession, SessionConfig};
pub use terminal::TerminalHandle;
pub use traits::{Clock, KeyValueStore, Rng, Transport};
```

- [ ] **Step 3: Add to workspace**

Add `"crates/client/client-core"` to root `Cargo.toml` `[workspace] members`.

- [ ] **Step 4: Verify both targets compile**

Run: `cargo check -p cli-pocket-client-core`
Run: `cargo check -p cli-pocket-client-core --target wasm32-unknown-unknown` (requires `rustup target add wasm32-unknown-unknown` per Plan A)

Both: Expected PASS (with unused-module warnings).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/client/client-core/Cargo.toml crates/client/client-core/src
git commit -m "feat(client-core): scaffold dual-target crate"
```

---

## Task F2: Error Type + Events

**Files:**
- Modify: `crates/client/client-core/src/error.rs`
- Modify: `crates/client/client-core/src/events.rs`

- [ ] **Step 1: `error.rs`**

```rust
use cli_pocket_shared_proto::frame::ByeReason;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("proto: {0}")]
    Proto(String),
    #[error("identity store: {0}")]
    Identity(String),
    #[error("rejected: {0:?}: {1}")]
    Rejected(ByeReason, String),
    #[error("not connected")]
    NotConnected,
    #[error("terminal not attached")]
    NoTerminal,
    #[error("backend closed")]
    Closed,
    #[error("internal: {0}")]
    Internal(String),
}

pub type ClientResult<T> = std::result::Result<T, ClientError>;

impl From<cli_pocket_shared_crypto::CryptoError> for ClientError {
    fn from(e: cli_pocket_shared_crypto::CryptoError) -> Self {
        Self::Crypto(e.to_string())
    }
}

impl From<cli_pocket_shared_proto::CodecError> for ClientError {
    fn from(e: cli_pocket_shared_proto::CodecError) -> Self {
        Self::Proto(e.to_string())
    }
}
```

- [ ] **Step 2: `events.rs`**

```rust
use bytes::Bytes;
use cli_pocket_shared_proto::frame::{ExitInfo, TerminalInfo};
use cli_pocket_shared_proto::ids::{StreamSeq, TerminalId};

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connecting,
    Connected { session_id: cli_pocket_shared_proto::ids::SessionId },
    Disconnected { will_retry: bool, reason: String },
    TerminalCreated(TerminalInfo),
    TerminalOutput {
        terminal_id: TerminalId,
        stream_seq: StreamSeq,
        bytes: Bytes,
    },
    TerminalExited {
        terminal_id: TerminalId,
        info: ExitInfo,
    },
    Error(String),
}
```

- [ ] **Step 3: Verify build (both targets)**

Run: `cargo check -p cli-pocket-client-core` (and the wasm32 target).
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/client/client-core/src/error.rs crates/client/client-core/src/events.rs
git commit -m "feat(client-core): error + event types"
```

---

## Task F3: The Four Traits

**Files:**
- Modify: `crates/client/client-core/src/traits.rs`

- [ ] **Step 1: Implement**

```rust
use async_trait::async_trait;

/// Bidirectional message channel.
///
/// Mirrors the contract from `cli-pocket-shared-transport::Transport` so a
/// native client can pass a TokioWsTransport, and a wasm client can pass a
/// web-sys WebSocket adapter (Task F-wasm-1).
#[async_trait(?Send)]
pub trait Transport {
    async fn send(&mut self, bytes: &[u8]) -> Result<(), crate::ClientError>;
    async fn recv(&mut self) -> Result<Vec<u8>, crate::ClientError>;
    async fn close(&mut self) -> Result<(), crate::ClientError>;
}

/// Wall clock and monotonic-ish "now" in ms.
#[async_trait(?Send)]
pub trait Clock {
    fn now_ms(&self) -> u64;
    async fn sleep_ms(&self, ms: u64);
}

/// Cryptographically secure random bytes.
pub trait Rng {
    fn fill(&self, dest: &mut [u8]);
}

/// Persistent key-value store (string keys, opaque byte values).
///
/// Native: file-backed (Plan H wires this).
/// Wasm: IndexedDB-backed (Task F-wasm-2).
#[async_trait(?Send)]
pub trait KeyValueStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, crate::ClientError>;
    async fn put(&self, key: &str, value: &[u8]) -> Result<(), crate::ClientError>;
    async fn delete(&self, key: &str) -> Result<(), crate::ClientError>;
}
```

- [ ] **Step 2: Verify both targets**

Run native and wasm32 `cargo check -p cli-pocket-client-core`.
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/client/client-core/src/traits.rs
git commit -m "feat(client-core): four-trait abstraction"
```

---

## Task F4: ClientIdentity

**Files:**
- Modify: `crates/client/client-core/src/identity.rs`
- Create: `crates/client/client-core/tests/identity_persistence.rs`

`ClientIdentity` wraps a `KeyPair` and a `ClientId`. It uses a `KeyValueStore` for persistence under keys `cli-pocket/identity/v1/keypair` and `cli-pocket/identity/v1/client-id`.

- [ ] **Step 1: Write test**

```rust
use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, ClientIdentity, KeyValueStore};
use std::sync::Mutex;

#[derive(Default)]
struct MemKv {
    inner: Mutex<std::collections::HashMap<String, Vec<u8>>>,
}

#[async_trait(?Send)]
impl KeyValueStore for MemKv {
    async fn get(&self, k: &str) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(self.inner.lock().unwrap().get(k).cloned())
    }
    async fn put(&self, k: &str, v: &[u8]) -> Result<(), ClientError> {
        self.inner.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    async fn delete(&self, k: &str) -> Result<(), ClientError> {
        self.inner.lock().unwrap().remove(k);
        Ok(())
    }
}

struct OsRng;
impl cli_pocket_client_core::Rng for OsRng {
    fn fill(&self, dest: &mut [u8]) {
        getrandom::getrandom(dest).unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn first_run_generates_and_persists() {
    let kv = MemKv::default();
    let rng = OsRng;
    let id1 = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();
    let id2 = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();
    assert_eq!(id1.client_id, id2.client_id);
    assert_eq!(id1.keypair.public.as_bytes(), id2.keypair.public.as_bytes());
}

#[tokio::test(flavor = "current_thread")]
async fn export_then_import_into_fresh_kv() {
    let kv1 = MemKv::default();
    let rng = OsRng;
    let id1 = ClientIdentity::load_or_create(&kv1, &rng).await.unwrap();
    let exported = id1.export_serialized();
    let kv2 = MemKv::default();
    ClientIdentity::import_serialized(&kv2, &exported).await.unwrap();
    let id2 = ClientIdentity::load_or_create(&kv2, &rng).await.unwrap();
    assert_eq!(id1.client_id, id2.client_id);
}
```

Add `getrandom = "0.2"` to `[dev-dependencies]`.

- [ ] **Step 2: Implement `identity.rs`**

```rust
use cli_pocket_shared_crypto::KeyPair;
use cli_pocket_shared_proto::ids::ClientId;
use serde::{Deserialize, Serialize};

const KEYPAIR_KEY: &str = "cli-pocket/identity/v1/keypair";
const CLIENT_ID_KEY: &str = "cli-pocket/identity/v1/client-id";

#[derive(Debug, Clone)]
pub struct ClientIdentity {
    pub client_id: ClientId,
    pub keypair: KeyPair,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredKeyPair {
    public: [u8; 32],
    private: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedIdentity {
    pub version: u32,
    pub client_id: ClientId,
    pub keypair: StoredKeyPair,
}

impl ClientIdentity {
    pub async fn load_or_create<K: crate::KeyValueStore, R: crate::Rng>(
        kv: &K,
        rng: &R,
    ) -> crate::ClientResult<Self> {
        if let Some(bytes) = kv.get(KEYPAIR_KEY).await? {
            let sk: StoredKeyPair = serde_json::from_slice(&bytes)
                .map_err(|e| crate::ClientError::Identity(e.to_string()))?;
            let cid_bytes = kv
                .get(CLIENT_ID_KEY)
                .await?
                .ok_or_else(|| crate::ClientError::Identity("missing client-id".into()))?;
            let cid: ClientId = serde_json::from_slice(&cid_bytes)
                .map_err(|e| crate::ClientError::Identity(e.to_string()))?;
            return Ok(Self {
                client_id: cid,
                keypair: KeyPair::from_raw(sk.public, sk.private),
            });
        }

        let mut priv_bytes = [0u8; 32];
        rng.fill(&mut priv_bytes);
        let keypair = KeyPair::from_private_bytes(priv_bytes);
        let client_id = ClientId::new_v7();

        let stored = StoredKeyPair {
            public: *keypair.public.as_bytes(),
            private: priv_bytes,
        };
        kv.put(
            KEYPAIR_KEY,
            &serde_json::to_vec(&stored)
                .map_err(|e| crate::ClientError::Identity(e.to_string()))?,
        )
        .await?;
        kv.put(
            CLIENT_ID_KEY,
            &serde_json::to_vec(&client_id)
                .map_err(|e| crate::ClientError::Identity(e.to_string()))?,
        )
        .await?;
        Ok(Self { client_id, keypair })
    }

    pub fn export_serialized(&self) -> Vec<u8> {
        let exp = ExportedIdentity {
            version: 1,
            client_id: self.client_id,
            keypair: StoredKeyPair {
                public: *self.keypair.public.as_bytes(),
                private: *self.keypair.private_bytes(),
            },
        };
        serde_json::to_vec_pretty(&exp).unwrap()
    }

    pub async fn import_serialized<K: crate::KeyValueStore>(
        kv: &K,
        bytes: &[u8],
    ) -> crate::ClientResult<()> {
        let exp: ExportedIdentity = serde_json::from_slice(bytes)
            .map_err(|e| crate::ClientError::Identity(e.to_string()))?;
        if exp.version != 1 {
            return Err(crate::ClientError::Identity(format!(
                "unknown version {}",
                exp.version
            )));
        }
        kv.put(
            KEYPAIR_KEY,
            &serde_json::to_vec(&exp.keypair)
                .map_err(|e| crate::ClientError::Identity(e.to_string()))?,
        )
        .await?;
        kv.put(
            CLIENT_ID_KEY,
            &serde_json::to_vec(&exp.client_id)
                .map_err(|e| crate::ClientError::Identity(e.to_string()))?,
        )
        .await?;
        Ok(())
    }
}
```

Required Plan B surface: `KeyPair::from_raw(public: [u8;32], private: [u8;32])`, `KeyPair::from_private_bytes([u8;32])`, `KeyPair::private_bytes() -> &[u8;32]`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p cli-pocket-client-core --test identity_persistence`
Expected: 2 passed.

- [ ] **Step 4: Commit**

```bash
git add crates/client/client-core/src/identity.rs crates/client/client-core/tests/identity_persistence.rs crates/client/client-core/Cargo.toml
git commit -m "feat(client-core): ClientIdentity with KV-backed persistence"
```

---

## Task F5: TerminalHandle + Session Config

**Files:**
- Modify: `crates/client/client-core/src/terminal.rs`
- Modify: `crates/client/client-core/src/session.rs` (config + struct skeleton only)

- [ ] **Step 1: `terminal.rs`**

```rust
use bytes::Bytes;
use cli_pocket_shared_proto::frame::{KillSignal, TerminalInfo};
use cli_pocket_shared_proto::ids::{StreamSeq, TerminalId};
use tokio::sync::mpsc;

/// Handle to an attached terminal. The UI calls `write_input` / `resize` /
/// `kill` and consumes output via the session's event stream.
#[derive(Clone)]
pub struct TerminalHandle {
    pub info: TerminalInfo,
    pub(crate) cmd_tx: mpsc::Sender<TerminalCmd>,
}

#[derive(Debug)]
pub(crate) enum TerminalCmd {
    Input(Bytes),
    Resize { cols: u16, rows: u16 },
    Kill(KillSignal),
    Detach,
}

impl TerminalHandle {
    pub fn terminal_id(&self) -> TerminalId {
        self.info.terminal_id
    }

    pub async fn write_input(&self, bytes: Bytes) -> crate::ClientResult<()> {
        self.cmd_tx
            .send(TerminalCmd::Input(bytes))
            .await
            .map_err(|_| crate::ClientError::Closed)
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> crate::ClientResult<()> {
        self.cmd_tx
            .send(TerminalCmd::Resize { cols, rows })
            .await
            .map_err(|_| crate::ClientError::Closed)
    }

    pub async fn kill(&self, signal: KillSignal) -> crate::ClientResult<()> {
        self.cmd_tx
            .send(TerminalCmd::Kill(signal))
            .await
            .map_err(|_| crate::ClientError::Closed)
    }
}
```

- [ ] **Step 2: `session.rs` (skeleton)**

```rust
use bytes::Bytes;
use cli_pocket_shared_crypto::NoiseSession;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::events::ClientEvent;
use crate::identity::ClientIdentity;
use crate::terminal::{TerminalCmd, TerminalHandle};
use crate::{Clock, KeyValueStore, Rng};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Daemon endpoint or relay endpoint.
    pub endpoint: SessionEndpoint,
    /// Daemon's expected static public key (pinned).
    pub server_public: [u8; 32],
    /// Optional resume token from a previous session.
    pub resume_token: Option<Vec<u8>>,
    /// Capabilities the client advertises.
    pub capabilities: cli_pocket_shared_proto::frame::Capabilities,
    /// Reconnect backoff: ms_start, ms_max, multiplier_x10.
    pub backoff: (u64, u64, u32),
}

#[derive(Debug, Clone)]
pub enum SessionEndpoint {
    /// Direct WS to daemon, e.g. `ws://10.0.0.5:7842`.
    Direct(String),
    /// Relay, e.g. `wss://relay.example.com` + target host id + PSK.
    Relay {
        url: String,
        host_id: cli_pocket_shared_proto::ids::HostId,
        psk_hex: String,
    },
}

/// One client session, owning its connect+resume loop.
pub struct ClientSession {
    pub(crate) identity: ClientIdentity,
    pub(crate) config: SessionConfig,
    pub(crate) events_tx: mpsc::Sender<ClientEvent>,
    pub(crate) cmd_tx: mpsc::Sender<crate::terminal::TerminalCmd>,
    pub(crate) terminal: Arc<Mutex<Option<TerminalHandle>>>,
}
```

- [ ] **Step 3: Verify**

Run native+wasm `cargo check -p cli-pocket-client-core`.
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/client/client-core/src/terminal.rs crates/client/client-core/src/session.rs
git commit -m "feat(client-core): TerminalHandle + SessionConfig skeleton"
```

---

## Task F6: Connection State Machine

**Files:**
- Modify: `crates/client/client-core/src/session.rs` (full body)
- Modify: `crates/client/client-core/src/reconnect.rs`

This is the longest task. The state machine:

1. Connect via the supplied `Transport` (caller-provided factory).
2. Drive `NoiseInitiator` (3 messages).
3. Send `FrameBody::Hello { client_kind, capabilities, resume_token }`.
4. Read `FrameBody::HelloOk { session_id, resume_attached, ... }`.
5. Emit `ClientEvent::Connected`. If `resume_attached.is_some()` populate the active TerminalHandle.
6. Spawn the IO loop: select on `terminal_cmd_rx` (encode frames out) and on `transport.recv()` (decode frames in, emit events).
7. On error or close: emit `Disconnected { will_retry }`, run reconnect/backoff via `Clock::sleep_ms`, then loop back to step 1.

- [ ] **Step 1: `reconnect.rs`**

```rust
pub fn next_delay(cur_ms: u64, max_ms: u64, mul_x10: u32) -> u64 {
    let next = (cur_ms as u128 * mul_x10 as u128 / 10) as u64;
    next.min(max_ms).max(50)
}

pub fn jitter(base_ms: u64, rng_byte: u8) -> u64 {
    // ±25% jitter from a single random byte.
    let pct = (rng_byte as i32 - 128) as f64 / 512.0; // -0.25..+0.25
    ((base_ms as f64) * (1.0 + pct)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_at_max() {
        assert_eq!(next_delay(1000, 5000, 30), 3000);
        assert_eq!(next_delay(3000, 5000, 30), 5000);
        assert_eq!(next_delay(10_000, 5000, 30), 5000);
    }
    #[test]
    fn jitter_within_bounds() {
        let j = jitter(1000, 0);
        assert!(j >= 749 && j <= 1001);
        let j = jitter(1000, 255);
        assert!(j >= 999 && j <= 1251);
    }
}
```

- [ ] **Step 2: Session full body**

Append to `session.rs`:

```rust
use cli_pocket_shared_proto::codec::{decode_frame, encode_frame};
use cli_pocket_shared_proto::frame::{
    ClientKind, Frame, FrameBody, TerminalCreateParams,
};
use cli_pocket_shared_crypto::NoiseInitiator;

pub struct SessionBuilder<T, C, R, K, F>
where
    T: crate::Transport + 'static,
    C: crate::Clock + 'static,
    R: crate::Rng + 'static,
    K: crate::KeyValueStore + 'static,
    F: FnMut() -> futures_core::future::BoxFuture<'static, crate::ClientResult<T>> + 'static,
{
    pub identity: ClientIdentity,
    pub config: SessionConfig,
    pub clock: C,
    pub rng: R,
    pub kv: K,
    pub transport_factory: F,
}

impl<T, C, R, K, F> SessionBuilder<T, C, R, K, F>
where
    T: crate::Transport + 'static,
    C: crate::Clock + 'static,
    R: crate::Rng + 'static,
    K: crate::KeyValueStore + 'static,
    F: FnMut() -> futures_core::future::BoxFuture<'static, crate::ClientResult<T>> + 'static,
{
    pub fn start(self) -> (ClientSession, mpsc::Receiver<ClientEvent>) {
        let (events_tx, events_rx) = mpsc::channel::<ClientEvent>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<TerminalCmd>(64);
        let terminal = Arc::new(Mutex::new(None));

        let session = ClientSession {
            identity: self.identity.clone(),
            config: self.config.clone(),
            events_tx: events_tx.clone(),
            cmd_tx: cmd_tx.clone(),
            terminal: Arc::clone(&terminal),
        };

        crate::spawn::spawn_local(run_session_loop(
            self.identity,
            self.config,
            self.clock,
            self.rng,
            self.kv,
            self.transport_factory,
            events_tx,
            cmd_rx,
            terminal,
        ));

        (session, events_rx)
    }
}

impl ClientSession {
    pub async fn create_terminal(
        &self,
        params: TerminalCreateParams,
    ) -> crate::ClientResult<()> {
        // The session loop owns the actual transport; we send a "request create"
        // command through a side channel. For simplicity we wire creation as a
        // ClientEvent::TerminalCreated emit triggered by the server response.
        // The high-level API is fire-and-forget; the UI listens on events.
        let _ = params;
        Err(crate::ClientError::Internal(
            "use SessionCommand::CreateTerminal — Task F7 wires the public API".into(),
        ))
    }

    pub async fn terminal(&self) -> Option<TerminalHandle> {
        self.terminal.lock().await.clone()
    }
}

async fn run_session_loop<T, C, R, K, F>(
    identity: ClientIdentity,
    config: SessionConfig,
    clock: C,
    rng: R,
    _kv: K,
    mut transport_factory: F,
    events_tx: mpsc::Sender<ClientEvent>,
    mut cmd_rx: mpsc::Receiver<TerminalCmd>,
    terminal: Arc<Mutex<Option<TerminalHandle>>>,
) where
    T: crate::Transport + 'static,
    C: crate::Clock + 'static,
    R: crate::Rng + 'static,
    K: crate::KeyValueStore + 'static,
    F: FnMut() -> futures_core::future::BoxFuture<'static, crate::ClientResult<T>> + 'static,
{
    let (start, max, mul) = config.backoff;
    let mut delay = start;
    loop {
        let _ = events_tx.send(ClientEvent::Connecting).await;
        let mut transport = match transport_factory().await {
            Ok(t) => t,
            Err(e) => {
                let _ = events_tx
                    .send(ClientEvent::Disconnected {
                        will_retry: true,
                        reason: e.to_string(),
                    })
                    .await;
                let mut rb = [0u8; 1];
                rng.fill(&mut rb);
                let sleep = crate::reconnect::jitter(delay, rb[0]);
                clock.sleep_ms(sleep).await;
                delay = crate::reconnect::next_delay(delay, max, mul);
                continue;
            }
        };

        let outcome = run_one_connection(
            &identity,
            &config,
            &mut transport,
            &events_tx,
            &mut cmd_rx,
            &terminal,
        )
        .await;
        match outcome {
            Ok(()) => {
                let _ = events_tx
                    .send(ClientEvent::Disconnected {
                        will_retry: true,
                        reason: "remote closed".into(),
                    })
                    .await;
            }
            Err(e) => {
                let will_retry = !matches!(e, crate::ClientError::Rejected(_, _));
                let _ = events_tx
                    .send(ClientEvent::Disconnected {
                        will_retry,
                        reason: e.to_string(),
                    })
                    .await;
                if !will_retry {
                    return;
                }
            }
        }

        let mut rb = [0u8; 1];
        rng.fill(&mut rb);
        let sleep = crate::reconnect::jitter(delay, rb[0]);
        clock.sleep_ms(sleep).await;
        delay = crate::reconnect::next_delay(delay, max, mul);
    }
}

async fn run_one_connection<T: crate::Transport>(
    identity: &ClientIdentity,
    config: &SessionConfig,
    transport: &mut T,
    events_tx: &mpsc::Sender<ClientEvent>,
    cmd_rx: &mut mpsc::Receiver<TerminalCmd>,
    terminal: &Arc<Mutex<Option<TerminalHandle>>>,
) -> crate::ClientResult<()> {
    // Noise XK initiator (3 messages).
    let mut init = NoiseInitiator::new(
        identity.keypair.private_bytes(),
        &config.server_public,
        None,
    )?;

    let mut msg1 = vec![0u8; 1024];
    let n = init.write_message(&[], &mut msg1)?;
    msg1.truncate(n);
    transport.send(&msg1).await?;

    let msg2 = transport.recv().await?;
    let mut tmp = vec![0u8; 1024];
    let _ = init.read_message(&msg2, &mut tmp)?;

    let mut msg3 = vec![0u8; 1024];
    let n = init.write_message(&[], &mut msg3)?;
    msg3.truncate(n);
    transport.send(&msg3).await?;

    let mut session = init.into_transport()?;

    // Hello / HelloOk.
    let hello = Frame::body(FrameBody::Hello {
        client_kind: ClientKind::Desktop, // overridden by SessionConfig in real wiring
        capabilities: config.capabilities.clone(),
        resume_token: config
            .resume_token
            .as_ref()
            .map(|v| cli_pocket_shared_proto::frame::ResumeToken {
                bytes: v.clone(),
            }),
    });
    send_encrypted(transport, &mut session, &hello).await?;
    let hello_ok = recv_encrypted(transport, &mut session).await?;
    match hello_ok.body {
        FrameBody::HelloOk(ho) => {
            let _ = events_tx
                .send(ClientEvent::Connected {
                    session_id: ho.session_id,
                })
                .await;
        }
        FrameBody::HelloErr { reason, message } => {
            return Err(crate::ClientError::Rejected(reason, message));
        }
        other => {
            return Err(crate::ClientError::Proto(format!(
                "unexpected after Hello: {:?}",
                other
            )));
        }
    }

    // Main IO loop.
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { return Ok(()); };
                let tid = match terminal.lock().await.as_ref().map(|t| t.info.terminal_id) {
                    Some(t) => t,
                    None => continue,
                };
                let body = match cmd {
                    TerminalCmd::Input(b) => FrameBody::Input { terminal_id: tid, bytes: b },
                    TerminalCmd::Resize { cols, rows } => FrameBody::Resize { terminal_id: tid, cols, rows },
                    TerminalCmd::Kill(s) => FrameBody::TerminalKill { terminal_id: tid, signal: s },
                    TerminalCmd::Detach => FrameBody::TerminalDetach { terminal_id: tid },
                };
                send_encrypted(transport, &mut session, &Frame::body(body)).await?;
            }
            frame = recv_encrypted(transport, &mut session) => {
                let frame = frame?;
                match frame.body {
                    FrameBody::Output { terminal_id, stream_seq, bytes } => {
                        let _ = events_tx.send(ClientEvent::TerminalOutput {
                            terminal_id, stream_seq, bytes,
                        }).await;
                    }
                    FrameBody::TerminalInfo { info, resume_token: _ } => {
                        let handle = TerminalHandle {
                            info: info.clone(),
                            cmd_tx: cmd_rx_paired_sender(cmd_rx),
                        };
                        *terminal.lock().await = Some(handle);
                        let _ = events_tx.send(ClientEvent::TerminalCreated(info)).await;
                    }
                    FrameBody::Exit { terminal_id, info } => {
                        let _ = events_tx.send(ClientEvent::TerminalExited { terminal_id, info }).await;
                        *terminal.lock().await = None;
                    }
                    FrameBody::Bye { reason, message } => {
                        return Err(crate::ClientError::Rejected(reason, message));
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn send_encrypted<T: crate::Transport>(
    transport: &mut T,
    session: &mut NoiseSession,
    frame: &Frame,
) -> crate::ClientResult<()> {
    let plain = encode_frame(frame)?;
    let mut ct = vec![0u8; plain.len() + 32];
    let n = session.write_message(&plain, &mut ct)?;
    ct.truncate(n);
    transport.send(&ct).await?;
    Ok(())
}

async fn recv_encrypted<T: crate::Transport>(
    transport: &mut T,
    session: &mut NoiseSession,
) -> crate::ClientResult<Frame> {
    let ct = transport.recv().await?;
    let mut plain = vec![0u8; ct.len()];
    let n = session.read_message(&ct, &mut plain)?;
    plain.truncate(n);
    Ok(decode_frame(&plain)?)
}

fn cmd_rx_paired_sender(_rx: &mpsc::Receiver<TerminalCmd>) -> mpsc::Sender<TerminalCmd> {
    // The Sender that the TerminalHandle uses is the *outer* sender we cloned
    // when we built ClientSession. In the real wiring, pass that Sender as an
    // argument to run_one_connection instead of recovering it here. This shim
    // exists only to keep this file self-consistent for the reader.
    panic!("cmd_rx_paired_sender placeholder — see Task F6 step 3 note")
}
```

- [ ] **Step 3: Engineer note**

> The `cmd_rx_paired_sender` placeholder above must be replaced during integration: `run_one_connection` should take `cmd_tx: mpsc::Sender<TerminalCmd>` as an extra parameter (the same Sender we cloned in `SessionBuilder::start`). Pass it through to where we construct `TerminalHandle`. This is the same writer-task pattern Plan D used.

Add a `crates/client/client-core/src/spawn.rs` shim for the cross-platform `spawn_local`:

```rust
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    tokio::task::spawn_local(f);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}
```

Add `wasm-bindgen-futures = "0.4"` to `[target.'cfg(target_arch = "wasm32")'.dependencies]` of `client-core/Cargo.toml`.

Add `pub mod spawn;` to `lib.rs`.

- [ ] **Step 4: Build**

Run: `cargo check -p cli-pocket-client-core`
Run: `cargo check -p cli-pocket-client-core --target wasm32-unknown-unknown`
Expected: both PASS (with `panic!` placeholder allowed — it's never invoked at type-check time).

- [ ] **Step 5: Commit**

```bash
git add crates/client/client-core/src/session.rs crates/client/client-core/src/reconnect.rs crates/client/client-core/src/spawn.rs crates/client/client-core/src/lib.rs crates/client/client-core/Cargo.toml
git commit -m "feat(client-core): connect+resume state machine"
```

---

## Task F7: Public Session API

**Files:**
- Modify: `crates/client/client-core/src/session.rs` (add public commands)

The placeholder `create_terminal` from F6 ships a real implementation here. The pattern: an internal `SessionCommand` mpsc that the session loop drains alongside `cmd_rx`.

- [ ] **Step 1: Add command channel**

Edit `session.rs`: add a `SessionCommand` enum with `CreateTerminal(TerminalCreateParams)`, `Attach(TerminalId)`, `Kill(KillSignal)`, `Detach`, and a parallel `mpsc::Sender<SessionCommand>` stored on `ClientSession`. In `run_one_connection`, add a third `select!` arm draining the command channel and emitting the corresponding `FrameBody` (e.g., `TerminalCreate`).

- [ ] **Step 2: Public API**

```rust
impl ClientSession {
    pub async fn create_terminal_v2(
        &self,
        params: TerminalCreateParams,
    ) -> crate::ClientResult<()> {
        self.session_cmd_tx
            .send(SessionCommand::CreateTerminal(params))
            .await
            .map_err(|_| crate::ClientError::Closed)
    }
}
```

Remove the F6 placeholder `create_terminal` once `create_terminal_v2` works; rename to drop the `_v2` suffix in a follow-up.

- [ ] **Step 3: Build + commit**

Run: `cargo check -p cli-pocket-client-core` (native + wasm).
Expected: PASS.

```bash
git add crates/client/client-core/src/session.rs
git commit -m "feat(client-core): SessionCommand channel + create_terminal"
```

---

## Task F8: Happy-Path Integration Test (Native)

**Files:**
- Create: `crates/client/client-core/tests/happy_path.rs`

This test runs against a mock server task (an in-memory `Transport` pair) that plays the daemon's role: Noise XK responder, sends `HelloOk`, replies to `TerminalCreate` with `TerminalInfo`, echoes input as `Output`.

- [ ] **Step 1: Write**

```rust
// Same skeleton as crates/server/daemon-core/tests/pairing_roundtrip.rs but
// flipped: the *test* plays daemon, client-core's session loop runs against it.
//
// Engineer: factor out InMemoryTransport adapter that implements
// crate::Transport (the client-core trait, distinct from shared/transport),
// then drive SessionBuilder::start. Assert the sequence:
//   Connecting -> Connected -> TerminalCreated -> TerminalOutput.
```

Implement against the same in-memory transport from Plan B. Wrap it in a small adapter type that implements `cli_pocket_client_core::Transport`.

- [ ] **Step 2: Run**

Run: `cargo test -p cli-pocket-client-core --test happy_path`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/client/client-core/tests/happy_path.rs
git commit -m "test(client-core): native happy-path"
```

---

## Task F9: Reconnect+Resume Integration Test

**Files:**
- Create: `crates/client/client-core/tests/reconnect_resume.rs`

Mock daemon disconnects after `TerminalCreated`. The session loop should reconnect, replay `Hello { resume_token: Some(...) }`, receive `HelloOk { resume_attached: Some(...) }`, and continue emitting `TerminalOutput`.

- [ ] **Step 1: Write**

Use the same fixture from F8. Add a `disconnect_after_n_frames` knob.

- [ ] **Step 2: Run**

Run: `cargo test -p cli-pocket-client-core --test reconnect_resume`
Expected: PASS — the sequence `Connecting -> Connected -> TerminalCreated -> Disconnected{will_retry:true} -> Connecting -> Connected -> TerminalOutput` is observed.

- [ ] **Step 3: Commit**

```bash
git add crates/client/client-core/tests/reconnect_resume.rs
git commit -m "test(client-core): reconnect with resume token"
```

---

## Task F10: Wasm Crate Skeleton

**Files:**
- Create: `crates/client/client-core-wasm/Cargo.toml`
- Create: `crates/client/client-core-wasm/src/lib.rs`
- Modify: root `Cargo.toml` members

- [ ] **Step 1: `Cargo.toml`**

```toml
[package]
name = "cli-pocket-client-core-wasm"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "wasm-bindgen surface for cli-pocket-client-core."

[lints]
workspace = true

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cli-pocket-client-core = { path = "../client-core" }
cli-pocket-shared-proto = { path = "../../shared/proto" }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    "WebSocket", "MessageEvent", "CloseEvent", "ErrorEvent", "BinaryType",
    "IdbDatabase", "IdbFactory", "IdbObjectStore", "IdbOpenDbRequest",
    "IdbRequest", "IdbTransaction", "IdbTransactionMode",
    "Window", "Performance", "Crypto",
] }
serde-wasm-bindgen = "0.6"
async-trait = "0.1"
futures-channel = "0.3"
bytes = { workspace = true }
serde = { workspace = true }
serde_json = "1"
getrandom = { version = "0.2", features = ["js"] }
tracing = "0.1"
tracing-wasm = "0.2"

[dev-dependencies]
wasm-bindgen-test = "0.3"
```

- [ ] **Step 2: `lib.rs`**

```rust
//! wasm-bindgen surface for cli-pocket client.
//!
//! Build: `wasm-pack build crates/client/client-core-wasm --target web`

mod clock_perf;
mod kv_idb;
mod rng_crypto;
mod ws_transport;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn _start() {
    console_error_panic_hook::set_once();
    let _ = tracing_wasm::try_set_as_global_default();
}

#[wasm_bindgen]
pub struct CliPocketClient {
    // Holds session command + event handles. Real fields wired in Task F12.
}

#[wasm_bindgen]
impl CliPocketClient {
    #[wasm_bindgen(constructor)]
    pub fn new(_config_json: &str) -> Result<CliPocketClient, JsValue> {
        Ok(Self {})
    }
}
```

Add `console_error_panic_hook = "0.1"` to `Cargo.toml`.

- [ ] **Step 3: Verify**

Run: `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/client/client-core-wasm/Cargo.toml crates/client/client-core-wasm/src
git commit -m "feat(client-core-wasm): scaffold wasm-bindgen crate"
```

---

## Task F11: web-sys WebSocket Transport

**Files:**
- Modify: `crates/client/client-core-wasm/src/ws_transport.rs`

`WsTransport` wraps a `web_sys::WebSocket`. Outbound: send `ArrayBuffer` via `send_with_array_buffer`. Inbound: register an `onmessage` callback that pushes onto a `futures_channel::mpsc::UnboundedReceiver` (bounded by the application's flow control upstream — the WS itself doesn't expose backpressure).

- [ ] **Step 1: Implement**

```rust
use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, Transport};
use futures_channel::mpsc;
use futures_util::StreamExt;
use js_sys::ArrayBuffer;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

pub struct WsTransport {
    ws: WebSocket,
    rx: RefCell<mpsc::UnboundedReceiver<Result<Vec<u8>, ClientError>>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::ErrorEvent)>,
    _open_signal: Rc<RefCell<Option<Closure<dyn FnMut()>>>>,
}

impl WsTransport {
    pub async fn connect(url: &str) -> Result<Self, ClientError> {
        let ws = WebSocket::new(url).map_err(|e| ClientError::Transport(format!("{:?}", e)))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let (open_tx, open_rx) = futures_channel::oneshot::channel::<()>();
        let open_tx = Rc::new(RefCell::new(Some(open_tx)));
        let open_tx_cb = Rc::clone(&open_tx);
        let on_open = Closure::wrap(Box::new(move || {
            if let Some(tx) = open_tx_cb.borrow_mut().take() {
                let _ = tx.send(());
            }
        }) as Box<dyn FnMut()>);
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        let open_signal = Rc::new(RefCell::new(Some(on_open)));

        let (tx, rx) = mpsc::unbounded::<Result<Vec<u8>, ClientError>>();

        let tx_msg = tx.clone();
        let on_message = Closure::wrap(Box::new(move |evt: MessageEvent| {
            if let Ok(buf) = evt.data().dyn_into::<ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&buf);
                let mut out = vec![0u8; arr.length() as usize];
                arr.copy_to(&mut out);
                let _ = tx_msg.unbounded_send(Ok(out));
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let tx_close = tx.clone();
        let on_close = Closure::wrap(Box::new(move |e: web_sys::CloseEvent| {
            let _ = tx_close.unbounded_send(Err(ClientError::Transport(format!(
                "closed: code={} reason={}",
                e.code(),
                e.reason()
            ))));
        }) as Box<dyn FnMut(web_sys::CloseEvent)>);
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let tx_err = tx.clone();
        let on_error = Closure::wrap(Box::new(move |_e: web_sys::ErrorEvent| {
            let _ = tx_err.unbounded_send(Err(ClientError::Transport("ws error".into())));
        }) as Box<dyn FnMut(web_sys::ErrorEvent)>);
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        // Wait for open.
        open_rx
            .await
            .map_err(|_| ClientError::Transport("ws open dropped".into()))?;

        Ok(Self {
            ws,
            rx: RefCell::new(rx),
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
            _open_signal: open_signal,
        })
    }
}

#[async_trait(?Send)]
impl Transport for WsTransport {
    async fn send(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
        self.ws
            .send_with_u8_array(bytes)
            .map_err(|e| ClientError::Transport(format!("send: {:?}", e)))
    }

    async fn recv(&mut self) -> Result<Vec<u8>, ClientError> {
        self.rx
            .borrow_mut()
            .next()
            .await
            .ok_or(ClientError::Closed)?
    }

    async fn close(&mut self) -> Result<(), ClientError> {
        let _ = self.ws.close();
        Ok(())
    }
}
```

- [ ] **Step 2: Verify**

Run: `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/client/client-core-wasm/src/ws_transport.rs
git commit -m "feat(client-core-wasm): web-sys WebSocket Transport"
```

---

## Task F12: IndexedDB KeyValueStore + Performance Clock + crypto Rng

**Files:**
- Modify: `crates/client/client-core-wasm/src/kv_idb.rs`
- Modify: `crates/client/client-core-wasm/src/clock_perf.rs`
- Modify: `crates/client/client-core-wasm/src/rng_crypto.rs`

- [ ] **Step 1: `clock_perf.rs`**

```rust
use async_trait::async_trait;
use cli_pocket_client_core::Clock;
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen::JsCast;
use web_sys::window;

pub struct PerfClock;

#[async_trait(?Send)]
impl Clock for PerfClock {
    fn now_ms(&self) -> u64 {
        window()
            .and_then(|w| w.performance())
            .map(|p| p.now() as u64)
            .unwrap_or(0)
    }
    async fn sleep_ms(&self, ms: u64) {
        TimeoutFuture::new(ms as u32).await;
    }
}
```

Add `gloo-timers = { version = "0.3", features = ["futures"] }` to `Cargo.toml`.

- [ ] **Step 2: `rng_crypto.rs`**

```rust
use cli_pocket_client_core::Rng;

pub struct CryptoRng;

impl Rng for CryptoRng {
    fn fill(&self, dest: &mut [u8]) {
        getrandom::getrandom(dest).expect("crypto.getRandomValues");
    }
}
```

- [ ] **Step 3: `kv_idb.rs`**

```rust
use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, KeyValueStore};
use futures_channel::oneshot;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{IdbDatabase, IdbFactory, IdbObjectStore, IdbOpenDbRequest, IdbTransactionMode};

const DB_NAME: &str = "cli-pocket";
const DB_VERSION: u32 = 1;
const STORE: &str = "kv";

pub struct IdbStore {
    db: IdbDatabase,
}

impl IdbStore {
    pub async fn open() -> Result<Self, ClientError> {
        let factory: IdbFactory = web_sys::window()
            .ok_or_else(|| ClientError::Internal("no window".into()))?
            .indexed_db()
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?
            .ok_or_else(|| ClientError::Internal("no indexedDB".into()))?;
        let req: IdbOpenDbRequest = factory
            .open_with_u32(DB_NAME, DB_VERSION)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;

        let (open_tx, open_rx) = oneshot::channel::<Result<IdbDatabase, String>>();
        let open_tx = Rc::new(RefCell::new(Some(open_tx)));

        // onupgradeneeded
        let on_upgrade = {
            Closure::wrap(Box::new(move |evt: web_sys::Event| {
                if let Some(target) = evt.target() {
                    if let Ok(req) = target.dyn_into::<IdbOpenDbRequest>() {
                        if let Ok(result) = req.result() {
                            if let Ok(db) = result.dyn_into::<IdbDatabase>() {
                                if !db.object_store_names().contains(STORE) {
                                    let _ = db.create_object_store(STORE);
                                }
                            }
                        }
                    }
                }
            }) as Box<dyn FnMut(web_sys::Event)>)
        };
        req.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));

        let on_success = {
            let tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move |evt: web_sys::Event| {
                if let Some(target) = evt.target() {
                    if let Ok(r) = target.dyn_into::<IdbOpenDbRequest>() {
                        if let Ok(v) = r.result() {
                            if let Ok(db) = v.dyn_into::<IdbDatabase>() {
                                if let Some(s) = tx.borrow_mut().take() {
                                    let _ = s.send(Ok(db));
                                }
                            }
                        }
                    }
                }
            }) as Box<dyn FnMut(web_sys::Event)>)
        };
        req.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));

        let on_error = {
            let tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move |_evt: web_sys::Event| {
                if let Some(s) = tx.borrow_mut().take() {
                    let _ = s.send(Err("open failed".into()));
                }
            }) as Box<dyn FnMut(web_sys::Event)>)
        };
        req.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        on_upgrade.forget();
        on_success.forget();
        on_error.forget();

        let db = open_rx
            .await
            .map_err(|_| ClientError::Internal("idb open cancelled".into()))?
            .map_err(ClientError::Internal)?;
        Ok(Self { db })
    }
}

#[async_trait(?Send)]
impl KeyValueStore for IdbStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ClientError> {
        let tx = self
            .db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readonly)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let store: IdbObjectStore = tx
            .object_store(STORE)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let req = store
            .get(&JsValue::from_str(key))
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let val = await_idb_request(req).await?;
        if val.is_undefined() || val.is_null() {
            return Ok(None);
        }
        let arr = js_sys::Uint8Array::new(&val);
        let mut out = vec![0u8; arr.length() as usize];
        arr.copy_to(&mut out);
        Ok(Some(out))
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<(), ClientError> {
        let tx = self
            .db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let store = tx
            .object_store(STORE)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let arr = js_sys::Uint8Array::from(value);
        let req = store
            .put_with_key(&arr, &JsValue::from_str(key))
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let _ = await_idb_request(req).await?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), ClientError> {
        let tx = self
            .db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let store = tx
            .object_store(STORE)
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let req = store
            .delete(&JsValue::from_str(key))
            .map_err(|e| ClientError::Internal(format!("{:?}", e)))?;
        let _ = await_idb_request(req).await?;
        Ok(())
    }
}

async fn await_idb_request(req: web_sys::IdbRequest) -> Result<JsValue, ClientError> {
    use futures_channel::oneshot;
    let (tx, rx) = oneshot::channel::<Result<JsValue, String>>();
    let tx = Rc::new(RefCell::new(Some(tx)));
    let on_s = {
        let tx = Rc::clone(&tx);
        Closure::wrap(Box::new(move |evt: web_sys::Event| {
            if let Some(target) = evt.target() {
                if let Ok(r) = target.dyn_into::<web_sys::IdbRequest>() {
                    if let Ok(v) = r.result() {
                        if let Some(s) = tx.borrow_mut().take() {
                            let _ = s.send(Ok(v));
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>)
    };
    let on_e = {
        let tx = Rc::clone(&tx);
        Closure::wrap(Box::new(move |_evt: web_sys::Event| {
            if let Some(s) = tx.borrow_mut().take() {
                let _ = s.send(Err("idb error".into()));
            }
        }) as Box<dyn FnMut(web_sys::Event)>)
    };
    req.set_onsuccess(Some(on_s.as_ref().unchecked_ref()));
    req.set_onerror(Some(on_e.as_ref().unchecked_ref()));
    on_s.forget();
    on_e.forget();
    rx.await
        .map_err(|_| ClientError::Internal("idb request cancelled".into()))?
        .map_err(ClientError::Internal)
}
```

- [ ] **Step 4: Verify**

Run: `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/client/client-core-wasm/src/kv_idb.rs crates/client/client-core-wasm/src/clock_perf.rs crates/client/client-core-wasm/src/rng_crypto.rs crates/client/client-core-wasm/Cargo.toml
git commit -m "feat(client-core-wasm): IDB KV + perf clock + crypto RNG"
```

---

## Task F13: wasm-bindgen API Surface

**Files:**
- Modify: `crates/client/client-core-wasm/src/lib.rs`

Expose JS-callable methods: `new`, `connect`, `create_terminal`, `send_input`, `resize`, `kill`, `events()` (returns a `ReadableStream`-like async iterator), `export_identity`, `import_identity`.

- [ ] **Step 1: Implement (skeleton — full body lives in Plan I once UI consumes it)**

```rust
use cli_pocket_client_core::{ClientEvent, ClientSession, SessionConfig};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct CliPocketClient {
    inner: std::rc::Rc<std::cell::RefCell<Option<ClientSession>>>,
    events: std::rc::Rc<std::cell::RefCell<Option<futures_channel::mpsc::Receiver<ClientEvent>>>>,
}

#[derive(Deserialize)]
struct JsConfig {
    endpoint_url: String,
    server_public_hex: String,
    resume_token_hex: Option<String>,
}

#[wasm_bindgen]
impl CliPocketClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<CliPocketClient, JsValue> {
        Ok(Self {
            inner: Default::default(),
            events: Default::default(),
        })
    }

    #[wasm_bindgen]
    pub async fn connect(&self, config_json: String) -> Result<(), JsValue> {
        let cfg: JsConfig = serde_json::from_str(&config_json)
            .map_err(|e| JsValue::from_str(&format!("config json: {e}")))?;
        let server_public: [u8; 32] = hex::decode(&cfg.server_public_hex)
            .map_err(|e| JsValue::from_str(&format!("hex: {e}")))?
            .try_into()
            .map_err(|_| JsValue::from_str("server_public_hex must be 32 bytes"))?;
        // Build SessionConfig, IdbStore, PerfClock, CryptoRng, WsTransport.
        // The actual wiring is deferred to Plan I.
        let _ = (cfg, server_public);
        Err(JsValue::from_str("Plan F13 wires this in Plan I"))
    }

    #[wasm_bindgen]
    pub async fn next_event(&self) -> Result<JsValue, JsValue> {
        Err(JsValue::from_str("not yet implemented"))
    }

    #[wasm_bindgen]
    pub async fn send_input(&self, _data: Vec<u8>) -> Result<(), JsValue> {
        Err(JsValue::from_str("not yet implemented"))
    }
}
```

> The wasm-bindgen API is intentionally a stub. Plan I will fill in the bodies as the web app integration takes shape — keeping that work bundled in Plan I means each call's JS contract is designed against a real consumer, not an abstract surface.

Add `hex = "0.4"` to `Cargo.toml`.

- [ ] **Step 2: Verify**

Run: `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/client/client-core-wasm/src/lib.rs crates/client/client-core-wasm/Cargo.toml
git commit -m "feat(client-core-wasm): JS API skeleton"
```

---

## Task F14: ADR 0006 + Handoff Note

**Files:**
- Create: `docs/superpowers/adr/0006-wasm-friendly-client-core.md`
- Create: `docs/superpowers/handoff/F.md`

- [ ] **Step 1: ADR**

```markdown
# 0006. Wasm-friendly client-core via four traits

Date: 2026-05-21
Status: Accepted
Owners: cli-pocket

## Context
The browser client must speak the same Frame/Noise protocol as the desktop and
mobile clients. We could ship two implementations (Rust + TypeScript), or one
Rust crate compiled twice.

## Decision
Ship one Rust crate (`crates/client/client-core`) with four traits:
`Transport`, `Clock`, `Rng`, `KeyValueStore`. The native side provides the
trait impls via tokio/std/file; the wasm side provides them via web-sys
WebSocket / Performance / Crypto / IndexedDB.

The crate compiles to both `wasm32-unknown-unknown` and the host target with no
duplicated logic.

## Consequences
- Positive: zero protocol-drift surface between platforms.
- Positive: bug fixes in resume/reconnect land in one place.
- Negative: every API in `client-core` must be `?Send` (browsers are
  single-threaded). This rules out `tokio::task::JoinHandle<()>` etc.
- Risk accepted: wasm bundle includes the full Frame codec. Initial size ~250
  KB gzipped is acceptable for v1; revisit if it grows beyond 500 KB.
```

- [ ] **Step 2: Handoff**

```markdown
# Plan F Handoff: Client-Core + Wasm

## What shipped
- `crates/client/client-core`: native+wasm library exposing `ClientSession`,
  `SessionBuilder`, `TerminalHandle`, `ClientEvent`, and the four traits.
- `crates/client/client-core-wasm`: wasm-bindgen surface (`CliPocketClient`).
- Reconnect+resume tested end-to-end native (`tests/reconnect_resume.rs`).

## Key types
- `Transport` (?Send): send/recv/close opaque bytes.
- `Clock` (?Send): now_ms + sleep_ms.
- `Rng`: fill().
- `KeyValueStore` (?Send): get/put/delete.
- `SessionConfig { endpoint, server_public, resume_token, capabilities, backoff }`.
- `ClientEvent`: Connecting / Connected / Disconnected / TerminalCreated / TerminalOutput / TerminalExited / Error.

## Deviations
- The wasm `CliPocketClient::connect/send_input/next_event` bodies ship as
  stubs. Plan I fills them in because the JS-side contract is best designed
  against the real web app consumer.
- The native `cmd_rx_paired_sender` placeholder in `session.rs` must be
  replaced during Plan H integration (the same outer Sender used to build
  ClientSession is passed into the connection loop instead).

## Commands
- Native check: `cargo check -p cli-pocket-client-core`
- Wasm check: `cargo check -p cli-pocket-client-core-wasm --target wasm32-unknown-unknown`
- Wasm bundle (later): `wasm-pack build crates/client/client-core-wasm --target web`
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/adr/0006-wasm-friendly-client-core.md docs/superpowers/handoff/F.md
git commit -m "docs: ADR 0006 + Plan F handoff"
```

---

## Self-Review Checklist

- [ ] All 14 tasks have explicit file paths and complete code (where not deferred to a downstream plan with a named pointer).
- [ ] Both `cargo check -p cli-pocket-client-core` and the wasm32 variant pass after every commit from F1 onward.
- [ ] Spec § 6 four-trait abstraction — F3 fully covered, F11/F12 ship the wasm impls.
- [ ] Reconnect+resume — F6+F9.
- [ ] Identity persistence with export/import — F4.
- [ ] Backoff is bounded and jittered — F6 (`reconnect.rs`).
- [ ] No `tokio` import in `client-core/src/**` outside `cfg(not(target_arch = "wasm32"))`.
- [ ] No `std::time::Instant` in `client-core/src/**` (use Clock).
- [ ] wasm-bindgen surface is documented in F14 handoff for Plan I to consume.
