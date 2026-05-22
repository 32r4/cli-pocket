use async_trait::async_trait;
use bytes::Bytes;
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{
    ClientEvent, ClientIdentity, ClientResult, Clock, KeyValueStore, Rng, SessionBuilder,
    SessionConfig, SessionEndpoint, Transport,
};
use cli_pocket_crypto::{KeyPair, NoiseResponder, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    AnchorState, ByeReason, Capabilities, CharsetState, ClientId, Color, Frame, FrameBody,
    HelloErr, HelloOk, MouseMode, ProtocolError, ServerInfo, SessionId, SgrAttrs, Snapshot,
    StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo, TerminalModes,
    PROTOCOL_VERSION,
};
use futures_channel::mpsc;
use futures_util::future::LocalBoxFuture;
use futures_util::task::{waker, ArcWake};
use futures_util::{FutureExt, StreamExt};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::task::Context;

#[test]
fn next_delay_caps_and_grows() {
    assert_eq!(
        cli_pocket_client_core::reconnect::next_delay(1000, 5000, 30),
        3000
    );
    assert_eq!(
        cli_pocket_client_core::reconnect::next_delay(3000, 5000, 30),
        5000
    );
    assert_eq!(
        cli_pocket_client_core::reconnect::next_delay(10_000, 5000, 30),
        5000
    );
}

#[test]
fn jitter_stays_in_band() {
    let low = cli_pocket_client_core::reconnect::jitter(1000, 0);
    assert!((749..=1001).contains(&low));
    let high = cli_pocket_client_core::reconnect::jitter(1000, 255);
    assert!((999..=1251).contains(&high));
}

#[derive(Default, Clone)]
struct TestSpawner {
    tasks: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
}

impl SessionSpawner for TestSpawner {
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>) {
        self.tasks.borrow_mut().push(Box::pin(fut));
    }
}

impl TestSpawner {
    fn poll_all(&self) -> usize {
        let mut tasks = self.tasks.borrow_mut();
        let waker = waker(Arc::new(NoopWake));
        let mut cx = Context::from_waker(&waker);
        let mut finished = 0;
        let mut idx = 0;
        while idx < tasks.len() {
            if tasks[idx].as_mut().poll(&mut cx).is_ready() {
                drop(tasks.swap_remove(idx));
                finished += 1;
            } else {
                idx += 1;
            }
        }
        finished
    }

    fn task_count(&self) -> usize {
        self.tasks.borrow().len()
    }
}

struct NoopWake;

impl ArcWake for NoopWake {
    fn wake_by_ref(_arc_self: &Arc<Self>) {}
}

#[derive(Clone, Default)]
struct DummyClock;

#[async_trait(?Send)]
impl Clock for DummyClock {
    fn now_ms(&self) -> u64 {
        0
    }

    async fn sleep_ms(&self, _ms: u64) {}
}

#[derive(Clone, Default)]
struct DummyRng;

impl Rng for DummyRng {
    fn fill(&self, dest: &mut [u8]) {
        for byte in dest {
            *byte = 128;
        }
    }
}

#[derive(Clone, Default)]
struct DummyKv;

#[async_trait(?Send)]
impl KeyValueStore for DummyKv {
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
struct DummyTransport;

#[async_trait(?Send)]
impl Transport for DummyTransport {
    async fn send(&mut self, _bytes: Vec<u8>) -> ClientResult<()> {
        Ok(())
    }

    async fn recv(&mut self) -> ClientResult<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn close(&mut self) -> ClientResult<()> {
        Ok(())
    }
}

#[test]
fn session_builder_accepts_host_spawner() {
    let spawner = TestSpawner::default();
    let identity = ClientIdentity {
        client_id: ClientId(
            cli_pocket_crypto::Identity::from_keypair(&KeyPair::generate().unwrap()).host_id,
        ),
        keypair: KeyPair::generate().unwrap(),
    };
    let transport_factory = || async { Ok(DummyTransport) }.boxed_local();
    let builder = SessionBuilder::<DummyTransport, _, _, _, _>::new(
        identity,
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: [0; 32],
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        transport_factory,
        spawner.clone(),
    );

    let (_session, _events) = builder.start();
    let _ = spawner.poll_all();
    assert_eq!(spawner.task_count(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn reconnect_replays_resume_token_reattaches_and_delivers_output() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            reconnect_replays_resume_token_reattaches_and_delivers_output_inner().await;
        })
        .await;
}

async fn reconnect_replays_resume_token_reattaches_and_delivers_output_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let first_stream = StreamId(11);
    let resumed_stream = StreamId(22);
    let session_id = SessionId::new();
    let info = terminal_info(terminal);
    let daemon = MockDaemon {
        keypair: server_keypair.clone(),
        first_stream,
        resumed_stream,
        session_id,
        info: info.clone(),
        seen_resume: Rc::new(RefCell::new(None)),
        seen_attach: Rc::new(RefCell::new(None)),
    };

    let client_transport = daemon.transport_factory();
    let spawner = AsyncSpawner;
    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        client_transport,
        spawner,
    );

    let (session, mut events) = builder.start();

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &info).await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), first_stream);
    assert_terminal_output(&mut events, terminal, StreamSeq(7), b"before-drop").await;
    assert_disconnected_retry(&mut events).await;

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_output(&mut events, terminal, StreamSeq(8), b"after-resume").await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), resumed_stream);

    assert_eq!(
        daemon.seen_resume.borrow().clone(),
        Some((session_id, vec![(terminal, StreamSeq(7))]))
    );
    assert_eq!(
        daemon.seen_attach.borrow().clone(),
        Some((terminal, Some(StreamSeq(7))))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_resume_token_attaches_without_existing_terminal_handle() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            startup_resume_token_attaches_without_existing_terminal_handle_inner().await;
        })
        .await;
}

async fn startup_resume_token_attaches_without_existing_terminal_handle_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let session_id = SessionId::new();
    let seen_attach = Rc::new(RefCell::new(None));
    let daemon = StartupResumeDaemon {
        keypair: server_keypair.clone(),
        session_id,
        resumed_stream: StreamId(92),
        seen_attach: Rc::clone(&seen_attach),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: Some(cli_pocket_proto::ResumeToken {
                session_id,
                attachments: vec![cli_pocket_proto::ResumeAttachment {
                    terminal,
                    last_seq: StreamSeq(41),
                }],
            }),
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (_session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;

    for _ in 0..100 {
        if seen_attach.borrow().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        *seen_attach.borrow(),
        Some((terminal, Some(StreamSeq(41)))),
        "startup resume must attach the terminal recorded in the supplied ResumeToken"
    );
    assert_terminal_output(&mut events, terminal, StreamSeq(42), b"startup-resumed").await;
}

#[tokio::test(flavor = "current_thread")]
async fn attach_ok_head_seq_seeds_resume_token_before_any_output() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            attach_ok_head_seq_seeds_resume_token_before_any_output_inner().await;
        })
        .await;
}

async fn attach_ok_head_seq_seeds_resume_token_before_any_output_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let session_id = SessionId::new();
    let seen_resume = Rc::new(RefCell::new(None));
    let daemon = AttachOnlyDisconnectDaemon {
        keypair: server_keypair.clone(),
        session_id,
        info: terminal_info(terminal),
        stream: StreamId(35),
        head_seq: StreamSeq(19),
        seen_resume: Rc::clone(&seen_resume),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (_session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &daemon.info).await;
    assert_disconnected_retry(&mut events).await;
    assert_connecting(&mut events).await;

    for _ in 0..100 {
        if seen_resume.borrow().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        *seen_resume.borrow(),
        Some((session_id, vec![(terminal, StreamSeq(19))])),
        "TerminalAttachOk head_seq must seed the next ResumeToken even before Output"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn original_terminal_handle_uses_reattached_stream_after_reconnect() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            original_terminal_handle_uses_reattached_stream_after_reconnect_inner().await;
        })
        .await;
}

