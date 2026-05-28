use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{
    ClientEvent, ClientIdentity, ClientResult, Clock, KeyValueStore, Rng, SessionBuilder,
    SessionConfig, SessionEndpoint, Transport,
};
use cli_pocket_crypto::{KeyPair, NoiseResponder, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    AnchorState, CharsetState, ClientId, Color, Frame, FrameBody, HelloOk, MouseMode, ServerInfo,
    SessionId, SgrAttrs, Snapshot, StreamId, StreamSeq, TerminalId, TerminalInfo, TerminalModes,
    PROTOCOL_VERSION,
};
use cli_pocket_transport::InMemoryTransportPair;
use futures_channel::mpsc;
use futures_util::{future::LocalBoxFuture, FutureExt, StreamExt};

#[tokio::test(flavor = "current_thread")]
async fn reconnect_with_resume() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            reconnect_with_resume_inner().await;
        })
        .await;
}

async fn reconnect_with_resume_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let session_id = SessionId::new();
    let info = terminal_info(terminal);
    let stream = StreamId(7);

    let daemon = ReconnectDaemon {
        keypair: server_keypair.clone(),
        session_id,
        stream,
        info: info.clone(),
        connection_count: Arc::new(AtomicU32::new(0)),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            resume_token: None,
            backoff: (10, 100, 20),
        },
        TestClock,
        TestRng,
        TestKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (_session, mut events) = builder.start();

    // First connection: Connecting -> Connected -> TerminalCreated
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &info).await;

    // Daemon closes transport after TerminalCreated, triggering disconnect
    assert_disconnected(&mut events, true).await;

    // Second connection: reconnect with resume token
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;

    // After resume, the daemon sends output proactively (not waiting for input)
    assert_terminal_output(
        &mut events,
        terminal,
        StreamSeq(1),
        b"resumed output".to_vec(),
    )
    .await;
}

#[derive(Clone)]
struct ReconnectDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    stream: StreamId,
    info: TerminalInfo,
    connection_count: Arc<AtomicU32>,
}

