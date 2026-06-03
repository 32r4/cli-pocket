//! D11: End-to-end pairing roundtrip integration test.
//!
//! Asserts the daemon's happy path: a paired client (whose public key is in
//! `clients.json`) connects over an `InMemoryTransport`, completes Noise XK,
//! sends `Hello`, creates a terminal, attaches it, writes input, and receives
//! chunked terminal output from the daemon.
//!
//! No real sockets are involved. The client side manually drives a
//! `NoiseInitiator` against `run_connection_with_handshake` running on the
//! other half of an `InMemoryTransportPair`.

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
use cli_pocket_proto::{
    ClientId, RequestBody, RequestFrame, RequestId, ResponseBody, StreamId, TerminalCreateParams,
    TerminalId, PROTOCOL_VERSION,
};
use cli_pocket_transport::{InMemoryTransport, InMemoryTransportPair, Transport};
use parking_lot::Mutex;
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
        config: Arc::new(Mutex::new(cli_pocket_daemon_core::DaemonConfig::default())),
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
    let create = request_frame(
        request_id,
        RequestBody::CreateTerminal {
            params: TerminalCreateParams {
                cols: 80,
                rows: 24,
                cwd: None,
                cmd: terminal_cmd(),
                env: Vec::new(),
            },
        },
    );
    send_frame(&mut client_transport, &mut session, &create)
        .await
        .expect("send TerminalCreate");

    // ---- Expect create response. ----
    let create_ok = recv_frame(&mut client_transport, &mut session)
        .await
        .expect("recv create response");
    let terminal_id = match &create_ok.body {
        FrameBody::Response(response) => {
            assert_eq!(
                response.id,
                RequestId(request_id),
                "request_id should match"
            );
            match &response.result {
                Ok(ResponseBody::CreateTerminal { info }) => info.terminal,
                other => panic!("expected CreateTerminal response body, got {other:?}"),
            }
        }
        other => panic!("expected create response, got {other:?}"),
    };

    // ---- Sanity: SessionManager now owns one terminal. ----
    let list = session_mgr.list();
    assert!(
        list.iter().any(|info| info.terminal == terminal_id),
        "session manager should track the created terminal"
    );

    daemon_task.abort();
    let _ = daemon_task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_client_receives_live_output_after_input() {
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
    db.add(ClientRecord {
        client_id: ClientId(Uuid::from_bytes([0x88; 16])),
        public_key: client_pub,
        paired_at: 0,
    })
    .await
    .expect("add client record");

    let session_mgr = Arc::new(SessionManager::new(4));
    let deps = ConnectionDeps {
        session_mgr,
        client_db: db,
        server_info: ServerInfo {
            server_version: "test".to_string(),
            server_label: None,
        },
        config: Arc::new(Mutex::new(cli_pocket_daemon_core::DaemonConfig::default())),
    };
    let InMemoryTransportPair {
        a: daemon_transport,
        b: client_transport,
    } = InMemoryTransportPair::new(16);
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

    let mut client_transport = client_transport;
    let mut session = connect_paired_client(&mut client_transport, &client_keypair)
        .await
        .expect("connect paired client");

    let terminal_id = create_terminal(
        &mut client_transport,
        &mut session,
        live_output_terminal_cmd(),
    )
    .await
    .expect("create terminal");
    let stream_id = attach_terminal(&mut client_transport, &mut session, terminal_id)
        .await
        .expect("attach terminal");

    let input = live_output_input();
    let expected = b"cli-pocket-live-output";
    send_frame(
        &mut client_transport,
        &mut session,
        &request_frame(
            3,
            RequestBody::SendInput {
                terminal_id,
                bytes: input.into(),
            },
        ),
    )
    .await
    .expect("send input");

    recv_output_containing(&mut client_transport, &mut session, stream_id, expected)
        .await
        .expect("recv live output");

    daemon_task.abort();
    let _ = daemon_task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_client_pages_history() {
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
    db.add(ClientRecord {
        client_id: ClientId(Uuid::from_bytes([0x89; 16])),
        public_key: client_pub,
        paired_at: 0,
    })
    .await
    .expect("add client record");

    let deps = ConnectionDeps {
        session_mgr: Arc::new(SessionManager::new(4)),
        client_db: db,
        server_info: ServerInfo {
            server_version: "test".to_string(),
            server_label: None,
        },
        config: Arc::new(Mutex::new(cli_pocket_daemon_core::DaemonConfig::default())),
    };
    let InMemoryTransportPair {
        a: daemon_transport,
        b: client_transport,
    } = InMemoryTransportPair::new(16);
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

    let mut client_transport = client_transport;
    let mut session = connect_paired_client(&mut client_transport, &client_keypair)
        .await
        .expect("connect paired client");

    let terminal_id = create_terminal(
        &mut client_transport,
        &mut session,
        live_output_terminal_cmd(),
    )
    .await
    .expect("create terminal");
    let stream_id = attach_terminal(&mut client_transport, &mut session, terminal_id)
        .await
        .expect("attach terminal");

    send_frame(
        &mut client_transport,
        &mut session,
        &request_frame(
            3,
            RequestBody::SendInput {
                terminal_id,
                bytes: live_output_input().into(),
            },
        ),
    )
    .await
    .expect("send input");

    recv_output_containing(
        &mut client_transport,
        &mut session,
        stream_id,
        b"cli-pocket-live-output",
    )
    .await
    .expect("recv live output");

    send_frame(
        &mut client_transport,
        &mut session,
        &request_frame(
            4,
            RequestBody::ReadHistory {
                terminal_id,
                before: None,
                max_bytes: 8,
            },
        ),
    )
    .await
    .expect("send history request");

    let history_stream = loop {
        let history_response = recv_frame(&mut client_transport, &mut session)
            .await
            .expect("recv history response");
        match history_response.body {
            FrameBody::Response(response) if response.id == RequestId(4) => match response.result {
                Ok(ResponseBody::ReadHistory { stream_id, .. }) => break stream_id,
                other => panic!("expected ReadHistory response body, got {other:?}"),
            },
            FrameBody::StreamData(_) => {}
            other => panic!("expected history Response, got {other:?}"),
        }
    };

    let mut history = Vec::new();
    loop {
        let frame = recv_frame(&mut client_transport, &mut session)
            .await
            .expect("recv history chunk");
        match frame.body {
            FrameBody::StreamData(chunk) => {
                assert_eq!(chunk.stream_id, history_stream);
                assert!(chunk.offset.is_some());
                let start_seq = chunk.seq;
                let bytes = chunk.bytes;
                let last = chunk.last;
                let end_seq = start_seq.0.saturating_add(bytes.len() as u64);
                assert!(end_seq >= start_seq.0);
                history.extend_from_slice(bytes.as_ref());
                if last {
                    break;
                }
            }
            other => panic!("expected history StreamData, got {other:?}"),
        }
    }

    assert_eq!(history, expected_history_tail());

    daemon_task.abort();
    let _ = daemon_task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_client_switches_active_attachment_stream() {
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
    db.add(ClientRecord {
        client_id: ClientId(Uuid::from_bytes([0x90; 16])),
        public_key: client_pub,
        paired_at: 0,
    })
    .await
    .expect("add client record");

    let deps = ConnectionDeps {
        session_mgr: Arc::new(SessionManager::new(4)),
        client_db: db,
        server_info: ServerInfo {
            server_version: "test".to_string(),
            server_label: None,
        },
        config: Arc::new(Mutex::new(cli_pocket_daemon_core::DaemonConfig::default())),
    };
    let InMemoryTransportPair {
        a: daemon_transport,
        b: client_transport,
    } = InMemoryTransportPair::new(16);
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

    let mut client_transport = client_transport;
    let mut session = connect_paired_client(&mut client_transport, &client_keypair)
        .await
        .expect("connect paired client");

    let terminal_a = create_terminal(
        &mut client_transport,
        &mut session,
        delayed_output_terminal_cmd("terminal-a"),
    )
    .await
    .expect("create terminal a");
    let stream_a = attach_terminal(&mut client_transport, &mut session, terminal_a)
        .await
        .expect("attach terminal a");

    let terminal_b = create_terminal(
        &mut client_transport,
        &mut session,
        delayed_output_terminal_cmd("terminal-b"),
    )
    .await
    .expect("create terminal b");
    let stream_b = attach_terminal(&mut client_transport, &mut session, terminal_b)
        .await
        .expect("attach terminal b");

    assert_ne!(
        stream_a, stream_b,
        "new attach should allocate a new stream"
    );

    recv_output_for_active_stream_only(
        &mut client_transport,
        &mut session,
        stream_a,
        b"terminal-a",
        stream_b,
        b"terminal-b",
    )
    .await
    .expect("only active attachment should receive live output");

    daemon_task.abort();
    let _ = daemon_task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_client_attach_unknown_terminal_returns_error_response() {
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
    db.add(ClientRecord {
        client_id: ClientId(Uuid::from_bytes([0x91; 16])),
        public_key: client_pub,
        paired_at: 0,
    })
    .await
    .expect("add client record");

    let deps = ConnectionDeps {
        session_mgr: Arc::new(SessionManager::new(4)),
        client_db: db,
        server_info: ServerInfo {
            server_version: "test".to_string(),
            server_label: None,
        },
        config: Arc::new(Mutex::new(cli_pocket_daemon_core::DaemonConfig::default())),
    };
    let InMemoryTransportPair {
        a: daemon_transport,
        b: client_transport,
    } = InMemoryTransportPair::new(16);
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

    let mut client_transport = client_transport;
    let mut session = connect_paired_client(&mut client_transport, &client_keypair)
        .await
        .expect("connect paired client");

    send_frame(
        &mut client_transport,
        &mut session,
        &request_frame(
            2,
            RequestBody::AttachTerminal {
                terminal_id: TerminalId::new(),
            },
        ),
    )
    .await
    .expect("send attach request");

    let response = recv_frame(&mut client_transport, &mut session)
        .await
        .expect("recv attach response");

    match response.body {
        FrameBody::Response(response) => {
            assert_eq!(response.id, RequestId(2));
            let Err(error) = response.result else {
                panic!("unknown terminal attach should fail");
            };
            assert_eq!(error.code, cli_pocket_proto::ProtocolError::UnknownTerminal);
        }
        other => panic!("expected error response, got {other:?}"),
    }

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

async fn connect_paired_client(
    client_transport: &mut InMemoryTransport,
    client_keypair: &KeyPair,
) -> Result<NoiseSession, String> {
    let mut init =
        NoiseAnonymousInitiator::new(client_keypair).map_err(|e| format!("initiator: {e}"))?;

    let msg1 = init
        .write_handshake()
        .map_err(|e| format!("write msg1: {e}"))?;
    client_transport
        .send(msg1)
        .await
        .map_err(|e| format!("send msg1: {e}"))?;

    let msg2 = recv_with_timeout(client_transport)
        .await
        .ok_or_else(|| "recv msg2: closed".to_string())?;
    init.read_handshake(&msg2)
        .map_err(|e| format!("read msg2: {e}"))?;

    let msg3 = init
        .write_handshake()
        .map_err(|e| format!("write msg3: {e}"))?;
    client_transport
        .send(msg3)
        .await
        .map_err(|e| format!("send msg3: {e}"))?;

    let mut session = init.finish().map_err(|e| format!("finish: {e}"))?;
    let hello = Frame::body(FrameBody::Hello(Hello {
        protocol_min: PROTOCOL_VERSION,
        protocol_max: PROTOCOL_VERSION,
        resume: None,
    }));
    send_frame(client_transport, &mut session, &hello).await?;

    let resp = recv_frame(client_transport, &mut session).await?;
    if !matches!(resp.body, FrameBody::HelloOk(_)) {
        return Err(format!("expected HelloOk, got {:?}", resp.body));
    }

    Ok(session)
}

async fn create_terminal(
    client_transport: &mut InMemoryTransport,
    session: &mut NoiseSession,
    cmd: Vec<String>,
) -> Result<TerminalId, String> {
    let create = request_frame(
        1,
        RequestBody::CreateTerminal {
            params: TerminalCreateParams {
                cols: 80,
                rows: 24,
                cwd: None,
                cmd,
                env: Vec::new(),
            },
        },
    );
    send_frame(client_transport, session, &create).await?;

    let create_ok = recv_frame(client_transport, session).await?;
    match create_ok.body {
        FrameBody::Response(response) => match response.result {
            Ok(ResponseBody::CreateTerminal { info }) => Ok(info.terminal),
            other => Err(format!(
                "expected CreateTerminal response body, got {other:?}"
            )),
        },
        other => Err(format!("expected CreateTerminal response, got {other:?}")),
    }
}

async fn attach_terminal(
    client_transport: &mut InMemoryTransport,
    session: &mut NoiseSession,
    terminal_id: TerminalId,
) -> Result<StreamId, String> {
    let attach = request_frame(2, RequestBody::AttachTerminal { terminal_id });
    send_frame(client_transport, session, &attach).await?;

    let stream_id = loop {
        let attach_ok = recv_frame(client_transport, session).await?;
        match attach_ok.body {
            FrameBody::Response(response) => {
                if response.id != RequestId(2) {
                    return Err(format!("unexpected attach request id {:?}", response.id));
                }
                match response.result {
                    Ok(ResponseBody::AttachTerminal { stream_id, .. }) => break stream_id,
                    other => {
                        return Err(format!(
                            "expected AttachTerminal response body, got {other:?}"
                        ))
                    }
                }
            }
            FrameBody::StreamData(_) => {}
            other => return Err(format!("expected AttachTerminal response, got {other:?}")),
        }
    };

    loop {
        let frame = recv_frame(client_transport, session).await?;
        match frame.body {
            FrameBody::StreamData(chunk) => {
                if chunk.stream_id != stream_id {
                    return Err(format!(
                        "baseline chunk for unexpected stream {:?}",
                        chunk.stream_id
                    ));
                }
                if chunk.last {
                    break;
                }
            }
            other => return Err(format!("expected baseline StreamData, got {other:?}")),
        }
    }

    Ok(stream_id)
}

async fn recv_output_containing(
    client_transport: &mut InMemoryTransport,
    session: &mut NoiseSession,
    stream_id: StreamId,
    marker: &[u8],
) -> Result<(), String> {
    timeout(Duration::from_secs(5), async {
        loop {
            let frame = recv_frame_inner(client_transport, session).await?;
            if let FrameBody::StreamData(chunk) = frame.body {
                if chunk.stream_id == stream_id
                    && chunk
                        .bytes
                        .as_ref()
                        .windows(marker.len())
                        .any(|w| w == marker)
                {
                    return Ok(());
                }
            }
        }
    })
    .await
    .map_err(|_| "timed out waiting for live output".to_string())?
}

async fn recv_output_for_active_stream_only(
    client_transport: &mut InMemoryTransport,
    session: &mut NoiseSession,
    inactive_stream_id: StreamId,
    inactive_marker: &[u8],
    active_stream_id: StreamId,
    active_marker: &[u8],
) -> Result<(), String> {
    timeout(Duration::from_secs(5), async {
        let mut saw_active = false;
        let mut quiet_polls_after_active = 0usize;

        loop {
            match timeout(
                Duration::from_millis(250),
                recv_frame_inner(client_transport, session),
            )
            .await
            {
                Ok(Ok(frame)) => {
                    if let FrameBody::StreamData(chunk) = frame.body {
                        if chunk.stream_id == inactive_stream_id
                            && chunk
                                .bytes
                                .as_ref()
                                .windows(inactive_marker.len())
                                .any(|window| window == inactive_marker)
                        {
                            return Err("inactive attachment emitted live output".to_string());
                        }
                        if chunk.stream_id == active_stream_id
                            && chunk
                                .bytes
                                .as_ref()
                                .windows(active_marker.len())
                                .any(|window| window == active_marker)
                        {
                            saw_active = true;
                        }
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(_) if saw_active => {
                    quiet_polls_after_active += 1;
                    if quiet_polls_after_active >= 4 {
                        return Ok(());
                    }
                }
                Err(_) => {}
            }
        }
    })
    .await
    .map_err(|_| "timed out waiting for active-only live output".to_string())?
}

fn request_frame(request_id: u32, body: RequestBody) -> Frame {
    Frame::body(FrameBody::Request(RequestFrame {
        id: RequestId(request_id),
        body,
    }))
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

#[cfg(unix)]
fn live_output_terminal_cmd() -> Vec<String> {
    vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "IFS= read -r line; printf '%s\\n' \"$line\"; sleep 30".to_string(),
    ]
}

#[cfg(unix)]
fn live_output_input() -> Vec<u8> {
    b"cli-pocket-live-output\n".to_vec()
}

#[cfg(unix)]
fn expected_history_tail() -> Vec<u8> {
    b"output\n".to_vec()
}

#[cfg(windows)]
fn live_output_terminal_cmd() -> Vec<String> {
    vec![
        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".to_string(),
        "-NoLogo".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        "$line = [Console]::In.ReadLine(); Write-Output $line; Start-Sleep -Seconds 30".to_string(),
    ]
}

#[cfg(windows)]
fn live_output_input() -> Vec<u8> {
    b"cli-pocket-live-output\r\n".to_vec()
}

#[cfg(windows)]
fn expected_history_tail() -> Vec<u8> {
    b"output\r\n".to_vec()
}

#[cfg(unix)]
fn delayed_output_terminal_cmd(marker: &str) -> Vec<String> {
    vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        format!("sleep 1; printf '%s\\n' '{marker}'; sleep 30"),
    ]
}

#[cfg(windows)]
fn delayed_output_terminal_cmd(marker: &str) -> Vec<String> {
    vec![
        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".to_string(),
        "-NoLogo".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        format!("Start-Sleep -Seconds 1; Write-Output '{marker}'; Start-Sleep -Seconds 30"),
    ]
}