async fn original_terminal_handle_uses_reattached_stream_after_reconnect_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let first_stream = StreamId(31);
    let resumed_stream = StreamId(62);
    let session_id = SessionId::new();
    let input_stream = Rc::new(RefCell::new(None));
    let daemon = StaleHandleInputDaemon {
        keypair: server_keypair.clone(),
        first_stream,
        resumed_stream,
        session_id,
        info: terminal_info(terminal),
        input_stream: Rc::clone(&input_stream),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &daemon.info).await;
    let original_handle = session.terminal().await.unwrap();
    assert_eq!(original_handle.stream_id(), first_stream);
    assert_terminal_output(&mut events, terminal, StreamSeq(7), b"before-drop").await;
    assert_disconnected_retry(&mut events).await;

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_output(&mut events, terminal, StreamSeq(8), b"after-attach").await;

    original_handle
        .write_input(Bytes::from_static(b"echo fresh\n"))
        .await
        .unwrap();

    for _ in 0..100 {
        if input_stream.borrow().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        *input_stream.borrow(),
        Some(resumed_stream),
        "stale TerminalHandle clone must send input to the current reattached stream"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_queued_before_reattach_ok_uses_new_stream() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            input_queued_before_reattach_ok_uses_new_stream_inner().await;
        })
        .await;
}

async fn input_queued_before_reattach_ok_uses_new_stream_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let first_stream = StreamId(51);
    let resumed_stream = StreamId(88);
    let session_id = SessionId::new();
    let attach_seen = Rc::new(RefCell::new(false));
    let input_stream = Rc::new(RefCell::new(None));
    let daemon = QueuedInputReconnectDaemon {
        keypair: server_keypair.clone(),
        first_stream,
        resumed_stream,
        session_id,
        info: terminal_info(terminal),
        attach_seen: Rc::clone(&attach_seen),
        input_stream: Rc::clone(&input_stream),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &daemon.info).await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), first_stream);
    assert_terminal_output(&mut events, terminal, StreamSeq(7), b"before-drop").await;
    assert_disconnected_retry(&mut events).await;

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    for _ in 0..100 {
        if *attach_seen.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(*attach_seen.borrow(), "reattach did not start");

    handle
        .write_input(Bytes::from_static(b"queued-before-attach-ok\n"))
        .await
        .unwrap();

    for _ in 0..100 {
        if input_stream.borrow().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        *input_stream.borrow(),
        Some(resumed_stream),
        "queued input must resolve the current stream after TerminalAttachOk"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_queued_during_failed_reattach_survives_next_connection() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            input_queued_during_failed_reattach_survives_next_connection_inner().await;
        })
        .await;
}

async fn input_queued_during_failed_reattach_survives_next_connection_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let session_id = SessionId::new();
    let failed_attach_seen = Rc::new(RefCell::new(false));
    let recovered_input = Rc::new(RefCell::new(None));
    let daemon = FailedReattachQueuedInputDaemon {
        keypair: server_keypair.clone(),
        first_stream: StreamId(61),
        recovered_stream: StreamId(99),
        session_id,
        info: terminal_info(terminal),
        failed_attach_seen: Rc::clone(&failed_attach_seen),
        recovered_input: Rc::clone(&recovered_input),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    assert_terminal_created(&mut events, &daemon.info).await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), daemon.first_stream);
    assert_terminal_output(
        &mut events,
        terminal,
        StreamSeq(7),
        b"before-failed-reattach",
    )
    .await;
    assert_disconnected_retry(&mut events).await;

    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;
    for _ in 0..100 {
        if *failed_attach_seen.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *failed_attach_seen.borrow(),
        "failed reattach did not start"
    );

    handle
        .write_input(Bytes::from_static(b"survive-failed-reattach\n"))
        .await
        .unwrap();

    assert_disconnected_retry(&mut events).await;
    assert_connecting(&mut events).await;
    assert_connected(&mut events, session_id).await;

    for _ in 0..100 {
        if recovered_input.borrow().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        *recovered_input.borrow(),
        Some((
            daemon.recovered_stream,
            Bytes::from_static(b"survive-failed-reattach\n")
        )),
        "accepted input must remain queued across a failed reattach attempt"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_created_waits_for_attach_ok_before_input_can_use_stream() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            terminal_created_waits_for_attach_ok_before_input_can_use_stream_inner().await;
        })
        .await;
}

async fn terminal_created_waits_for_attach_ok_before_input_can_use_stream_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let info = terminal_info(terminal);
    let input_streams = Rc::new(RefCell::new(Vec::<StreamId>::new()));
    let attach_seen = Rc::new(RefCell::new(false));
    let attach_ok_sent = Rc::new(RefCell::new(false));
    let daemon = DelayedAttachDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        info: info.clone(),
        attached_stream: StreamId(44),
        input_streams: Rc::clone(&input_streams),
        attach_seen: Rc::clone(&attach_seen),
        attach_ok_sent: Rc::clone(&attach_ok_sent),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;

    for _ in 0..100 {
        if *attach_seen.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *attach_seen.borrow(),
        "mock daemon did not receive TerminalAttach"
    );

    assert!(
        session.terminal().await.is_none(),
        "terminal handle must not be exposed before TerminalAttachOk supplies the real stream"
    );
    let early_event = events.next().now_or_never();
    assert!(
        !matches!(early_event, Some(Some(ClientEvent::TerminalCreated(_)))),
        "TerminalCreated must not be emitted before TerminalAttachOk supplies the real stream"
    );

    for _ in 0..100 {
        if *attach_ok_sent.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *attach_ok_sent.borrow(),
        "mock daemon did not send TerminalAttachOk"
    );

    assert_terminal_created(&mut events, &info).await;
    assert!(
        !input_streams.borrow().contains(&StreamId(0)),
        "client sent input to placeholder stream 0 before TerminalAttachOk"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_attach_ok_with_wrong_request_id_is_ignored() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            terminal_attach_ok_with_wrong_request_id_is_ignored_inner().await;
        })
        .await;
}

async fn terminal_attach_ok_with_wrong_request_id_is_ignored_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let info = terminal_info(terminal);
    let wrong_ok_sent = Rc::new(RefCell::new(false));
    let allow_correct_ok = Rc::new(RefCell::new(false));
    let daemon = WrongAttachOkRequestDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        info: info.clone(),
        wrong_stream: StreamId(144),
        correct_stream: StreamId(145),
        wrong_ok_sent: Rc::clone(&wrong_ok_sent),
        allow_correct_ok: Rc::clone(&allow_correct_ok),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;

    for _ in 0..100 {
        if *wrong_ok_sent.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *wrong_ok_sent.borrow(),
        "mock daemon did not send the wrong TerminalAttachOk"
    );
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(
        session.terminal().await.is_none(),
        "wrong TerminalAttachOk request_id must not expose a terminal handle"
    );
    let early_event = events.next().now_or_never();
    assert!(
        !matches!(early_event, Some(Some(ClientEvent::TerminalCreated(_)))),
        "wrong TerminalAttachOk request_id must not emit TerminalCreated"
    );

    *allow_correct_ok.borrow_mut() = true;
    assert_terminal_created(&mut events, &info).await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), daemon.correct_stream);
}

#[tokio::test(flavor = "current_thread")]
async fn stale_attach_ok_with_existing_handle_does_not_rebind_stream() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            stale_attach_ok_with_existing_handle_does_not_rebind_stream_inner().await;
        })
        .await;
}