impl ReconnectDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<ClientTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let pair = InMemoryTransportPair::new(8);
                let client = ClientTransport(pair.a);
                let server = pair.b;
                tokio::task::spawn_local(async move {
                    let _ = state.run_connection(server).await;
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(
        self,
        mut transport: cli_pocket_transport::InMemoryTransport,
    ) -> ClientResult<()> {
        let conn = self.connection_count.fetch_add(1, Ordering::SeqCst) + 1;

        // Only handle the first two connections; extra ones are from the
        // session loop continuing after the test completes.
        if conn > 2 {
            return Ok(());
        }

        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;

        match hello.body {
            FrameBody::Hello(hello) => {
                let resumed = conn == 2;
                assert_eq!(hello.protocol_min, PROTOCOL_VERSION);
                assert_eq!(hello.protocol_max, PROTOCOL_VERSION);
                if resumed {
                    assert!(
                        hello.resume.is_some(),
                        "expected resume token on second connection"
                    );
                    let token = hello.resume.unwrap();
                    assert_eq!(token.session_id, self.session_id);
                    assert_eq!(token.attachments.len(), 1);
                    assert_eq!(token.attachments[0].terminal, self.info.terminal);
                    assert_eq!(token.attachments[0].last_seq, StreamSeq(0));
                } else {
                    assert!(
                        hello.resume.is_none(),
                        "expected no resume token on first connection"
                    );
                }
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::HelloOk(HelloOk {
                        protocol: PROTOCOL_VERSION,
                        server_info: ServerInfo {
                            server_version: "reconnect-daemon".to_owned(),
                            host_label: Some("test-host".to_owned()),
                        },
                        session_id: self.session_id,
                        resumed,
                    })),
                )
                .await?;
            }
            other => panic!("expected Hello, got {other:?}"),
        }

        if conn == 1 {
            // First connection: TerminalList -> TerminalListOk -> TerminalAttach -> TerminalAttachOk -> close
            let list = recv_encrypted(&mut transport, &mut session).await?;
            assert!(matches!(list.body, FrameBody::TerminalList { .. }));

            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::TerminalListOk {
                    request_id: 1,
                    terminals: vec![self.info.clone()],
                }),
            )
            .await?;

            let attach = recv_encrypted(&mut transport, &mut session).await?;
            match attach.body {
                FrameBody::TerminalAttach {
                    request_id,
                    terminal,
                    since,
                } => {
                    assert_eq!(request_id, 1);
                    assert_eq!(terminal, self.info.terminal);
                    assert_eq!(since, None);
                }
                other => panic!("expected TerminalAttach, got {other:?}"),
            }

            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::TerminalAttachOk {
                    request_id: 1,
                    snapshot: snapshot(StreamSeq(0)),
                    head_seq: StreamSeq(0),
                    stream: self.stream,
                    initial_window: 4096,
                }),
            )
            .await?;

            // Close transport to trigger reconnect
            drop(transport);
            return Ok(());
        }

        // Second connection (resume): expect TerminalAttach with since=Some(StreamSeq(0))
        let attach = recv_encrypted(&mut transport, &mut session).await?;
        match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
            } => {
                assert_eq!(request_id, 1);
                assert_eq!(terminal, self.info.terminal);
                assert_eq!(
                    since,
                    Some(StreamSeq(0)),
                    "expected since=StreamSeq(0) on resume"
                );
            }
            other => panic!("expected TerminalAttach with since, got {other:?}"),
        }

        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalAttachOk {
                request_id: 1,
                snapshot: snapshot(StreamSeq(0)),
                head_seq: StreamSeq(0),
                stream: self.stream,
                initial_window: 4096,
            }),
        )
        .await?;

        // Send output proactively (don't wait for client input)
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Output {
                stream: self.stream,
                seq: StreamSeq(1),
                bytes: b"resumed output".to_vec().into(),
            }),
        )
        .await
    }

    async fn handshake(
        &self,
        transport: &mut cli_pocket_transport::InMemoryTransport,
    ) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = recv_transport(transport).await?;
        responder.read_handshake(&m1)?;
        cli_pocket_transport::Transport::send(transport, responder.write_handshake()?).await?;
        let m3 = recv_transport(transport).await?;
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone, Default)]
struct TestClock;

#[async_trait::async_trait(?Send)]
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        0
    }

    async fn sleep_ms(&self, _ms: u64) {}
}

#[derive(Clone, Default)]
struct TestRng;

impl Rng for TestRng {
    fn fill(&self, dest: &mut [u8]) {
        dest.fill(128);
    }
}

#[derive(Clone, Default)]
struct TestKv;

#[async_trait::async_trait(?Send)]
impl KeyValueStore for TestKv {
    async fn get(&self, _key: &str) -> ClientResult<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn put(&self, _key: &str, _value: &[u8]) -> ClientResult<()> {
        Ok(())
    }

    async fn delete(&self, _key: &str) -> ClientResult<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct AsyncSpawner;

impl SessionSpawner for AsyncSpawner {
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>) {
        tokio::task::spawn_local(fut);
    }
}

struct ClientTransport(cli_pocket_transport::InMemoryTransport);

#[async_trait::async_trait(?Send)]
impl Transport for ClientTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> ClientResult<()> {
        Ok(cli_pocket_transport::Transport::send(&mut self.0, bytes).await?)
    }

    async fn recv(&mut self) -> ClientResult<Option<Vec<u8>>> {
        Ok(cli_pocket_transport::Transport::recv(&mut self.0).await?)
    }

    async fn close(&mut self) -> ClientResult<()> {
        Ok(cli_pocket_transport::Transport::close(&mut self.0).await?)
    }
}

