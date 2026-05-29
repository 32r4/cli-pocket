use async_trait::async_trait;
use bytes::Bytes;
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{
    ClientEvent, ClientIdentity, ClientResult, Clock, KeyValueStore, Rng, SessionBuilder,
    SessionConfig, SessionEndpoint, Transport,
};
use cli_pocket_crypto::{KeyPair, NoiseAnonymousResponder, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    AnchorState, CharsetState, ClientId, Color, Frame, FrameBody, HelloOk, MouseMode, ServerInfo,
    SessionId, SgrAttrs, Snapshot, StreamId, StreamSeq, TerminalId, TerminalInfo, TerminalModes,
    PROTOCOL_VERSION,
};
use cli_pocket_transport::InMemoryTransportPair;
use futures_channel::mpsc;
use futures_util::{future::LocalBoxFuture, FutureExt, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_NOW_MS: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "current_thread")]
async fn session_emits_happy_path_terminal_output() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            session_emits_happy_path_terminal_output_inner().await;
        })
        .await;
}

async fn session_emits_happy_path_terminal_output_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let session_id = SessionId::new();
    let info = terminal_info(terminal);
    let daemon = HappyPathDaemon {
        keypair: server_keypair.clone(),
        session_id,
        stream: StreamId(7),
        info: info.clone(),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(
                cli_pocket_crypto::Identity::from_keypair(&client_keypair).server_id,
            ),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            resume_token: None,
            backoff: (50, 100, 20),
        },
        TestClock,
        TestRng,
        TestKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    let input = Bytes::from_static(b"hello from client\n");

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &info).await;

    let handle = session.terminal().await.unwrap();
    handle.write_input(input.clone()).await.unwrap();

    assert_terminal_output(&mut events, terminal, StreamSeq(1), &input).await;
}

#[derive(Clone)]
struct HappyPathDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    stream: StreamId,
    info: TerminalInfo,
}

impl HappyPathDaemon {
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
                    state.run_connection(server).await.unwrap();
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
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        match hello.body {
            FrameBody::Hello(hello) => {
                assert_eq!(hello.protocol_min, PROTOCOL_VERSION);
                assert_eq!(hello.protocol_max, PROTOCOL_VERSION);
                assert_eq!(hello.resume, None);
            }
            other => panic!("expected Hello, got {other:?}"),
        }

        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::HelloOk(HelloOk {
                protocol: PROTOCOL_VERSION,
                server_info: ServerInfo {
                    server_version: "happy-path-daemon".to_owned(),
                    server_label: Some("test-server".to_owned()),
                },
                session_id: self.session_id,
                resumed: false,
            })),
        )
        .await?;

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

        let input = recv_encrypted(&mut transport, &mut session).await?;
        let bytes = match input.body {
            FrameBody::Input { stream, bytes } => {
                assert_eq!(stream, self.stream);
                assert_eq!(bytes.as_slice(), b"hello from client\n");
                bytes
            }
            other => panic!("expected Input, got {other:?}"),
        };

        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Output {
                stream: self.stream,
                seq: StreamSeq(1),
                bytes,
            }),
        )
        .await
    }

    async fn handshake(
        &self,
        transport: &mut cli_pocket_transport::InMemoryTransport,
    ) -> ClientResult<NoiseSession> {
        let mut responder = NoiseAnonymousResponder::new(&self.keypair)?;
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

#[async_trait(?Send)]
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        TEST_NOW_MS.load(Ordering::Relaxed)
    }

    async fn sleep_ms(&self, ms: u64) {
        tokio::task::yield_now().await;
        TEST_NOW_MS.fetch_add(ms, Ordering::Relaxed);
    }
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

#[async_trait(?Send)]
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

#[async_trait(?Send)]
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

async fn assert_connecting(events: &mut mpsc::Receiver<ClientEvent>) {
    assert!(matches!(
        events.next().await.unwrap(),
        ClientEvent::Connecting
    ));
}

async fn assert_connected(events: &mut mpsc::Receiver<ClientEvent>, expected: SessionId) {
    match events.next().await.unwrap() {
        ClientEvent::Connected {
            session_id,
            server_label,
        } => {
            assert_eq!(session_id, expected);
            assert_eq!(server_label.as_deref(), Some("test-server"));
        }
        other => panic!("expected Connected, got {other:?}"),
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
    bytes: &[u8],
) {
    match events.next().await.unwrap() {
        ClientEvent::TerminalOutput {
            terminal_id: actual_terminal,
            stream_seq: actual_seq,
            bytes: actual_bytes,
        } => {
            assert_eq!(actual_terminal, terminal_id);
            assert_eq!(actual_seq, stream_seq);
            assert_eq!(actual_bytes, *bytes);
        }
        other => panic!("expected TerminalOutput, got {other:?}"),
    }
}

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