async fn stale_attach_ok_with_existing_handle_does_not_rebind_stream_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let info = terminal_info(terminal);
    let wrong_ok_sent = Rc::new(RefCell::new(false));
    let allow_correct_ok = Rc::new(RefCell::new(false));
    let daemon = WrongAttachOkExistingHandleDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        info: info.clone(),
        first_stream: StreamId(151),
        wrong_stream: StreamId(152),
        correct_stream: StreamId(153),
        wrong_ok_sent: Rc::clone(&wrong_ok_sent),
        allow_correct_ok: Rc::clone(&allow_correct_ok),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    assert_terminal_created(&mut events, &info).await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), daemon.first_stream);
    assert_terminal_output(&mut events, terminal, StreamSeq(7), b"before-drop").await;
    assert_disconnected_retry(&mut events).await;

    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    for _ in 0..100 {
        if *wrong_ok_sent.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *wrong_ok_sent.borrow(),
        "mock daemon did not send the wrong TerminalAttachOk"
    );
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }

    assert_eq!(
        handle.stream_id(),
        daemon.first_stream,
        "wrong TerminalAttachOk request_id must not rebind an existing terminal handle"
    );
    let early_event = events.next().now_or_never();
    assert!(
        early_event.is_none(),
        "wrong TerminalAttachOk request_id must not emit events before a matching attach ok"
    );

    *allow_correct_ok.borrow_mut() = true;
    assert_terminal_output(&mut events, terminal, StreamSeq(9), b"after-correct-attach").await;
    assert_eq!(handle.stream_id(), daemon.correct_stream);
}

#[tokio::test(flavor = "current_thread")]
async fn create_terminal_sends_create_and_waits_for_attach_ok() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            create_terminal_sends_create_and_waits_for_attach_ok_inner().await;
        })
        .await;
}

async fn create_terminal_sends_create_and_waits_for_attach_ok_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let params = TerminalCreateParams {
        cols: 100,
        rows: 40,
        cwd: Some("/tmp".to_owned()),
        cmd: vec!["sh".to_owned()],
        env: vec![("TERM".to_owned(), "xterm-256color".to_owned())],
        scrollback_bytes: Some(8192),
    };
    let create_seen = Rc::new(RefCell::new(false));
    let attach_seen = Rc::new(RefCell::new(false));
    let attach_ok_sent = Rc::new(RefCell::new(false));
    let daemon = CreateTerminalDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        terminal,
        stream: StreamId(77),
        params: params.clone(),
        create_seen: Rc::clone(&create_seen),
        attach_seen: Rc::clone(&attach_seen),
        attach_ok_sent: Rc::clone(&attach_ok_sent),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;

    session.create_terminal(params).await.unwrap();

    for _ in 0..100 {
        if *create_seen.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *create_seen.borrow(),
        "mock daemon did not receive TerminalCreate"
    );

    for _ in 0..100 {
        if *attach_seen.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *attach_seen.borrow(),
        "mock daemon did not receive TerminalAttach"
    );
    assert!(
        session.terminal().await.is_none(),
        "terminal handle must not be exposed before TerminalAttachOk"
    );
    let early_event = events.next().now_or_never();
    assert!(
        !matches!(early_event, Some(Some(ClientEvent::TerminalCreated(_)))),
        "TerminalCreated must not be emitted before TerminalAttachOk"
    );

    for _ in 0..100 {
        if *attach_ok_sent.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *attach_ok_sent.borrow(),
        "mock daemon did not send TerminalAttachOk"
    );

    match events.next().await.unwrap() {
        ClientEvent::TerminalCreated(info) => {
            assert_eq!(info.terminal, terminal);
            assert_eq!(info.cols, 100);
            assert_eq!(info.rows, 40);
        }
        other => panic!("expected TerminalCreated, got {other:?}"),
    }
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.terminal_id(), terminal);
    assert_eq!(handle.stream_id(), daemon.stream);
}

#[tokio::test(flavor = "current_thread")]
async fn create_terminal_replaces_existing_handle_after_matching_attach_ok() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            create_terminal_replaces_existing_handle_after_matching_attach_ok_inner().await;
        })
        .await;
}

async fn create_terminal_replaces_existing_handle_after_matching_attach_ok_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let original_terminal = TerminalId::new();
    let new_terminal = TerminalId::new();
    let params = terminal_params(104, 38, "second");
    let daemon = CreateSecondTerminalDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        original_info: terminal_info(original_terminal),
        original_stream: StreamId(177),
        new_terminal,
        new_stream: StreamId(233),
        params: params.clone(),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    assert_terminal_created(&mut events, &daemon.original_info).await;
    let original_handle = session.terminal().await.unwrap();
    assert_eq!(original_handle.terminal_id(), original_terminal);
    assert_eq!(original_handle.stream_id(), daemon.original_stream);

    session.create_terminal(params.clone()).await.unwrap();
    assert_terminal_created_with_size(&mut events, new_terminal, params.cols, params.rows).await;

    let current_handle = session.terminal().await.unwrap();
    assert_eq!(current_handle.terminal_id(), new_terminal);
    assert_eq!(current_handle.stream_id(), daemon.new_stream);
}

#[tokio::test(flavor = "current_thread")]
async fn pending_create_is_discarded_when_connection_drops_before_response() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            pending_create_is_discarded_when_connection_drops_before_response_inner().await;
        })
        .await;
}

async fn pending_create_is_discarded_when_connection_drops_before_response_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let stale_params = terminal_params(72, 18, "stale");
    let fresh_params = terminal_params(132, 43, "fresh");
    let first_create_seen = Rc::new(RefCell::new(false));
    let second_create = Rc::new(RefCell::new(None));
    let daemon = DroppedCreateReconnectDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        terminal,
        stream: StreamId(117),
        stale_params: stale_params.clone(),
        fresh_params: fresh_params.clone(),
        first_create_seen: Rc::clone(&first_create_seen),
        second_create: Rc::clone(&second_create),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;

    session.create_terminal(stale_params).await.unwrap();
    for _ in 0..100 {
        if *first_create_seen.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *first_create_seen.borrow(),
        "mock daemon did not receive the dropped TerminalCreate"
    );
    assert_disconnected_retry(&mut events).await;

    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    session.create_terminal(fresh_params.clone()).await.unwrap();
    assert_terminal_created_with_size(&mut events, terminal, fresh_params.cols, fresh_params.rows)
        .await;

    assert_eq!(*second_create.borrow(), Some((2, fresh_params)));
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_create_err_removes_pending_create_and_surfaces_error() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            terminal_create_err_removes_pending_create_and_surfaces_error_inner().await;
        })
        .await;
}

async fn terminal_create_err_removes_pending_create_and_surfaces_error_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let params = terminal_params(90, 30, "rejected");
    let daemon = CreateErrDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        terminal,
        stream: StreamId(213),
        params: params.clone(),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;

    session.create_terminal(params).await.unwrap();
    assert_error_contains(&mut events, "terminal create failed").await;
    assert_error_contains(&mut events, "missing metadata").await;

    assert!(
        session.terminal().await.is_none(),
        "TerminalCreateErr must clear the pending create so a late matching TerminalCreateOk cannot create a terminal"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_create_ok_after_err_for_known_terminal_is_ignored() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            terminal_create_ok_after_err_for_known_terminal_is_ignored_inner().await;
        })
        .await;
}

async fn terminal_create_ok_after_err_for_known_terminal_is_ignored_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let params = terminal_params(90, 30, "stale-known");
    let attach_after_stale_ok = Rc::new(RefCell::new(false));
    let stale_observed = Rc::new(RefCell::new(false));
    let daemon = CreateErrThenKnownTerminalOkDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        info: terminal_info(terminal),
        original_stream: StreamId(311),
        stale_stream: StreamId(312),
        params: params.clone(),
        attach_after_stale_ok: Rc::clone(&attach_after_stale_ok),
        stale_observed: Rc::clone(&stale_observed),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    assert_terminal_created(&mut events, &daemon.info).await;
    let handle = session.terminal().await.unwrap();
    assert_eq!(handle.stream_id(), daemon.original_stream);

    session.create_terminal(params).await.unwrap();
    assert_error_contains(&mut events, "terminal create failed").await;

    for _ in 0..100 {
        if *stale_observed.borrow() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        *stale_observed.borrow(),
        "mock daemon did not observe the stale TerminalCreateOk aftermath"
    );
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(
        !*attach_after_stale_ok.borrow(),
        "stale TerminalCreateOk for a known terminal must not send TerminalAttach"
    );
    assert_eq!(
        session.terminal().await.unwrap().stream_id(),
        daemon.original_stream,
        "stale TerminalCreateOk must not replace the existing terminal stream"
    );
    let late_event = events.next().now_or_never();
    assert!(
        !matches!(late_event, Some(Some(ClientEvent::TerminalCreated(_)))),
        "stale TerminalCreateOk must not emit TerminalCreated"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hello_resume_stale_falls_back_to_attach_original_terminal() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            hello_resume_stale_falls_back_to_attach_original_terminal_inner().await;
        })
        .await;
}

