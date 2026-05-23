//! Relay HTTP routes (Plan E task E7).
//!
//! Exposes the `/health`, `/metrics`, `/ws/host`, and `/ws/client` endpoints
//! over [`axum`], threading the relay's shared state through `AppState`.
//!
//! The two WebSocket handlers are intentionally minimal: they upgrade the
//! connection, log it, and hand off to placeholder handler functions. The
//! full per-side forwarder lives in [`crate::forward`] and the bridge from
//! axum's [`WebSocket`] to the `tokio-tungstenite::Message`-typed sink/stream
//! is a follow-up task.

use std::sync::Arc;

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use cli_pocket_proto::HostId;
use metrics_exporter_prometheus::PrometheusHandle;
use serde::Deserialize;
use uuid::Uuid;

use crate::caps::Caps;
use crate::pairs::PairManager;
use crate::registry::HostRegistry;

/// Shared application state passed to every axum handler.
///
/// The fields are individually `Clone` (each wraps an `Arc` internally), so
/// the `Clone` impl in [`crate::server`] cheaply duplicates the handle.
pub struct AppState {
    pub registry: HostRegistry,
    pub pairs: PairManager,
    pub caps: Caps,
    pub metrics: Arc<PrometheusHandle>,
    pub config: crate::RelayConfig,
}

/// Build the relay router with the given shared state.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_route))
        .route("/ws/host", get(ws_host))
        .route("/ws/client", get(ws_client))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn metrics_route(State(s): State<AppState>) -> impl IntoResponse {
    s.metrics.render()
}

#[derive(Deserialize)]
struct ClientQuery {
    host: String,
}

async fn ws_host(State(s): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_host(socket, s))
}

async fn ws_client(
    State(s): State<AppState>,
    Query(q): Query<ClientQuery>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let Ok(uuid) = Uuid::parse_str(&q.host) else {
        return (StatusCode::BAD_REQUEST, "invalid host").into_response();
    };
    let host_id = HostId(uuid);
    ws.on_upgrade(move |socket| handle_client(socket, host_id, s))
        .into_response()
}

/// Placeholder host-side handler.
///
/// Closes the upgraded socket cleanly. The full host-side forwarder
/// (parse `RelayCtrl::HostRegister`, register in `s.registry`, run
/// [`crate::forward::run_host_side`]) lands in a follow-up task because it
/// needs the axum -> tungstenite `Message` shim.
async fn handle_host(socket: WebSocket, s: AppState) {
    tracing::debug!(
        hosts = s.caps.snapshot().hosts,
        "ws/host upgrade received (forwarder wiring deferred)"
    );
    let _ = socket.close().await;
}

/// Placeholder client-side handler. See [`handle_host`] for status.
async fn handle_client(socket: WebSocket, target: HostId, s: AppState) {
    tracing::debug!(
        ?target,
        pairs = s.caps.snapshot().pairs,
        "ws/client upgrade received (forwarder wiring deferred)"
    );
    let _ = socket.close().await;
}
