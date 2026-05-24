use std::net::{IpAddr, SocketAddr};

use cli_pocket_daemon_core::config::DaemonConfig;
use cli_pocket_daemon_core::{Daemon, DaemonError};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout, Duration};

#[tokio::test(flavor = "current_thread")]
async fn start_returns_error_when_listen_addr_is_in_use() {
    let occupied = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind occupied listener");
    let occupied_addr = occupied.local_addr().expect("occupied local addr");

    let dir = TempDir::new().expect("tempdir");
    let mut cfg = test_config(dir.path(), occupied_addr);
    cfg.listen.addr = occupied_addr.ip();
    cfg.listen.port = occupied_addr.port();

    let mut daemon = Daemon::boot(cfg).await.expect("boot daemon");

    match daemon.start().await {
        Ok(()) => {
            daemon.shutdown().await;
            panic!("start should fail when listen address is already in use");
        }
        Err(err) => assert!(
            matches!(err, DaemonError::Io(_)),
            "expected I/O bind error, got {err:?}"
        ),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn start_generates_and_rotates_pairing_code_after_ttl() {
    let dir = TempDir::new().expect("tempdir");
    let mut cfg = test_config(dir.path(), "127.0.0.1:0".parse().expect("socket addr"));
    cfg.pairing.code_ttl_secs = 1;

    let mut daemon = Daemon::boot(cfg).await.expect("boot daemon");
    let first_code = daemon.pairing_codes.current_code();
    assert_eq!(first_code.len(), 6);
    assert!(first_code.chars().all(|ch| ch.is_ascii_digit()));

    daemon.start().await.expect("start daemon");
    timeout(Duration::from_secs(3), async {
        loop {
            if daemon.pairing_codes.current_code() != first_code {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("pairing code should rotate after ttl");

    daemon.shutdown().await;
}

fn test_config(base: &std::path::Path, listen: SocketAddr) -> DaemonConfig {
    let mut cfg = DaemonConfig::default();
    cfg.listen.addr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    cfg.listen.port = listen.port();
    cfg.security.identity_path = base.join("identity.json");
    cfg.security.clients_path = base.join("clients.json");
    cfg.security.revoked_path = base.join("revoked.json");
    cfg
}