async fn hello_resume_stale_falls_back_to_attach_original_terminal_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let intended = TerminalId::new();
    let wrong = TerminalId::new();
    let fallback_attach = Rc::new(RefCell::new(None));
    let daemon = HelloResumeStaleDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        intended_info: terminal_info(intended),
        wrong_info: terminal_info(wrong),
        fallback_attach: Rc::clone(&fallback_attach),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (_session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    assert_terminal_created(&mut events, &daemon.intended_info).await;
    assert_terminal_output(&mut events, intended, StreamSeq(7), b"before-stale").await;
    assert_disconnected_retry(&mut events).await;
    assert_connecting(&mut events).await;
    assert_disconnected_retry(&mut events).await;

    for _ in 0..100 {
        if fallback_attach.borrow().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        *fallback_attach.borrow(),
        Some((intended, None)),
        "fresh fallback must attach the originally active terminal"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bye_resume_stale_clears_resume_token_before_retry() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            bye_resume_stale_clears_resume_token_before_retry_inner().await;
        })
        .await;
}

async fn bye_resume_stale_clears_resume_token_before_retry_inner() {
    let server_keypair = KeyPair::generate().unwrap();
    let client_keypair = KeyPair::generate().unwrap();
    let terminal = TerminalId::new();
    let third_resume = Rc::new(RefCell::new(ObservedResume::Pending));
    let daemon = ByeResumeStaleDaemon {
        keypair: server_keypair.clone(),
        session_id: SessionId::new(),
        info: terminal_info(terminal),
        third_resume: Rc::clone(&third_resume),
    };

    let builder = SessionBuilder::new(
        ClientIdentity {
            client_id: ClientId(cli_pocket_crypto::Identity::from_keypair(&client_keypair).host_id),
            keypair: client_keypair,
        },
        SessionConfig {
            endpoint: SessionEndpoint::Direct("ws://localhost".to_owned()),
            server_public: server_keypair.public,
            resume_token: None,
            capabilities: Capabilities::NONE,
            backoff: (50, 100, 20),
        },
        DummyClock,
        DummyRng,
        DummyKv,
        daemon.transport_factory(),
        AsyncSpawner,
    );

    let (_session, mut events) = builder.start();
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    assert_terminal_created(&mut events, &daemon.info).await;
    assert_terminal_output(&mut events, terminal, StreamSeq(7), b"before-bye-stale").await;
    assert_disconnected_retry(&mut events).await;
    assert_connecting(&mut events).await;
    assert_connected(&mut events, daemon.session_id).await;
    assert_disconnected_retry(&mut events).await;

    for _ in 0..100 {
        if !matches!(*third_resume.borrow(), ObservedResume::Pending) {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert!(
        matches!(*third_resume.borrow(), ObservedResume::SeenNone),
        "ResumeStale Bye must clear the stale token before reconnect"
    );
}

#[derive(Clone)]
struct AsyncSpawner;

impl SessionSpawner for AsyncSpawner {
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>) {
        tokio::task::spawn_local(fut);
    }
}

#[derive(Clone)]
struct MockDaemon {
    keypair: KeyPair,
    first_stream: StreamId,
    resumed_stream: StreamId,
    session_id: SessionId,
    info: TerminalInfo,
    seen_resume: SeenResume,
    seen_attach: SeenAttach,
}

type SeenResume = Rc<RefCell<Option<(SessionId, Vec<(TerminalId, StreamSeq)>)>>>;
type SeenAttach = Rc<RefCell<Option<(TerminalId, Option<StreamSeq>)>>>;
type ObservedAttach = Rc<RefCell<Option<(TerminalId, Option<StreamSeq>)>>>;

#[derive(Clone)]
struct StartupResumeDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    resumed_stream: StreamId,
    seen_attach: SeenAttach,
}

impl StartupResumeDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };
        assert!(resume.is_some());
        send_hello_ok(&mut transport, &mut session, self.session_id, true).await?;

        let attach = recv_encrypted(&mut transport, &mut session).await?;
        match attach.body {
            FrameBody::TerminalAttach {
                terminal, since, ..
            } => {
                *self.seen_attach.borrow_mut() = Some((terminal, since));
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        }

        send_attach_ok(
            &mut transport,
            &mut session,
            self.resumed_stream,
            StreamSeq(41),
        )
        .await?;
        send_output(
            &mut transport,
            &mut session,
            self.resumed_stream,
            StreamSeq(42),
            b"startup-resumed",
        )
        .await?;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::ServerShutdown,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct AttachOnlyDisconnectDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    info: TerminalInfo,
    stream: StreamId,
    head_seq: StreamSeq,
    seen_resume: SeenResume,
}

impl AttachOnlyDisconnectDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        if attempt == 1 {
            assert_eq!(resume, None);
            send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
            let list = recv_encrypted(&mut transport, &mut session).await?;
            assert!(matches!(list.body, FrameBody::TerminalList { .. }));
            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::TerminalListOk {
                    request_id: 1,
                    terminals: vec![self.info],
                }),
            )
            .await?;
            assert!(matches!(
                recv_encrypted(&mut transport, &mut session).await?.body,
                FrameBody::TerminalAttach { .. }
            ));
            send_attach_ok(&mut transport, &mut session, self.stream, self.head_seq).await?;
            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::Bye {
                    reason: ByeReason::ServerShutdown,
                }),
            )
            .await
        } else {
            *self.seen_resume.borrow_mut() = resume.map(|resume| {
                (
                    resume.session_id,
                    resume
                        .attachments
                        .into_iter()
                        .map(|attachment| (attachment.terminal, attachment.last_seq))
                        .collect(),
                )
            });
            Ok(())
        }
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

impl MockDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;

        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        if attempt == 1 {
            assert_eq!(resume, None);
            self.run_first_connection(&mut transport, &mut session)
                .await?;
        } else {
            self.run_resumed_connection(&mut transport, &mut session, resume)
                .await?;
        }

        Ok(())
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }

    async fn run_first_connection(
        &self,
        transport: &mut MemoryTransport,
        session: &mut NoiseSession,
    ) -> ClientResult<()> {
        send_hello_ok(transport, session, self.session_id, false).await?;
        let list = recv_encrypted(transport, session).await?;
        assert!(matches!(list.body, FrameBody::TerminalList { .. }));
        send_encrypted(
            transport,
            session,
            Frame::body(FrameBody::TerminalListOk {
                request_id: 0,
                terminals: vec![self.info.clone()],
            }),
        )
        .await?;
        let attach = recv_encrypted(transport, session).await?;
        assert!(matches!(attach.body, FrameBody::TerminalAttach { .. }));
        send_attach_ok(transport, session, self.first_stream, StreamSeq(6)).await?;
        send_output(
            transport,
            session,
            self.first_stream,
            StreamSeq(7),
            b"before-drop",
        )
        .await?;
        send_encrypted(
            transport,
            session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::ProtocolError(ProtocolError::BackpressureExceeded),
            }),
        )
        .await
    }

    async fn run_resumed_connection(
        &self,
        transport: &mut MemoryTransport,
        session: &mut NoiseSession,
        resume: Option<cli_pocket_proto::ResumeToken>,
    ) -> ClientResult<()> {
        let resume = resume.expect("second connection must send resume token");
        *self.seen_resume.borrow_mut() = Some((
            resume.session_id,
            resume
                .attachments
                .iter()
                .map(|attachment| (attachment.terminal, attachment.last_seq))
                .collect(),
        ));
        send_hello_ok(transport, session, self.session_id, true).await?;

        let attach = recv_encrypted(transport, session).await?;
        match attach.body {
            FrameBody::TerminalAttach {
                terminal, since, ..
            } => {
                *self.seen_attach.borrow_mut() = Some((terminal, since));
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        }

        send_attach_ok(transport, session, self.resumed_stream, StreamSeq(7)).await?;
        send_output(
            transport,
            session,
            self.resumed_stream,
            StreamSeq(8),
            b"after-resume",
        )
        .await
    }
}

