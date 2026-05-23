//! Integration tests for the relay's axum router (Plan E task E7).
//!
//! Each test builds a fresh [`RelayServer`], serves its router on
//! `127.0.0.1:0`, and drives a real HTTP request through [`reqwest`].
//! The OS-assigned port lets multiple tests run in parallel without
//! colliding on a fixed listen address.

use std::net::{IpAddr, Ipv4Addr};

use cli_pocket_relay_core::http::router;
use cli_pocket_relay_core::{RelayConfig, RelayServer};

#[tokio::test(flavor = "current_thread")]
async fn health_returns_ok() {
    let mut config = RelayConfig::default();
    config.listen.addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    config.listen.port = 0;

    let server = RelayServer::new(config);
    let app = router(server.state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral");
    let addr = listener.local_addr().expect("local_addr");

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("GET /health");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.text().await.expect("read body");
    assert_eq!(body, "ok");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn metrics_returns_prometheus_text() {
    let mut config = RelayConfig::default();
    config.listen.addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    config.listen.port = 0;

    let server = RelayServer::new(config);
    let app = router(server.state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral");
    let addr = listener.local_addr().expect("local_addr");

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .expect("GET /metrics");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.text().await.expect("read body");
    // Either contains relay-prefixed metrics, or is empty if nothing has
    // been recorded yet.
    assert!(
        body.is_empty() || body.contains("cli_pocket_relay"),
        "unexpected /metrics body: {body:?}"
    );

    handle.abort();
}
