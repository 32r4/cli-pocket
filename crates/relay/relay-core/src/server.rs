//! Relay server facade.
//!
//! Wraps the relay HTTP/WS endpoints into a runnable server. The full axum
//! routing wiring lives in `http.rs` (Task E7 fills it in); this facade gives
//! the binary a stable `serve()` API even when the inner routes are stubs.

use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct RelayServer {
    config: crate::RelayConfig,
}

impl RelayServer {
    #[must_use]
    pub fn new(config: crate::RelayConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn config(&self) -> &crate::RelayConfig {
        &self.config
    }

    /// Run the relay listener until the OS closes the socket.
    ///
    /// The actual HTTP/WS routes are filled in by Task E7. For now this
    /// binds the listener so `cli-pocket-relay serve` produces visible
    /// "listening on …" output and a runnable process.
    pub async fn serve(self) -> crate::RelayResult<()> {
        let addr = SocketAddr::new(self.config.listen.addr, self.config.listen.port);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(%addr, "relay listening (stub — routes wired in Task E7)");

        loop {
            let (_sock, peer) = listener.accept().await?;
            tracing::debug!(%peer, "accepted (route handler is a stub)");
        }
    }
}