#[derive(Clone)]
struct StaleHandleInputDaemon {
    keypair: KeyPair,
    first_stream: StreamId,
    resumed_stream: StreamId,
    session_id: SessionId,
    info: TerminalInfo,
    input_stream: Rc<RefCell<Option<StreamId>>>,
}

impl StaleHandleInputDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        if attempt == 1 {
            assert_eq!(resume, None);
            send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
            let list = recv_encrypted(&mut transport, &mut session).await?;
            assert!(matches!(list.body, FrameBody::TerminalList { .. }));
            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::TerminalListOk {
                    request_id: 1,
                    terminals: vec![self.info],
                }),
            )
            .await?;
            assert!(matches!(
                recv_encrypted(&mut transport, &mut session).await?.body,
                FrameBody::TerminalAttach { .. }
            ));
            send_attach_ok(
                &mut transport,
                &mut session,
                self.first_stream,
                StreamSeq(6),
            )
            .await?;
            send_output(
                &mut transport,
                &mut session,
                self.first_stream,
                StreamSeq(7),
                b"before-drop",
            )
            .await?;
            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::Bye {
                    reason: ByeReason::ServerShutdown,
                }),
            )
            .await
        } else {
            assert!(resume.is_some());
            send_hello_ok(&mut transport, &mut session, self.session_id, true).await?;
            assert!(matches!(
                recv_encrypted(&mut transport, &mut session).await?.body,
                FrameBody::TerminalAttach { .. }
            ));
            send_attach_ok(
                &mut transport,
                &mut session,
                self.resumed_stream,
                StreamSeq(7),
            )
            .await?;
            send_output(
                &mut transport,
                &mut session,
                self.resumed_stream,
                StreamSeq(8),
                b"after-attach",
            )
            .await?;

            loop {
                let frame = recv_encrypted(&mut transport, &mut session).await?;
                if let FrameBody::Input { stream, .. } = frame.body {
                    *self.input_stream.borrow_mut() = Some(stream);
                    return Ok(());
                }
            }
        }
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct QueuedInputReconnectDaemon {
    keypair: KeyPair,
    first_stream: StreamId,
    resumed_stream: StreamId,
    session_id: SessionId,
    info: TerminalInfo,
    attach_seen: Rc<RefCell<bool>>,
    input_stream: Rc<RefCell<Option<StreamId>>>,
}

impl QueuedInputReconnectDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        if attempt == 1 {
            assert_eq!(resume, None);
            send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
            let list = recv_encrypted(&mut transport, &mut session).await?;
            assert!(matches!(list.body, FrameBody::TerminalList { .. }));
            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::TerminalListOk {
                    request_id: 1,
                    terminals: vec![self.info],
                }),
            )
            .await?;
            assert!(matches!(
                recv_encrypted(&mut transport, &mut session).await?.body,
                FrameBody::TerminalAttach { .. }
            ));
            send_attach_ok(
                &mut transport,
                &mut session,
                self.first_stream,
                StreamSeq(6),
            )
            .await?;
            send_output(
                &mut transport,
                &mut session,
                self.first_stream,
                StreamSeq(7),
                b"before-drop",
            )
            .await?;
            send_encrypted(
                &mut transport,
                &mut session,
                Frame::body(FrameBody::Bye {
                    reason: ByeReason::ServerShutdown,
                }),
            )
            .await
        } else {
            assert!(resume.is_some());
            send_hello_ok(&mut transport, &mut session, self.session_id, true).await?;
            assert!(matches!(
                recv_encrypted(&mut transport, &mut session).await?.body,
                FrameBody::TerminalAttach { .. }
            ));
            *self.attach_seen.borrow_mut() = true;

            for _ in 0..20 {
                if let Some(frame) = try_recv_encrypted(&mut transport, &mut session)? {
                    if let FrameBody::Input { stream, .. } = frame.body {
                        *self.input_stream.borrow_mut() = Some(stream);
                    }
                }
                tokio::task::yield_now().await;
            }

            send_attach_ok(
                &mut transport,
                &mut session,
                self.resumed_stream,
                StreamSeq(7),
            )
            .await?;

            loop {
                let frame = recv_encrypted(&mut transport, &mut session).await?;
                if let FrameBody::Input { stream, .. } = frame.body {
                    *self.input_stream.borrow_mut() = Some(stream);
                    return Ok(());
                }
            }
        }
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct FailedReattachQueuedInputDaemon {
    keypair: KeyPair,
    first_stream: StreamId,
    recovered_stream: StreamId,
    session_id: SessionId,
    info: TerminalInfo,
    failed_attach_seen: Rc<RefCell<bool>>,
    recovered_input: Rc<RefCell<Option<(StreamId, Bytes)>>>,
}

impl FailedReattachQueuedInputDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        match attempt {
            1 => {
                assert_eq!(resume, None);
                send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
                let list = recv_encrypted(&mut transport, &mut session).await?;
                assert!(matches!(list.body, FrameBody::TerminalList { .. }));
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::TerminalListOk {
                        request_id: 1,
                        terminals: vec![self.info],
                    }),
                )
                .await?;
                assert!(matches!(
                    recv_encrypted(&mut transport, &mut session).await?.body,
                    FrameBody::TerminalAttach { .. }
                ));
                send_attach_ok(
                    &mut transport,
                    &mut session,
                    self.first_stream,
                    StreamSeq(6),
                )
                .await?;
                send_output(
                    &mut transport,
                    &mut session,
                    self.first_stream,
                    StreamSeq(7),
                    b"before-failed-reattach",
                )
                .await?;
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::Bye {
                        reason: ByeReason::ServerShutdown,
                    }),
                )
                .await
            }
            2 => {
                assert!(resume.is_some());
                send_hello_ok(&mut transport, &mut session, self.session_id, true).await?;
                assert!(matches!(
                    recv_encrypted(&mut transport, &mut session).await?.body,
                    FrameBody::TerminalAttach { .. }
                ));
                *self.failed_attach_seen.borrow_mut() = true;

                for _ in 0..20 {
                    if let Some(frame) = try_recv_encrypted(&mut transport, &mut session)? {
                        if let FrameBody::Input { .. } = frame.body {
                            panic!("input must not be sent before TerminalAttachOk");
                        }
                    }
                    tokio::task::yield_now().await;
                }

                transport.close().await
            }
            3 => {
                assert!(resume.is_some());
                send_hello_ok(&mut transport, &mut session, self.session_id, true).await?;
                assert!(matches!(
                    recv_encrypted(&mut transport, &mut session).await?.body,
                    FrameBody::TerminalAttach { .. }
                ));
                send_attach_ok(
                    &mut transport,
                    &mut session,
                    self.recovered_stream,
                    StreamSeq(7),
                )
                .await?;

                loop {
                    let frame = recv_encrypted(&mut transport, &mut session).await?;
                    if let FrameBody::Input { stream, bytes } = frame.body {
                        *self.recovered_input.borrow_mut() =
                            Some((stream, Bytes::from(bytes.into_vec())));
                        return Ok(());
                    }
                }
            }
            _ => Ok(()),
        }
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct DelayedAttachDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    info: TerminalInfo,
    attached_stream: StreamId,
    input_streams: Rc<RefCell<Vec<StreamId>>>,
    attach_seen: Rc<RefCell<bool>>,
    attach_ok_sent: Rc<RefCell<bool>>,
}

