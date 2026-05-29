//! D14: Revocation drops live sessions integration test.
//!
//! Same setup as `pairing_roundtrip.rs`: a paired client (whose public key is
//! in `clients.json`) connects over an `InMemoryTransport`, completes Noise
//! XK, sends `Hello`, receives `HelloOk`, and creates a terminal. Then the
//! test calls `db.revoke(cid)` and asserts that the next encrypted recv()
//! returns either a `Bye { reason: Revoked }` frame or transport close within
//! 500ms — i.e. the daemon drops live sessions whose client_id is revoked.
//!
//! Per Plan D this duplicates the setup from `pairing_roundtrip.rs` rather
//! than factoring a shared `tests/common/mod.rs` helper; the duplication is
//! deliberate so each integration test stays self-contained.

use std::sync::Arc;
use std::time::Duration;

use cli_pocket_crypto::{KeyPair, NoiseAnonymousInitiator, NoiseSession};
use cli_pocket_daemon_core::client_db::{ClientDb, ClientRecord};
use cli_pocket_daemon_core::connection::{
    run_connection_with_handshake, ConnectionDeps, HandshakeKind,
};
use cli_pocket_daemon_core::identity_store::load_or_create;
use cli_pocket_daemon_core::session::SessionManager;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::{Hello, ServerInfo};
use cli_pocket_proto::{ByeReason, ClientId, TerminalCreateParams, PROTOCOL_VERSION};
use cli_pocket_transport::{InMemoryTransport, InMemoryTransportPair, Transport};
use tempfile::TempDir;
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revocation_drops_live_session() {
    // ---- Setup: tempdir, daemon identity, client DB with client's pub key. ----
    let dir = TempDir::new().expect("tempdir");
    let id_path = dir.path().join("identity.json");
    let clients_path = dir.path().join("clients.json");
    let revoked_path = dir.path().join("revoked.json");

    let daemon_id = load_or_create(&id_path).expect("load_or_create daemon identity");

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
        paired_at: 0,
    })
    .await
    .expect("add client record");

    // ---- SessionManager + ConnectionDeps for the daemon side. ----
    let session_mgr = Arc::new(SessionManager::new(4));
    let server_info = ServerInfo {
        server_version: "test".to_string(),
        server_label: None,
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
        run_connection_with_handshake(
            daemon_transport,
            &daemon_keypair,
            HandshakeKind::Direct { auto_pair: false },
            deps,
        )
        .await
    });

    // ---- Client side: drive Noise XK initiator manually. ----
    let mut client_transport = client_transport;
    let mut init = NoiseAnonymousInitiator::new(&client_keypair).expect("initiator");

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

    // ---- Expect TerminalCreateOk. ----
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
    // Mirrors `pairing_roundtrip.rs`: a freshly-spawned PTY may emit an empty
    // snapshot, in which case no Output frame is produced; tolerate either via
    // a short timeout.
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
            assert_eq!(
                s, stream_id,
                "Output.stream should match the created stream"
            );
        }
    }

    // ---- Sanity: SessionManager now owns one terminal. ----
    let list = session_mgr.list();
    assert!(
        list.iter().any(|info| info.terminal == terminal_id),
        "session manager should track the created terminal"
    );

    // ---- Revoke the client. ----
    db.revoke(client_id).await.expect("revoke client");

    // ---- Assert: next encrypted recv() yields Bye{Revoked} or transport
    //               close within 500ms. ----
    let outcome = timeout(
        Duration::from_millis(500),
        recv_revocation(&mut client_transport, &mut session),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "after revocation, expected Bye{{Revoked}} or transport close within 500ms; \
             daemon kept the session alive (revocation not enforced for live connections)"
        )
    });

    match outcome {
        // Both Bye{Revoked} and a clean transport close are acceptable per the
        // task spec ("Bye{Revoked} frame or transport close within 500ms").
        RevocationOutcome::ByeRevoked | RevocationOutcome::TransportClosed => {}
        RevocationOutcome::Other(frame) => {
            panic!(
                "after revocation, expected Bye{{Revoked}} or transport close, got {:?}",
                frame.body
            );
        }
    }

    daemon_task.abort();
    let _ = daemon_task.await;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

enum RevocationOutcome {
    ByeRevoked,
    TransportClosed,
    Other(Frame),
}

async fn recv_revocation(
    t: &mut InMemoryTransport,
    session: &mut NoiseSession,
) -> RevocationOutcome {
    match t.recv().await {
        Ok(Some(ct)) => match session.decrypt(&ct) {
            Ok(plain) => match decode_frame(&plain) {
                Ok(frame) => match &frame.body {
                    FrameBody::Bye {
                        reason: ByeReason::Revoked,
                    } => RevocationOutcome::ByeRevoked,
                    _ => RevocationOutcome::Other(frame),
                },
                Err(_) => RevocationOutcome::TransportClosed,
            },
            // A decrypt failure here means the daemon side tore the session
            // down — treat as a transport-close equivalent.
            Err(_) => RevocationOutcome::TransportClosed,
        },
        // `recv` returning `Ok(None)` or `Err(_)` both mean the peer is gone.
        Ok(None) | Err(_) => RevocationOutcome::TransportClosed,
    }
}

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
