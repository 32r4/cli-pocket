//! Stuck-pair sweeper ("guillotine").
//!
//! The relay routes opaque ciphertext between matched daemon/client pairs. A
//! misbehaving peer can leave a pair half-open with no progress; the
//! guillotine periodically scans live pairs and closes any whose
//! `last_progress` is older than a configured threshold.
//!
//! ## E5/E6 coordination
//!
//! Plan E task E5 owns `pairs.rs` (the concrete `Pair` / `PairManager`
//! types), and task E6 (this module) lands in parallel. To avoid touching
//! E5's files before they exist, the guillotine is written against two
//! traits — [`PairHandle`] and [`PairSweep`] — that capture exactly the
//! behaviour the sweeper needs. Task E7 wires the real `Pair` /
//! `PairManager` into these traits so the runtime can spawn `run(..)` with
//! the live manager.
//!
//! Tests use a hand-rolled mock that implements both traits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cli_pocket_proto::PairId;
use futures_util::future::BoxFuture;

/// One live pair, observable by the guillotine.
///
/// The trait is intentionally minimal: enough to decide whether the pair is
/// stuck and to close it asynchronously. The real implementation in
/// `pairs.rs` (Plan E task E5) will send `PairClose { reason: Stuck }` on
/// both the server and client write channels.
pub trait PairHandle: Send + Sync + 'static {
    /// Stable identifier for this pair.
    fn pair_id(&self) -> PairId;

    /// Instant of the most recent progress (bytes forwarded or control
    /// frame). The guillotine compares this against
    /// `Instant::now() - threshold`.
    fn last_progress(&self) -> Instant;

    /// Asynchronously close both legs of the pair with a "stuck" reason.
    ///
    /// Implementations must not block; sending on a full or closed channel
    /// should be tolerated silently (the peer is presumably gone anyway).
    fn close_stuck(self: Arc<Self>) -> BoxFuture<'static, ()>;
}

/// The pair-manager surface used by the guillotine.
///
/// Decoupled from the concrete `PairManager` in `pairs.rs` so this module
/// can land before task E5 finishes. See module docs.
pub trait PairSweep: Clone + Send + Sync + 'static {
    /// Concrete pair type managed by this sweeper.
    type Pair: PairHandle;

    /// Snapshot every live pair. The sweeper filters this list against the
    /// idle threshold; implementations should clone cheaply (the live
    /// `PairManager` keeps `Arc<Pair>` values in a `Mutex<HashMap>`).
    fn list_for_sweep(&self) -> Vec<Arc<Self::Pair>>;

    /// Drop the pair from the live set. Called after `close_stuck` has been
    /// dispatched so the server/client tasks can drain their writer queues.
    fn remove(&self, pair_id: &PairId);
}

/// Run the stuck-pair sweeper forever.
///
/// Spawn this as a background task at relay startup. Sleeps for
/// `(idle_seconds / 4).max(1)` between sweeps so the worst-case detection
/// latency is roughly `1.25 * idle_seconds`. Each sweep:
///
/// 1. snapshots the live pairs,
/// 2. filters those whose `last_progress` is older than `idle_seconds`,
/// 3. closes them via [`PairHandle::close_stuck`],
/// 4. removes them from the manager,
/// 5. increments `cli_pocket_relay_pair_close_total{reason="stuck"}`.
pub async fn run<S: PairSweep>(pairs: S, idle_seconds: u64) {
    let interval = Duration::from_secs((idle_seconds / 4).max(1));
    let threshold = Duration::from_secs(idle_seconds);
    loop {
        tokio::time::sleep(interval).await;
        sweep_once(&pairs, threshold, Instant::now()).await;
    }
}

/// One pass of the sweeper, factored out for testability.
///
/// Made `pub` (rather than `pub(crate)`) so integration tests in
/// `tests/guillotine_kicks.rs` can drive a single sweep deterministically
/// without leaking the infinite loop into the test runtime.
pub async fn sweep_once<S: PairSweep>(pairs: &S, threshold: Duration, now: Instant) {
    let to_kill: Vec<Arc<S::Pair>> = pairs
        .list_for_sweep()
        .into_iter()
        .filter(|p| now.duration_since(p.last_progress()) > threshold)
        .collect();
    for pair in to_kill {
        let pair_id = pair.pair_id();
        tracing::warn!(?pair_id, "guillotine: closing stuck pair");
        Arc::clone(&pair).close_stuck().await;
        pairs.remove(&pair_id);
        metrics::counter!("cli_pocket_relay_pair_close_total", "reason" => "stuck").increment(1);
    }
}