impl DelayedAttachDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
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
            FrameBody::TerminalAttach { .. } => {}
            other => panic!("expected TerminalAttach, got {other:?}"),
        }
        *self.attach_seen.borrow_mut() = true;

        for _ in 0..20 {
            if let Some(frame) = try_recv_encrypted(&mut transport, &mut session)? {
                if let FrameBody::Input { stream, .. } = frame.body {
                    self.input_streams.borrow_mut().push(stream);
                }
            }
            tokio::task::yield_now().await;
        }

        send_attach_ok(
            &mut transport,
            &mut session,
            self.attached_stream,
            StreamSeq(0),
        )
        .await?;
        *self.attach_ok_sent.borrow_mut() = true;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::Revoked,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct WrongAttachOkRequestDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    info: TerminalInfo,
    wrong_stream: StreamId,
    correct_stream: StreamId,
    wrong_ok_sent: Rc<RefCell<bool>>,
    allow_correct_ok: Rc<RefCell<bool>>,
}

impl WrongAttachOkRequestDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
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
        let request_id = match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
            } => {
                assert_eq!(terminal, self.info.terminal);
                assert_eq!(since, None);
                request_id
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        };
        send_attach_ok_with_request(
            &mut transport,
            &mut session,
            request_id.saturating_add(1),
            self.wrong_stream,
            StreamSeq(0),
        )
        .await?;
        *self.wrong_ok_sent.borrow_mut() = true;

        for _ in 0..100 {
            if *self.allow_correct_ok.borrow() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            *self.allow_correct_ok.borrow(),
            "test did not permit the correct TerminalAttachOk"
        );

        send_attach_ok_with_request(
            &mut transport,
            &mut session,
            request_id,
            self.correct_stream,
            StreamSeq(0),
        )
        .await?;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::Revoked,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct WrongAttachOkExistingHandleDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    info: TerminalInfo,
    first_stream: StreamId,
    wrong_stream: StreamId,
    correct_stream: StreamId,
    wrong_ok_sent: Rc<RefCell<bool>>,
    allow_correct_ok: Rc<RefCell<bool>>,
}

impl WrongAttachOkExistingHandleDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        match attempt {
            1 => {
                assert_eq!(resume, None);
                self.run_first_connection(&mut transport, &mut session)
                    .await
            }
            2 => {
                assert!(resume.is_some());
                self.run_resumed_connection(&mut transport, &mut session)
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn run_first_connection(
        &self,
        transport: &mut MemoryTransport,
        session: &mut NoiseSession,
    ) -> ClientResult<()> {
        send_hello_ok(transport, session, self.session_id, false).await?;
        let list = recv_encrypted(transport, session).await?;
        assert!(matches!(list.body, FrameBody::TerminalList { .. }));
        send_encrypted(
            transport,
            session,
            Frame::body(FrameBody::TerminalListOk {
                request_id: 1,
                terminals: vec![self.info.clone()],
            }),
        )
        .await?;
        assert!(matches!(
            recv_encrypted(transport, session).await?.body,
            FrameBody::TerminalAttach { .. }
        ));
        send_attach_ok(transport, session, self.first_stream, StreamSeq(6)).await?;
        send_output(
            transport,
            session,
            self.first_stream,
            StreamSeq(7),
            b"before-drop",
        )
        .await?;
        send_encrypted(
            transport,
            session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::ServerShutdown,
            }),
        )
        .await
    }

    async fn run_resumed_connection(
        &self,
        transport: &mut MemoryTransport,
        session: &mut NoiseSession,
    ) -> ClientResult<()> {
        send_hello_ok(transport, session, self.session_id, true).await?;
        let attach = recv_encrypted(transport, session).await?;
        let request_id = match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
                ..
            } => {
                assert_eq!(terminal, self.info.terminal);
                assert_eq!(since, Some(StreamSeq(7)));
                request_id
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        };

        send_attach_ok_with_request(
            transport,
            session,
            request_id.saturating_add(1),
            self.wrong_stream,
            StreamSeq(8),
        )
        .await?;
        *self.wrong_ok_sent.borrow_mut() = true;

        for _ in 0..100 {
            if *self.allow_correct_ok.borrow() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            *self.allow_correct_ok.borrow(),
            "test did not permit the correct TerminalAttachOk"
        );

        send_attach_ok_with_request(
            transport,
            session,
            request_id,
            self.correct_stream,
            StreamSeq(8),
        )
        .await?;
        send_output(
            transport,
            session,
            self.correct_stream,
            StreamSeq(9),
            b"after-correct-attach",
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct CreateTerminalDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    terminal: TerminalId,
    stream: StreamId,
    params: TerminalCreateParams,
    create_seen: Rc<RefCell<bool>>,
    attach_seen: Rc<RefCell<bool>>,
    attach_ok_sent: Rc<RefCell<bool>>,
}

impl CreateTerminalDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
        let list = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(list.body, FrameBody::TerminalList { .. }));
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalListOk {
                request_id: 1,
                terminals: Vec::new(),
            }),
        )
        .await?;

        let create = recv_encrypted(&mut transport, &mut session).await?;
        let request_id = match create.body {
            FrameBody::TerminalCreate { request_id, params } => {
                assert_eq!(params, self.params);
                *self.create_seen.borrow_mut() = true;
                request_id
            }
            other => panic!("expected TerminalCreate, got {other:?}"),
        };
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateOk {
                request_id,
                terminal: self.terminal,
                stream: self.stream,
            }),
        )
        .await?;

        let attach = recv_encrypted(&mut transport, &mut session).await?;
        let attach_request_id = match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
            } => {
                assert_eq!(terminal, self.terminal);
                assert_eq!(since, None);
                *self.attach_seen.borrow_mut() = true;
                request_id
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        };

        for _ in 0..20 {
            tokio::task::yield_now().await;
        }

        send_attach_ok_with_request(
            &mut transport,
            &mut session,
            attach_request_id,
            self.stream,
            StreamSeq(0),
        )
        .await?;
        *self.attach_ok_sent.borrow_mut() = true;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::Revoked,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct CreateSecondTerminalDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    original_info: TerminalInfo,
    original_stream: StreamId,
    new_terminal: TerminalId,
    new_stream: StreamId,
    params: TerminalCreateParams,
}

impl CreateSecondTerminalDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
        let list = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(list.body, FrameBody::TerminalList { .. }));
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalListOk {
                request_id: 1,
                terminals: vec![self.original_info.clone()],
            }),
        )
        .await?;

        let attach = recv_encrypted(&mut transport, &mut session).await?;
        let original_attach_request_id = match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
            } => {
                assert_eq!(terminal, self.original_info.terminal);
                assert_eq!(since, None);
                request_id
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        };
        send_attach_ok_with_request(
            &mut transport,
            &mut session,
            original_attach_request_id,
            self.original_stream,
            StreamSeq(0),
        )
        .await?;

        let create = recv_encrypted(&mut transport, &mut session).await?;
        let create_request_id = match create.body {
            FrameBody::TerminalCreate { request_id, params } => {
                assert_eq!(params, self.params);
                request_id
            }
            other => panic!("expected TerminalCreate, got {other:?}"),
        };
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateOk {
                request_id: create_request_id,
                terminal: self.new_terminal,
                stream: self.new_stream,
            }),
        )
        .await?;

        let attach = recv_encrypted(&mut transport, &mut session).await?;
        let new_attach_request_id = match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
            } => {
                assert_eq!(terminal, self.new_terminal);
                assert_eq!(since, None);
                request_id
            }
            other => panic!("expected TerminalAttach, got {other:?}"),
        };
        assert_eq!(new_attach_request_id, create_request_id);
        send_attach_ok_with_request(
            &mut transport,
            &mut session,
            new_attach_request_id,
            self.new_stream,
            StreamSeq(0),
        )
        .await?;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::Revoked,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct DroppedCreateReconnectDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    terminal: TerminalId,
    stream: StreamId,
    stale_params: TerminalCreateParams,
    fresh_params: TerminalCreateParams,
    first_create_seen: Rc<RefCell<bool>>,
    second_create: Rc<RefCell<Option<(u32, TerminalCreateParams)>>>,
}

