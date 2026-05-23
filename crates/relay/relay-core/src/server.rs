//! Relay server facade (Plan E task E7).
//!
//! Owns the long-lived [`AppState`] and spawns the two background tasks the
//! relay needs (caps refill + stuck-pair guillotine) alongside the axum
//! listener.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::caps::Caps;
use crate::config::RelayConfig;
use crate::http::{router, AppState};
use crate::pairs::PairManager;
use crate::registry::HostRegistry;

/// Top-level relay server. Construct with [`RelayServer::new`] and run with
/// [`RelayServer::serve`].
pub struct RelayServer {
    pub state: AppState,
    pub config: RelayConfig,
}

impl RelayServer {
    /// Build a fresh server. Installs the Prometheus recorder via
    /// [`crate::metrics::init`] (so this must run exactly once per process).
    #[must_use]
    pub fn new(config: RelayConfig) -> Self {
        let registry = HostRegistry::new();
        let pairs = PairManager::new();
        let caps = Caps::new(
            config.caps.max_hosts,
            config.caps.max_pairs,
            config.caps.max_bytes_per_sec,
            config.caps.max_queued_bytes,
        );
        let metrics = Arc::new(crate::metrics::init());
        let state = AppState {
            registry,
            pairs,
            caps,
            metrics,
            config: config.clone(),
        };
        Self { state, config }
    }

    /// Borrow the live config.
    #[must_use]
    pub fn config(&self) -> &RelayConfig {
        &self.config
    }

    /// Bind the configured address and run the relay until the listener is
    /// torn down. Spawns the caps-refill and guillotine background tasks.
    pub async fn serve(self) -> crate::RelayResult<()> {
        let addr = SocketAddr::new(self.config.listen.addr, self.config.listen.port);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        tracing::info!(%local_addr, "relay listening");

        let _refill = tokio::spawn(refill_loop(self.state.caps.clone_handle()));
        let _gull = tokio::spawn(crate::guillotine::run(
            self.state.pairs.clone_handle(),
            self.config.guillotine.idle_seconds,
        ));

        let app = router(self.state.clone());
        axum::serve(listener, app)
            .await
            .map_err(|e| crate::RelayError::Internal(e.to_string()))?;
        Ok(())
    }
}

async fn refill_loop(_caps: Caps) {
    // The per-pair rate buckets live on `PairTicket`s, not on `Caps` itself.
    // Until pair construction is wired through the server (follow-up task)
    // there is nothing to refill, so this loop just ticks at the documented
    // 1 Hz cadence and exits when the runtime drops it.
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
    }
}

// `AppState` cannot derive `Clone` because `crate::RelayConfig` is `Clone` but
// the field-level handle helpers we want to use (`clone_handle`) are not the
// derive's default behaviour. Implementing it manually documents the intent.
impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone_handle(),
            pairs: self.pairs.clone_handle(),
            caps: self.caps.clone_handle(),
            metrics: Arc::clone(&self.metrics),
            config: self.config.clone(),
        }
    }
}
