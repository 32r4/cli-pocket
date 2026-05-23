//! Pair management. Plan E5 ships the concrete `Pair`/`PairManager` types;
//! Plan E7 wires them into the [`crate::guillotine`] sweeper traits so the
//! background sweep operates on the real registry.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use cli_pocket_proto::{HostId, PairId};
use futures_util::future::{BoxFuture, FutureExt};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::guillotine::{PairHandle, PairSweep};

/// Per-pair routed payload. `HostToClient` / `ClientToHost` carry already-
/// encoded `RelayData` ciphertext frames; `Close` signals the writer task
/// should flush a `PairClose` and shut its side of the pair down.
#[derive(Debug)]
pub enum PairMsg {
    /// Ciphertext flowing host -> client.
    HostToClient(Bytes),
    /// Ciphertext flowing client -> host.
    ClientToHost(Bytes),
    /// Tear-down signal carrying a static reason tag.
    Close(&'static str),
}

/// Convenience bundle returned by the pair-opening path so callers can stash
/// the two writer-task senders together.
pub struct PairEnds {
    pub to_host: mpsc::Sender<PairMsg>,
    pub to_client: mpsc::Sender<PairMsg>,
}

/// A live pair. Identifies the participants and exposes the per-side senders
/// the forwarder writes into. `last_progress` is consulted by the guillotine
/// (Task E6) to evict stuck pairs.
pub struct Pair {
    pub pair_id: PairId,
    pub host_id: HostId,
    pub created_at: Instant,
    pub last_progress: Mutex<Instant>,
    /// Sender into the host-WS writer task (Data direction).
    pub host_tx: mpsc::Sender<PairMsg>,
    /// Sender into the client-WS writer task (Data direction).
    pub client_tx: mpsc::Sender<PairMsg>,
}

impl Pair {
    /// Build a new pair record. The forwarder owns the matching receivers and
    /// the WS writer tasks they feed.
    #[must_use]
    pub fn new(
        pair_id: PairId,
        host_id: HostId,
        host_tx: mpsc::Sender<PairMsg>,
        client_tx: mpsc::Sender<PairMsg>,
    ) -> Self {
        let now = Instant::now();
        Self {
            pair_id,
            host_id,
            created_at: now,
            last_progress: Mutex::new(now),
            host_tx,
            client_tx,
        }
    }

    /// Stamp the last-progress marker. Called by the forwarder whenever a
    /// data frame is successfully routed in either direction.
    pub fn touch(&self) {
        *self.last_progress.lock() = Instant::now();
    }

    /// Read the last-progress marker without disturbing it.
    #[must_use]
    pub fn last_progress(&self) -> Instant {
        *self.last_progress.lock()
    }
}

/// Registry of live pairs keyed by `PairId`. Cheap to clone — the inner map
/// is shared behind an `Arc<Mutex<...>>`.
#[derive(Default, Clone)]
pub struct PairManager {
    inner: Arc<Mutex<HashMap<PairId, Arc<Pair>>>>,
}

impl PairManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a pair. Returns the previously-stored entry, if any.
    pub fn insert(&self, pair: Arc<Pair>) -> Option<Arc<Pair>> {
        self.inner.lock().insert(pair.pair_id, pair)
    }

    /// Look up a pair by id.
    #[must_use]
    pub fn get(&self, pid: &PairId) -> Option<Arc<Pair>> {
        self.inner.lock().get(pid).cloned()
    }

    /// Remove and return a pair by id.
    #[must_use]
    pub fn remove(&self, pid: &PairId) -> Option<Arc<Pair>> {
        self.inner.lock().remove(pid)
    }

    /// Snapshot the current pair list. Used by the guillotine sweep so the
    /// inner lock is not held while scanning.
    #[must_use]
    pub fn list_for_sweep(&self) -> Vec<Arc<Pair>> {
        self.inner.lock().values().cloned().collect()
    }

    /// Clone a handle pointing at the same underlying map.
    #[must_use]
    pub fn clone_handle(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Return an `Instant` suitable for stamping a fresh progress marker. Exists
/// so callers in tests and the guillotine share a single time source.
#[must_use]
pub fn now_marker() -> Instant {
    Instant::now()
}

impl PairHandle for Pair {
    fn pair_id(&self) -> PairId {
        self.pair_id
    }

    fn last_progress(&self) -> Instant {
        Pair::last_progress(self)
    }

    fn close_stuck(self: Arc<Self>) -> BoxFuture<'static, ()> {
        async move {
            // Best-effort: a full or closed channel means the peer is already
            // gone, so we just drop the signal silently.
            let _ = self.host_tx.send(PairMsg::Close("stuck")).await;
            let _ = self.client_tx.send(PairMsg::Close("stuck")).await;
        }
        .boxed()
    }
}

impl PairSweep for PairManager {
    type Pair = Pair;

    fn list_for_sweep(&self) -> Vec<Arc<Pair>> {
        PairManager::list_for_sweep(self)
    }

    fn remove(&self, pair_id: &PairId) {
        let _ = PairManager::remove(self, pair_id);
    }
}