impl DroppedCreateReconnectDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
        let list = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(list.body, FrameBody::TerminalList { .. }));
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalListOk {
                request_id: 1,
                terminals: Vec::new(),
            }),
        )
        .await?;

        let create = recv_encrypted(&mut transport, &mut session).await?;
        let request_id = match create.body {
            FrameBody::TerminalCreate {
                request_id: _,
                params,
            } if attempt == 1 => {
                assert_eq!(params, self.stale_params);
                *self.first_create_seen.borrow_mut() = true;
                return transport.close().await;
            }
            FrameBody::TerminalCreate { request_id, params } => {
                assert_eq!(params, self.fresh_params);
                *self.second_create.borrow_mut() = Some((request_id, params));
                request_id
            }
            other => panic!("expected TerminalCreate, got {other:?}"),
        };

        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateOk {
                request_id,
                terminal: self.terminal,
                stream: self.stream,
            }),
        )
        .await?;
        let attach = recv_encrypted(&mut transport, &mut session).await?;
        let attach_request_id = match attach.body {
            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since: None,
            } if terminal == self.terminal => request_id,
            other => panic!("expected TerminalAttach, got {other:?}"),
        };
        send_attach_ok_with_request(
            &mut transport,
            &mut session,
            attach_request_id,
            self.stream,
            StreamSeq(0),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct CreateErrDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    terminal: TerminalId,
    stream: StreamId,
    params: TerminalCreateParams,
}

impl CreateErrDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
        let list = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(list.body, FrameBody::TerminalList { .. }));
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalListOk {
                request_id: 1,
                terminals: Vec::new(),
            }),
        )
        .await?;

        let create = recv_encrypted(&mut transport, &mut session).await?;
        let request_id = match create.body {
            FrameBody::TerminalCreate { request_id, params } => {
                assert_eq!(params, self.params);
                request_id
            }
            other => panic!("expected TerminalCreate, got {other:?}"),
        };
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateErr {
                request_id,
                error: ProtocolError::InvalidParam("bad cwd".to_owned()),
            }),
        )
        .await?;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateOk {
                request_id,
                terminal: self.terminal,
                stream: self.stream,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct CreateErrThenKnownTerminalOkDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    info: TerminalInfo,
    original_stream: StreamId,
    stale_stream: StreamId,
    params: TerminalCreateParams,
    attach_after_stale_ok: Rc<RefCell<bool>>,
    stale_observed: Rc<RefCell<bool>>,
}

impl CreateErrThenKnownTerminalOkDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        move || {
            let state = state.clone();
            async move {
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        assert!(matches!(hello.body, FrameBody::Hello(_)));
        send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
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
        assert!(matches!(
            attach.body,
            FrameBody::TerminalAttach {
                terminal,
                since: None,
                ..
            } if terminal == self.info.terminal
        ));
        send_attach_ok(
            &mut transport,
            &mut session,
            self.original_stream,
            StreamSeq(0),
        )
        .await?;

        let create = recv_encrypted(&mut transport, &mut session).await?;
        let request_id = match create.body {
            FrameBody::TerminalCreate { request_id, params } => {
                assert_eq!(params, self.params);
                request_id
            }
            other => panic!("expected TerminalCreate, got {other:?}"),
        };
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateErr {
                request_id,
                error: ProtocolError::InvalidParam("bad cwd".to_owned()),
            }),
        )
        .await?;
        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::TerminalCreateOk {
                request_id,
                terminal: self.info.terminal,
                stream: self.stale_stream,
            }),
        )
        .await?;

        for _ in 0..20 {
            if let Some(frame) = try_recv_encrypted(&mut transport, &mut session)? {
                if matches!(frame.body, FrameBody::TerminalAttach { .. }) {
                    *self.attach_after_stale_ok.borrow_mut() = true;
                }
            }
            tokio::task::yield_now().await;
        }
        *self.stale_observed.borrow_mut() = true;

        send_encrypted(
            &mut transport,
            &mut session,
            Frame::body(FrameBody::Bye {
                reason: ByeReason::Revoked,
            }),
        )
        .await
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct HelloResumeStaleDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    intended_info: TerminalInfo,
    wrong_info: TerminalInfo,
    fallback_attach: ObservedAttach,
}