async fn send_encrypted(
    transport: &mut cli_pocket_transport::InMemoryTransport,
    session: &mut NoiseSession,
    frame: Frame,
) -> ClientResult<()> {
    Ok(
        cli_pocket_transport::Transport::send(transport, session.encrypt(&encode_frame(&frame)?)?)
            .await?,
    )
}

async fn recv_encrypted(
    transport: &mut cli_pocket_transport::InMemoryTransport,
    session: &mut NoiseSession,
) -> ClientResult<Frame> {
    let ciphertext = recv_transport(transport).await?;
    Ok(decode_frame(&session.decrypt(&ciphertext)?)?)
}

async fn recv_transport(
    transport: &mut cli_pocket_transport::InMemoryTransport,
) -> ClientResult<Vec<u8>> {
    cli_pocket_transport::Transport::recv(transport)
        .await?
        .ok_or(cli_pocket_client_core::ClientError::Closed)
}

// --- Assertion helpers ---

async fn assert_connecting(events: &mut mpsc::Receiver<ClientEvent>) {
    assert!(
        matches!(events.next().await.unwrap(), ClientEvent::Connecting),
        "expected Connecting"
    );
}

async fn assert_connected(events: &mut mpsc::Receiver<ClientEvent>, expected: SessionId) {
    match events.next().await.unwrap() {
        ClientEvent::Connected { session_id, .. } => assert_eq!(session_id, expected),
        other => panic!("expected Connected, got {other:?}"),
    }
}

async fn assert_disconnected(events: &mut mpsc::Receiver<ClientEvent>, expected_will_retry: bool) {
    match events.next().await.unwrap() {
        ClientEvent::Disconnected { will_retry, reason } => {
            assert_eq!(
                will_retry, expected_will_retry,
                "will_retry mismatch: {reason}"
            );
        }
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

async fn assert_terminal_created(
    events: &mut mpsc::Receiver<ClientEvent>,
    expected: &TerminalInfo,
) {
    match events.next().await.unwrap() {
        ClientEvent::TerminalCreated(actual) => assert_eq!(&actual, expected),
        other => panic!("expected TerminalCreated, got {other:?}"),
    }
}

async fn assert_terminal_output(
    events: &mut mpsc::Receiver<ClientEvent>,
    terminal_id: TerminalId,
    stream_seq: StreamSeq,
    expected_bytes: Vec<u8>,
) {
    match events.next().await.unwrap() {
        ClientEvent::TerminalOutput {
            terminal_id: actual_terminal,
            stream_seq: actual_seq,
            bytes: actual_bytes,
        } => {
            assert_eq!(actual_terminal, terminal_id);
            assert_eq!(actual_seq, stream_seq);
            assert_eq!(actual_bytes.as_ref(), expected_bytes.as_slice());
        }
        other => panic!("expected TerminalOutput, got {other:?}"),
    }
}

// --- Fixture helpers ---

fn terminal_info(terminal: TerminalId) -> TerminalInfo {
    TerminalInfo {
        terminal,
        cols: 120,
        rows: 32,
        created_at_unix_ms: 1_779_321_600_000,
        label: Some("main".to_owned()),
        attached_clients: 1,
    }
}

fn snapshot(head_seq: StreamSeq) -> Snapshot {
    Snapshot {
        cols: 120,
        rows: 32,
        anchor_state: AnchorState {
            cursor: (0, 0),
            sgr: SgrAttrs {
                fg: Some(Color::Palette(7)),
                bg: None,
                bold: false,
                faint: false,
                italic: false,
                underline: false,
                blink: false,
                reverse: false,
                strikethrough: false,
            },
            modes: TerminalModes {
                deccmm_cursor_keys: false,
                autowrap: true,
                alt_screen: false,
                bracketed_paste: false,
                mouse_reporting: MouseMode::Off,
                origin_mode: false,
            },
            charset: CharsetState::default(),
            title: Some("main".to_owned()),
        },
        bytes: Vec::new().into(),
        head_seq,
    }
}
