#![cfg(not(target_arch = "wasm32"))]

//! End-to-end smoke test: SessionHandle ↔ in-process mock WS server.
//! Currently #[ignore]'d because the mock daemon would need to replay Plan B's
//! canonical Hello / HelloOk frames — see crates/shared/proto/tests/test_vectors
//! for the source-of-truth fixtures once they're stable enough to load here.

use cli_pocket_client_core::{ClientIdentity, SessionBuilder, SessionConfig, SessionEndpoint};
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_proto::Capabilities;
use cli_pocket_tauri_bindings::{FileKvStore, OsRandom, SessionHandle, TokioClock, TokioWsTransport};
use futures_util::future::FutureExt as _;
use futures_util::future::LocalBoxFuture;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::task::LocalSet;

// Simple SessionSpawner that uses tokio::task::spawn_local.
#[derive(Clone, Copy)]
struct LocalSpawner;

impl SessionSpawner for LocalSpawner {
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>) {
        tokio::task::spawn_local(fut);
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires mock daemon replay; enable when test_vectors are loaded here"]
async fn round_trip_create_terminal() {
    let local = LocalSet::new();
    local
        .run_until(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            // Mock daemon stub — accepts one WS connection and drops it.
            // Real replay of Hello/HelloOk frames is the missing piece; the
            // scaffold compiles so the wiring is exercised.
            tokio::task::spawn_local(async move {
                let (sock, _) = listener.accept().await.unwrap();
                let _ws = tokio_tungstenite::accept_async(sock).await.unwrap();
                // TODO: replay Hello / HelloOk + TerminalCreated frames using
                // cli_pocket_proto::codec::{decode_frame, encode_frame}.
            });

            let dir = tempdir().unwrap();
            let kv = FileKvStore::open_at(dir.path()).unwrap();
            let identity = ClientIdentity::load_or_create(&kv, &OsRandom)
                .await
                .unwrap();

            let url = format!("ws://{addr}");
            // Move url into a closure used by the factory each reconnect attempt.
            let factory = move || {
                let u = url.clone();
                async move { TokioWsTransport::connect(&u, Some("cli-pocket-host/v1")).await }
                    .boxed_local()
            };

            let config = SessionConfig {
                endpoint: SessionEndpoint::Direct(format!("ws://{addr}")),
                server_public: [0_u8; 32],
                resume_token: None,
                capabilities: Capabilities::NONE,
                backoff: (50, 1_000, 20),
            };

            let builder = SessionBuilder::new(
                identity,
                config,
                TokioClock,
                OsRandom,
                kv,
                factory,
                LocalSpawner,
            );

            let handle = SessionHandle::new_disconnected();
            handle.connect(builder);

            // Once Plan B's vectors are integrated:
            //   handle.create_terminal(TerminalCreateParams { … }).await.unwrap();
            //   let ev = handle.take_event_rx().unwrap().recv().await.unwrap();
            //   assert!(matches!(ev, ClientEvent::TerminalCreated(_)));
        })
        .await;
}