impl HelloResumeStaleDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        match attempt {
            1 => {
                assert_eq!(resume, None);
                send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
                let list = recv_encrypted(&mut transport, &mut session).await?;
                assert!(matches!(list.body, FrameBody::TerminalList { .. }));
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::TerminalListOk {
                        request_id: 1,
                        terminals: vec![self.intended_info],
                    }),
                )
                .await?;
                assert!(matches!(
                    recv_encrypted(&mut transport, &mut session).await?.body,
                    FrameBody::TerminalAttach { .. }
                ));
                send_attach_ok(&mut transport, &mut session, StreamId(11), StreamSeq(6)).await?;
                send_output(
                    &mut transport,
                    &mut session,
                    StreamId(11),
                    StreamSeq(7),
                    b"before-stale",
                )
                .await?;
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::Bye {
                        reason: ByeReason::ServerShutdown,
                    }),
                )
                .await
            }
            2 => {
                assert!(resume.is_some());
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::HelloErr(HelloErr {
                        error: ProtocolError::ResumeStale,
                    })),
                )
                .await
            }
            3 => {
                assert_eq!(resume, None);
                send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
                let frame = recv_encrypted(&mut transport, &mut session).await?;
                match frame.body {
                    FrameBody::TerminalAttach {
                        terminal, since, ..
                    } => {
                        *self.fallback_attach.borrow_mut() = Some((terminal, since));
                    }
                    FrameBody::TerminalList { .. } => {
                        send_encrypted(
                            &mut transport,
                            &mut session,
                            Frame::body(FrameBody::TerminalListOk {
                                request_id: 1,
                                terminals: vec![self.wrong_info, self.intended_info],
                            }),
                        )
                        .await?;
                    }
                    other => panic!("expected TerminalAttach or TerminalList, got {other:?}"),
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct ByeResumeStaleDaemon {
    keypair: KeyPair,
    session_id: SessionId,
    info: TerminalInfo,
    third_resume: Rc<RefCell<ObservedResume>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObservedResume {
    Pending,
    SeenNone,
    SeenSome,
}

impl ByeResumeStaleDaemon {
    fn transport_factory(
        &self,
    ) -> impl FnMut() -> LocalBoxFuture<'static, ClientResult<MemoryTransport>> + 'static {
        let state = self.clone();
        let attempts = Rc::new(RefCell::new(0_u8));
        move || {
            let state = state.clone();
            let attempts = Rc::clone(&attempts);
            async move {
                let attempt = {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    *attempts
                };
                let (client, server) = memory_pair();
                tokio::task::spawn_local(async move {
                    state.run_connection(server, attempt).await.unwrap();
                });
                Ok(client)
            }
            .boxed_local()
        }
    }

    async fn run_connection(self, mut transport: MemoryTransport, attempt: u8) -> ClientResult<()> {
        let mut session = self.handshake(&mut transport).await?;
        let hello = recv_encrypted(&mut transport, &mut session).await?;
        let resume = match hello.body {
            FrameBody::Hello(hello) => hello.resume,
            other => panic!("expected Hello, got {other:?}"),
        };

        match attempt {
            1 => {
                assert_eq!(resume, None);
                send_hello_ok(&mut transport, &mut session, self.session_id, false).await?;
                let list = recv_encrypted(&mut transport, &mut session).await?;
                assert!(matches!(list.body, FrameBody::TerminalList { .. }));
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::TerminalListOk {
                        request_id: 1,
                        terminals: vec![self.info],
                    }),
                )
                .await?;
                assert!(matches!(
                    recv_encrypted(&mut transport, &mut session).await?.body,
                    FrameBody::TerminalAttach { .. }
                ));
                send_attach_ok(&mut transport, &mut session, StreamId(11), StreamSeq(6)).await?;
                send_output(
                    &mut transport,
                    &mut session,
                    StreamId(11),
                    StreamSeq(7),
                    b"before-bye-stale",
                )
                .await?;
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::Bye {
                        reason: ByeReason::ServerShutdown,
                    }),
                )
                .await
            }
            2 => {
                assert!(resume.is_some());
                send_hello_ok(&mut transport, &mut session, self.session_id, true).await?;
                assert!(matches!(
                    recv_encrypted(&mut transport, &mut session).await?.body,
                    FrameBody::TerminalAttach { .. }
                ));
                send_encrypted(
                    &mut transport,
                    &mut session,
                    Frame::body(FrameBody::Bye {
                        reason: ByeReason::ProtocolError(ProtocolError::ResumeStale),
                    }),
                )
                .await
            }
            3 => {
                *self.third_resume.borrow_mut() = if resume.is_some() {
                    ObservedResume::SeenSome
                } else {
                    ObservedResume::SeenNone
                };
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handshake(&self, transport: &mut MemoryTransport) -> ClientResult<NoiseSession> {
        let mut responder = NoiseResponder::new(&self.keypair, None)?;
        let m1 = transport.recv().await?.unwrap();
        responder.read_handshake(&m1)?;
        transport.send(responder.write_handshake()?).await?;
        let m3 = transport.recv().await?.unwrap();
        responder.read_handshake(&m3)?;
        Ok(responder.finish()?)
    }
}

#[derive(Clone)]
struct MemoryTransport {
    incoming: Rc<RefCell<VecDeque<Vec<u8>>>>,
    outgoing: Rc<RefCell<VecDeque<Vec<u8>>>>,
    incoming_closed: Rc<RefCell<bool>>,
    outgoing_closed: Rc<RefCell<bool>>,
}

fn memory_pair() -> (MemoryTransport, MemoryTransport) {
    let client_to_server = Rc::new(RefCell::new(VecDeque::new()));
    let server_to_client = Rc::new(RefCell::new(VecDeque::new()));
    let client_to_server_closed = Rc::new(RefCell::new(false));
    let server_to_client_closed = Rc::new(RefCell::new(false));
    (
        MemoryTransport {
            incoming: Rc::clone(&server_to_client),
            outgoing: Rc::clone(&client_to_server),
            incoming_closed: Rc::clone(&server_to_client_closed),
            outgoing_closed: Rc::clone(&client_to_server_closed),
        },
        MemoryTransport {
            incoming: client_to_server,
            outgoing: server_to_client,
            incoming_closed: client_to_server_closed,
            outgoing_closed: server_to_client_closed,
        },
    )
}

#[async_trait(?Send)]
impl Transport for MemoryTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> ClientResult<()> {
        self.outgoing.borrow_mut().push_back(bytes);
        Ok(())
    }

    async fn recv(&mut self) -> ClientResult<Option<Vec<u8>>> {
        loop {
            if let Some(bytes) = self.incoming.borrow_mut().pop_front() {
                return Ok(Some(bytes));
            }
            if *self.incoming_closed.borrow() {
                return Ok(None);
            }
            tokio::task::yield_now().await;
        }
    }

    async fn close(&mut self) -> ClientResult<()> {
        *self.outgoing_closed.borrow_mut() = true;
        Ok(())
    }
}

async fn send_encrypted(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
    frame: Frame,
) -> ClientResult<()> {
    transport
        .send(session.encrypt(&encode_frame(&frame)?)?)
        .await
}

async fn send_hello_ok(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
    session_id: SessionId,
    resumed: bool,
) -> ClientResult<()> {
    send_encrypted(
        transport,
        session,
        Frame::body(FrameBody::HelloOk(HelloOk {
            protocol: PROTOCOL_VERSION,
            server_info: server_info(),
            session_id,
            resumed,
        })),
    )
    .await
}

async fn send_attach_ok(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
    stream: StreamId,
    head_seq: StreamSeq,
) -> ClientResult<()> {
    send_attach_ok_with_request(transport, session, 1, stream, head_seq).await
}

async fn send_attach_ok_with_request(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
    request_id: u32,
    stream: StreamId,
    head_seq: StreamSeq,
) -> ClientResult<()> {
    send_encrypted(
        transport,
        session,
        Frame::body(FrameBody::TerminalAttachOk {
            request_id,
            snapshot: snapshot(head_seq),
            head_seq,
            stream,
            initial_window: 4096,
        }),
    )
    .await
}

async fn send_output(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
    stream: StreamId,
    seq: StreamSeq,
    bytes: &[u8],
) -> ClientResult<()> {
    send_encrypted(
        transport,
        session,
        Frame::body(FrameBody::Output {
            stream,
            seq,
            bytes: bytes.to_vec().into(),
        }),
    )
    .await
}

async fn recv_encrypted(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
) -> ClientResult<Frame> {
    let ciphertext = transport.recv().await?.unwrap();
    Ok(decode_frame(&session.decrypt(&ciphertext)?)?)
}

fn try_recv_encrypted(
    transport: &mut MemoryTransport,
    session: &mut NoiseSession,
) -> ClientResult<Option<Frame>> {
    let Some(ciphertext) = transport.incoming.borrow_mut().pop_front() else {
        return Ok(None);
    };

    Ok(Some(decode_frame(&session.decrypt(&ciphertext)?)?))
}

async fn assert_connecting(events: &mut mpsc::Receiver<ClientEvent>) {
    assert!(matches!(
        events.next().await.unwrap(),
        ClientEvent::Connecting
    ));
}

async fn assert_connected(events: &mut mpsc::Receiver<ClientEvent>, expected: SessionId) {
    match events.next().await.unwrap() {
        ClientEvent::Connected { session_id } => assert_eq!(session_id, expected),
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

async fn assert_terminal_created_with_size(
    events: &mut mpsc::Receiver<ClientEvent>,
    terminal: TerminalId,
    cols: u16,
    rows: u16,
) {
    match events.next().await.unwrap() {
        ClientEvent::TerminalCreated(info) => {
            assert_eq!(info.terminal, terminal);
            assert_eq!(info.cols, cols);
            assert_eq!(info.rows, rows);
        }
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
            assert_eq!(actual_bytes, Bytes::copy_from_slice(bytes));
        }
        other => panic!("expected TerminalOutput, got {other:?}"),
    }
}

async fn assert_error_contains(events: &mut mpsc::Receiver<ClientEvent>, expected: &str) {
    for _ in 0..100 {
        if let Some(event) = events.next().now_or_never() {
            match event.unwrap() {
                ClientEvent::Error(message) => {
                    assert!(
                        message.contains(expected),
                        "expected error containing {expected:?}, got {message:?}"
                    );
                    return;
                }
                other => panic!("expected Error, got {other:?}"),
            }
        }
        tokio::task::yield_now().await;
    }

    panic!("expected Error containing {expected:?}, got no event");
}

async fn assert_disconnected_retry(events: &mut mpsc::Receiver<ClientEvent>) {
    match events.next().await.unwrap() {
        ClientEvent::Disconnected {
            will_retry: true, ..
        } => {}
        other => panic!("expected retrying Disconnected, got {other:?}"),
    }
}

fn terminal_params(cols: u16, rows: u16, label: &str) -> TerminalCreateParams {
    TerminalCreateParams {
        cols,
        rows,
        cwd: Some(format!("/tmp/{label}")),
        cmd: vec![label.to_owned()],
        env: vec![("TERM".to_owned(), "xterm-256color".to_owned())],
        scrollback_bytes: Some(8192),
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

fn server_info() -> ServerInfo {
    ServerInfo {
        server_version: "test-daemon".to_owned(),
        host_label: Some("lab".to_owned()),
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
