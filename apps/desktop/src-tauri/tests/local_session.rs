use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::time::Duration;

use cli_pocket_client_core::{
    ClientEvent, ClientIdentity, SessionBuilder, SessionConfig, SessionEndpoint,
};
use cli_pocket_daemon_core::config::{ListenConfig, SecurityConfig};
use cli_pocket_daemon_core::{Daemon, DaemonConfig};
use cli_pocket_tauri_bindings::{
    FileKvStore, OsRandom, SessionHandle, TokioClock, TokioWsTransport,
};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_native_session_connects_to_embedded_local_daemon() {
    let root = unique_temp_dir();
    let server_dir = root.join("server");
    let client_dir = root.join("client");
    std::fs::create_dir_all(&server_dir).expect("create server temp dir");
    std::fs::create_dir_all(&client_dir).expect("create client temp dir");

    let port = unused_local_port();
    let mut daemon = Daemon::boot(DaemonConfig {
        listen: ListenConfig {
            addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
        },
        security: SecurityConfig {
            identity_path: server_dir.join("identity.json"),
            clients_path: server_dir.join("clients.json"),
            revoked_path: server_dir.join("revoked.json"),
        },
        ..DaemonConfig::default()
    })
    .await
    .expect("boot daemon");
    daemon.start_local_only().await.expect("start daemon");

    let kv = FileKvStore::open_at(&client_dir).expect("open client kv");
    let identity = ClientIdentity::load_or_create(&kv, &OsRandom)
        .await
        .expect("load identity");
    let endpoint_url = format!("ws://127.0.0.1:{port}/session");
    let (event_tx, mut event_rx) = mpsc::channel::<ClientEvent>(16);
    let handle = SessionHandle::spawn(event_tx);

    handle
        .connect(move |spawner| {
            SessionBuilder::new(
                identity,
                SessionConfig {
                    endpoint: SessionEndpoint::Direct(endpoint_url.clone()),
                    resume_token: None,
                    backoff: (50, 1_000, 20),
                },
                TokioClock,
                OsRandom,
                kv.clone(),
                move || {
                    let url = endpoint_url.clone();
                    Box::pin(async move { TokioWsTransport::connect(&url, None).await })
                },
                spawner.clone(),
            )
        })
        .await
        .expect("start session");

    let mut disconnects = Vec::new();
    let connected = timeout(Duration::from_secs(5), async {
        while let Some(event) = event_rx.recv().await {
            match event {
                ClientEvent::Connected { .. } => return true,
                ClientEvent::Disconnected { reason, .. } => disconnects.push(reason),
                _ => {}
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    daemon.shutdown().await;
    std::fs::remove_dir_all(root).expect("remove temp dir");

    assert!(
        connected,
        "session disconnected before connect: disconnects={disconnects:?}, handle_connected={}",
        handle.is_connected().await
    );
}

fn unused_local_port() -> u16 {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .expect("bind ephemeral local port");
    listener.local_addr().expect("local addr").port()
}

fn unique_temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cli-pocket-desktop-test-{}", uuid::Uuid::now_v7()))
}
