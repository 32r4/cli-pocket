//! Integration tests for the guillotine sweeper.
//!
//! Plan E task E6 ships the sweeper before task E5 lands the concrete
//! `Pair` / `PairManager`. So these tests use a hand-rolled mock that
//! implements the `PairHandle` / `PairSweep` traits exposed by
//! `guillotine.rs`. When E7 wires the real types together, an additional
//! end-to-end test will cover the integration; the unit-level invariants
//! here stay valid because they pin the guillotine's contract.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cli_pocket_proto::PairId;
use cli_pocket_relay_core::guillotine::{run, sweep_once, PairHandle, PairSweep};
use futures_util::future::{BoxFuture, FutureExt};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Signal emitted by a [`MockPair`] when the guillotine closes it.
#[derive(Debug)]
struct CloseSignal {
    pair_id: PairId,
}

/// Test double for the real `Pair` type. Records its own `last_progress`
/// and forwards a single `CloseSignal` to a shared channel when
/// `close_stuck` fires.
struct MockPair {
    pair_id: PairId,
    last_progress: Mutex<Instant>,
    close_tx: mpsc::Sender<CloseSignal>,
}

impl MockPair {
    fn new(close_tx: mpsc::Sender<CloseSignal>, last_progress: Instant) -> Arc<Self> {
        Arc::new(Self {
            pair_id: PairId(Uuid::now_v7()),
            last_progress: Mutex::new(last_progress),
            close_tx,
        })
    }

    fn touch(&self, at: Instant) {
        *self.last_progress.lock() = at;
    }
}

impl PairHandle for MockPair {
    fn pair_id(&self) -> PairId {
        self.pair_id
    }

    fn last_progress(&self) -> Instant {
        *self.last_progress.lock()
    }

    fn close_stuck(self: Arc<Self>) -> BoxFuture<'static, ()> {
        async move {
            // Best-effort: if the receiver is dropped the test has bugs but
            // the production code must not panic.
            let _ = self
                .close_tx
                .send(CloseSignal {
                    pair_id: self.pair_id,
                })
                .await;
        }
        .boxed()
    }
}

/// Test double for `PairManager`. Wraps an `Arc<Mutex<HashMap<...>>>`
/// so `Clone` produces shared state, mirroring the real manager's
/// `clone_handle` shape.
#[derive(Clone, Default)]
struct MockManager {
    pairs: Arc<Mutex<HashMap<PairId, Arc<MockPair>>>>,
}

impl MockManager {
    fn insert(&self, pair: Arc<MockPair>) {
        self.pairs.lock().insert(pair.pair_id, pair);
    }

    fn len(&self) -> usize {
        self.pairs.lock().len()
    }
}

impl PairSweep for MockManager {
    type Pair = MockPair;

    fn list_for_sweep(&self) -> Vec<Arc<MockPair>> {
        self.pairs.lock().values().cloned().collect()
    }

    fn remove(&self, pair_id: &PairId) {
        self.pairs.lock().remove(pair_id);
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn stuck_pair_is_closed_and_removed() {
    let manager = MockManager::default();
    let (close_tx, mut close_rx) = mpsc::channel(8);
    let start = Instant::now();
    let pair = MockPair::new(close_tx, start);
    let pair_id = pair.pair_id;
    manager.insert(pair);

    let handle = tokio::spawn(run(manager.clone(), 4));

    // Advance well past the threshold; the sweeper wakes every
    // `idle_seconds / 4` = 1 second, so 10 s gives ~10 sweeps.
    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::task::yield_now().await;

    let signal = close_rx
        .recv()
        .await
        .expect("guillotine should signal close");
    assert_eq!(signal.pair_id, pair_id);
    assert_eq!(manager.len(), 0, "manager should drop the stuck pair");

    handle.abort();
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn fresh_pair_is_left_alone() {
    let manager = MockManager::default();
    let (close_tx, mut close_rx) = mpsc::channel(8);
    let pair = MockPair::new(close_tx, Instant::now());
    manager.insert(Arc::clone(&pair));

    let handle = tokio::spawn(run(manager.clone(), 4));

    // 3 seconds < 4 s threshold, then re-touch and wait another 3 s. The
    // pair's last_progress is at most 3 s old, so it must survive every
    // sweep.
    tokio::time::advance(Duration::from_secs(3)).await;
    pair.touch(Instant::now());
    tokio::time::advance(Duration::from_secs(3)).await;
    tokio::task::yield_now().await;

    assert!(
        close_rx.try_recv().is_err(),
        "active pair must not be closed"
    );
    assert_eq!(manager.len(), 1, "active pair must remain registered");

    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn sweep_once_only_kills_idle_pairs() {
    let manager = MockManager::default();
    let (close_tx, mut close_rx) = mpsc::channel(8);

    let now = Instant::now();
    let stuck = MockPair::new(close_tx.clone(), now);
    let fresh = MockPair::new(close_tx, now);
    let stuck_id = stuck.pair_id;
    let fresh_id = fresh.pair_id;
    manager.insert(stuck);
    manager.insert(Arc::clone(&fresh));

    // Move "now" forward 10 s but refresh the second pair so only the
    // first is over-threshold.
    let later = now + Duration::from_secs(10);
    fresh.touch(later);

    sweep_once(&manager, Duration::from_secs(4), later).await;

    let signal = close_rx.recv().await.expect("stuck pair should fire close");
    assert_eq!(signal.pair_id, stuck_id);
    assert!(
        close_rx.try_recv().is_err(),
        "fresh pair must not be closed"
    );

    let remaining: Vec<PairId> = manager
        .list_for_sweep()
        .into_iter()
        .map(|p| p.pair_id())
        .collect();
    assert_eq!(remaining, vec![fresh_id]);
}
