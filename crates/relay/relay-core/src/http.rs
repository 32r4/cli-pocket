//! Relay HTTP routes (Plan E task E7).
//!
//! Exposes the `/health`, `/metrics`, `/ws/server`, and `/ws/client` endpoints
//! over [`axum`], threading the relay's shared state through `AppState`.
//!
//! The two WebSocket handlers are intentionally minimal: they upgrade the
//! connection, log it, and hand off to placeholder handler functions. The
//! full per-side forwarder lives in [`crate::forward`] and the bridge from
//! axum's [`WebSocket`] to the `tokio-tungstenite::Message`-typed sink/stream
//! is a follow-up task.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use cli_pocket_proto::ServerId;
use metrics_exporter_prometheus::PrometheusHandle;
use serde::Deserialize;
use uuid::Uuid;

use crate::caps::Caps;
use crate::forward::{run_client_side, run_server_side};
use crate::pairs::PairManager;
use crate::registry::ServerRegistry;

/// Shared application state passed to every axum handler.
///
/// The fields are individually `Clone` (each wraps an `Arc` internally), so
/// the `Clone` impl in [`crate::server`] cheaply duplicates the handle.
pub struct AppState {
    pub registry: ServerRegistry,
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
        .route("/ws/server", get(ws_server))
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
    server: String,
}

async fn ws_server(State(s): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_server(socket, s))
}

async fn ws_client(
    State(s): State<AppState>,
    Query(q): Query<ClientQuery>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let Ok(uuid) = Uuid::parse_str(&q.server) else {
        return (StatusCode::BAD_REQUEST, "invalid server").into_response();
    };
    let server_id = ServerId(uuid);
    ws.on_upgrade(move |socket| handle_client(socket, server_id, s))
        .into_response()
}

async fn handle_server(socket: WebSocket, s: AppState) {
    let ws = AxumWs(socket);
    if let Err(err) = run_server_side(ws, s.registry, s.pairs, s.caps).await {
        tracing::warn!(error = %err, "server relay websocket exited");
    }
}

async fn handle_client(socket: WebSocket, target: ServerId, s: AppState) {
    let ws = AxumWs(socket);
    if let Err(err) = run_client_side(ws, target, s.registry, s.pairs, s.caps).await {
        tracing::warn!(error = %err, ?target, "client relay websocket exited");
    }
}

struct AxumWs(WebSocket);

impl futures_util::stream::Stream for AxumWs {
    type Item =
        Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.0).poll_next(cx) {
            Poll::Ready(Some(Ok(msg))) => Poll::Ready(Some(Ok(axum_to_tungstenite(msg)))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(
                tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(err.to_string())),
            ))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl futures_util::sink::Sink<tokio_tungstenite::tungstenite::Message> for AxumWs {
    type Error = tokio_tungstenite::tungstenite::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.0).poll_ready(cx).map_err(|err| {
            tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(err.to_string()))
        })
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: tokio_tungstenite::tungstenite::Message,
    ) -> Result<(), Self::Error> {
        Pin::new(&mut self.0)
            .start_send(tungstenite_to_axum(item))
            .map_err(|err| {
                tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(err.to_string()))
            })
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.0).poll_flush(cx).map_err(|err| {
            tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(err.to_string()))
        })
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.0).poll_close(cx).map_err(|err| {
            tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(err.to_string()))
        })
    }
}

fn axum_to_tungstenite(msg: axum::extract::ws::Message) -> tokio_tungstenite::tungstenite::Message {
    match msg {
        axum::extract::ws::Message::Text(text) => {
            tokio_tungstenite::tungstenite::Message::Text(text)
        }
        axum::extract::ws::Message::Binary(bytes) => {
            tokio_tungstenite::tungstenite::Message::Binary(bytes)
        }
        axum::extract::ws::Message::Ping(bytes) => {
            tokio_tungstenite::tungstenite::Message::Ping(bytes)
        }
        axum::extract::ws::Message::Pong(bytes) => {
            tokio_tungstenite::tungstenite::Message::Pong(bytes)
        }
        axum::extract::ws::Message::Close(frame) => {
            tokio_tungstenite::tungstenite::Message::Close(frame.map(|frame| {
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(
                        frame.code,
                    ),
                    reason: frame.reason,
                }
            }))
        }
    }
}

fn tungstenite_to_axum(msg: tokio_tungstenite::tungstenite::Message) -> axum::extract::ws::Message {
    match msg {
        tokio_tungstenite::tungstenite::Message::Text(text) => {
            axum::extract::ws::Message::Text(text)
        }
        tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
            axum::extract::ws::Message::Binary(bytes)
        }
        tokio_tungstenite::tungstenite::Message::Ping(bytes) => {
            axum::extract::ws::Message::Ping(bytes)
        }
        tokio_tungstenite::tungstenite::Message::Pong(bytes) => {
            axum::extract::ws::Message::Pong(bytes)
        }
        tokio_tungstenite::tungstenite::Message::Close(frame) => {
            axum::extract::ws::Message::Close(frame.map(|frame| axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason,
            }))
        }
        tokio_tungstenite::tungstenite::Message::Frame(frame) => {
            axum::extract::ws::Message::Binary(frame.payload().clone())
        }
    }
}
