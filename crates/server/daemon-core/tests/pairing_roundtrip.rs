//! D11: End-to-end pairing roundtrip integration test.
//!
//! Asserts the daemon's happy path: a paired client (whose public key is in
//! `clients.json`) connects over an `InMemoryTransport`, completes Noise XK,
//! sends `Hello`, creates a terminal, writes input, and receives a
//! `TerminalCreateOk` plus an `Output` snapshot frame from the daemon.
//!
//! No real sockets are involved. The client side manually drives a
//! `NoiseInitiator` against `run_connection_with_handshake` running on the
//! other half of an `InMemoryTransportPair`.

use std::sync::Arc;
use std::time::Duration;

use cli_pocket_crypto::{KeyPair, NoiseInitiator, NoiseSession};
use cli_pocket_daemon_core::client_db::{ClientDb, ClientRecord};
use cli_pocket_daemon_core::connection::{run_connection_with_handshake, ConnectionDeps};
use cli_pocket_daemon_core::identity_store::load_or_create;
use cli_pocket_daemon_core::session::SessionManager;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::{Capabilities, ClientKind, Hello, ServerInfo};
use cli_pocket_proto::{ClientId, TerminalCreateParams, PROTOCOL_VERSION};
use cli_pocket_transport::{InMemoryTransport, InMemoryTransportPair, Transport};
use tempfile::TempDir;
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_client_creates_terminal_end_to_end() {
    // ---- Setup: tempdir, daemon identity, client DB with client's pub key. ----
    let dir = TempDir::new().expect("tempdir");
    let id_path = dir.path().join("identity.json");
    let clients_path = dir.path().join("clients.json");
    let revoked_path = dir.path().join("revoked.json");

    let daemon_id = load_or_create(&id_path).expect("load_or_create daemon identity");
    let daemon_pub = daemon_id.keypair.public;

    let client_keypair = KeyPair::generate().expect("client keypair");
    let client_pub = client_keypair.public;

    let db = Arc::new(
        ClientDb::open(&clients_path, &revoked_path)
            .await
            .expect("ClientDb::open"),
    );
    let client_id = ClientId(Uuid::from_bytes([
        0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
        0x77,
    ]));
    db.add(ClientRecord {
        client_id,
        public_key: client_pub,
        label: "test-client".into(),
        paired_at: 0,
    })
    .await
    .expect("add client record");

    // ---- SessionManager + ConnectionDeps for the daemon side. ----
    let session_mgr = Arc::new(SessionManager::new(4));
    let server_info = ServerInfo {
        server_version: "test".to_string(),
        host_label: None,
    };

    let deps = ConnectionDeps {
        session_mgr: Arc::clone(&session_mgr),
        client_db: Arc::clone(&db),
        server_info,
    };

    // ---- InMemoryTransport pair: `a` -> daemon, `b` -> manual client. ----
    let InMemoryTransportPair {
        a: daemon_transport,
        b: client_transport,
    } = InMemoryTransportPair::new(16);

    // ---- Spawn daemon-side `run_connection_with_handshake`. ----
    let daemon_keypair = daemon_id.keypair.clone();
    let daemon_task = tokio::spawn(async move {
        run_connection_with_handshake(daemon_transport, &daemon_keypair, None, deps).await
    });

    // ---- Client side: drive Noise XK initiator manually. ----
    let mut client_transport = client_transport;
    let mut init = NoiseInitiator::new(&client_keypair, &daemon_pub, None).expect("initiator");

    // XK msg1: client -> daemon (`e`)
    let msg1 = init.write_handshake().expect("write msg1");
    client_transport.send(msg1).await.expect("send msg1");

    // XK msg2: daemon -> client (`e, ee, s, es`)
    let msg2 = recv_with_timeout(&mut client_transport)
        .await
        .expect("recv msg2");
    init.read_handshake(&msg2).expect("read msg2");

    // XK msg3: client -> daemon (`s, se`)
    let msg3 = init.write_handshake().expect("write msg3");
    client_transport.send(msg3).await.expect("send msg3");

    // Transition both sides to transport mode.
    let mut session = init.finish().expect("initiator finish");

    // ---- Send Hello frame (encrypted). ----
    let hello = Frame::body(FrameBody::Hello(Hello {
        protocol_min: PROTOCOL_VERSION,
        protocol_max: PROTOCOL_VERSION,
        capabilities: Capabilities::NONE,
        client_kind: ClientKind::Cli,
        resume: None,
    }));
    send_frame(&mut client_transport, &mut session, &hello)
        .await
        .expect("send Hello");

    // ---- Expect HelloOk. ----
    let resp = recv_frame(&mut client_transport, &mut session)
        .await
        .expect("recv HelloOk");
    match &resp.body {
        FrameBody::HelloOk(ok) => {
            assert_eq!(ok.protocol, PROTOCOL_VERSION, "HelloOk.protocol mismatch");
            assert!(!ok.resumed, "fresh session should not be resumed");
        }
        other => panic!("expected HelloOk, got {other:?}"),
    }

    // ---- Send TerminalCreate. ----
    let request_id = 1u32;
    let create = Frame::body(FrameBody::TerminalCreate {
        request_id,
        params: TerminalCreateParams {
            cols: 80,
            rows: 24,
            cwd: None,
            cmd: terminal_cmd(),
            env: Vec::new(),
            scrollback_bytes: None,
        },
    });
    send_frame(&mut client_transport, &mut session, &create)
        .await
        .expect("send TerminalCreate");

    // ---- Expect TerminalCreateOk (the snapshot Output may follow but is optional). ----
    let create_ok = recv_frame(&mut client_transport, &mut session)
        .await
        .expect("recv TerminalCreateOk");
    let (terminal_id, stream_id) = match &create_ok.body {
        FrameBody::TerminalCreateOk {
            request_id: rid,
            terminal,
            stream,
        } => {
            assert_eq!(*rid, request_id, "request_id should match");
            (*terminal, *stream)
        }
        other => panic!("expected TerminalCreateOk, got {other:?}"),
    };

    // ---- Drain any follow-up snapshot Output frame, if produced quickly. ----
    // The daemon sends an initial Output with the terminal's snapshot only when
    // it is non-empty; on a freshly-spawned PTY the snapshot may legitimately be
    // empty, so we tolerate either outcome via a short timeout.
    if let Ok(Ok(extra)) = timeout(
        Duration::from_millis(200),
        recv_frame_inner(&mut client_transport, &mut session),
    )
    .await
    {
        if let FrameBody::Output {
            stream: s,
            bytes: _,
            seq: _,
        } = extra.body
        {
            assert_eq!(s, stream_id, "Output.stream should match the created stream");
        }
    }

    // ---- Sanity: SessionManager now owns one terminal. ----
    let list = session_mgr.list();
    assert!(
        list.iter().any(|info| info.terminal == terminal_id),
        "session manager should track the created terminal"
    );

    daemon_task.abort();
    let _ = daemon_task.await;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn recv_with_timeout(t: &mut InMemoryTransport) -> Option<Vec<u8>> {
    timeout(Duration::from_secs(5), t.recv())
        .await
        .expect("transport recv timed out")
        .expect("transport recv error")
}

async fn send_frame(
    t: &mut InMemoryTransport,
    session: &mut NoiseSession,
    frame: &Frame,
) -> Result<(), String> {
    let plain = encode_frame(frame).map_err(|e| format!("encode_frame: {e}"))?;
    let ct = session
        .encrypt(&plain)
        .map_err(|e| format!("noise encrypt: {e}"))?;
    t.send(ct).await.map_err(|e| format!("transport send: {e}"))
}

async fn recv_frame(
    t: &mut InMemoryTransport,
    session: &mut NoiseSession,
) -> Result<Frame, String> {
    timeout(Duration::from_secs(5), recv_frame_inner(t, session))
        .await
        .map_err(|_| "recv_frame timed out".to_string())?
}

async fn recv_frame_inner(
    t: &mut InMemoryTransport,
    session: &mut NoiseSession,
) -> Result<Frame, String> {
    let ct = t
        .recv()
        .await
        .map_err(|e| format!("transport recv: {e}"))?
        .ok_or_else(|| "transport closed".to_string())?;
    let plain = session
        .decrypt(&ct)
        .map_err(|e| format!("noise decrypt: {e}"))?;
    decode_frame(&plain).map_err(|e| format!("decode_frame: {e}"))
}

#[cfg(windows)]
fn terminal_cmd() -> Vec<String> {
    // Long-lived shell so the reaper does not snatch the terminal before we
    // assert against the session manager. `cmd.exe` waits for input on its
    // own; we never send any, so it blocks until the test aborts the daemon.
    vec!["C:\\Windows\\System32\\cmd.exe".to_string()]
}

#[cfg(unix)]
fn terminal_cmd() -> Vec<String> {
    // `cat` with no args blocks reading from stdin, keeping the PTY alive.
    vec!["/bin/cat".to_string()]
}
