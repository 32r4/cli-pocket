use bytes::Bytes;
use cli_pocket_crypto::{NoiseAnonymousInitiator, NoiseInitiator, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    ByeReason, Frame, FrameBody, Hello, ProtocolError, ServerConfig, ServerId, StreamId,
    TerminalCreateParams, TerminalId, TerminalInfo, PROTOCOL_VERSION,
};
use futures_channel::{mpsc, oneshot};
use futures_util::{
    future::{Either, LocalBoxFuture},
    FutureExt, SinkExt, StreamExt,
};
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::rc::Rc;

use crate::events::ClientEvent;
use crate::identity::ClientIdentity;
use crate::relay::open_client_pair;
use crate::snapshot::TerminalSnapshot;
use crate::terminal::{TerminalCmd, TerminalHandle};
use crate::{ClientError, ClientResult, Clock, KeyValueStore, Rng, Transport};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub endpoint: SessionEndpoint,
    pub resume_token: Option<cli_pocket_proto::ResumeToken>,
    pub backoff: (u64, u64, u32),
}

#[derive(Debug, Clone)]
pub enum SessionEndpoint {
    Direct(String),
    Relay {
        url: String,
        server_id: ServerId,
        psk_hex: String,
        server_public: [u8; 32],
    },
}

const HEARTBEAT_IDLE_MS: u64 = 10_000;
const HEARTBEAT_TIMEOUT_MS: u64 = 30_000;

type SharedReply<T> = Rc<RefCell<Option<oneshot::Sender<ClientResult<T>>>>>;
type TerminalSnapshotReply = SharedReply<TerminalSnapshot>;
type TerminalListReply = SharedReply<Vec<TerminalInfo>>;
type ServerConfigReply = SharedReply<ServerConfig>;
type UnitReply = SharedReply<()>;

struct PendingServerConfigReplies {
    reads: HashMap<u32, ServerConfigReply>,
    writes: HashMap<u32, ServerConfigReply>,
}

impl PendingServerConfigReplies {
    fn new() -> Self {
        Self {
            reads: HashMap::new(),
            writes: HashMap::new(),
        }
    }
}

pub trait SessionSpawner {
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>);
}

impl<F> SessionSpawner for F
where
    F: Fn(LocalBoxFuture<'static, ()>) + 'static,
{
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>) {
        (self)(fut);
    }
}

pub struct SessionBuilder<T, C, R, K, S>
where
    T: Transport + 'static,
    C: Clock + 'static,
    R: Rng + 'static,
    K: KeyValueStore + 'static,
    S: SessionSpawner + 'static,
{
    pub identity: ClientIdentity,
    pub config: SessionConfig,
    pub clock: C,
    pub rng: R,
    pub kv: K,
    pub transport_factory: Box<dyn FnMut() -> LocalBoxFuture<'static, ClientResult<T>>>,
    pub spawner: S,
    _transport: PhantomData<T>,
}

pub struct ClientSession {
    terminal: Rc<RefCell<Option<TerminalHandle>>>,
    session_cmd_tx: mpsc::Sender<SessionCommand>,
    stop_requested: Rc<Cell<bool>>,
}

#[derive(Debug, Clone)]
enum SessionCommand {
    CreateTerminal(TerminalCreateParams),
    OpenTerminal {
        terminal: TerminalId,
        reply: TerminalSnapshotReply,
        emit_created_event: bool,
    },
    ListTerminals {
        reply: TerminalListReply,
    },
    GetServerConfig {
        reply: ServerConfigReply,
    },
    SetServerConfig {
        config: ServerConfig,
        reply: ServerConfigReply,
    },
    KillTerminal {
        terminal: TerminalId,
        reply: UnitReply,
    },
    Shutdown,
}

impl<T, C, R, K, S> SessionBuilder<T, C, R, K, S>
where
    T: Transport + 'static,
    C: Clock + 'static,
    R: Rng + 'static,
    K: KeyValueStore + 'static,
    S: SessionSpawner + 'static,
{
    pub fn new(
        identity: ClientIdentity,
        config: SessionConfig,
        clock: C,
        rng: R,
        kv: K,
        transport_factory: impl FnMut() -> LocalBoxFuture<'static, ClientResult<T>> + 'static,
        spawner: S,
    ) -> Self {
        Self {
            identity,
            config,
            clock,
            rng,
            kv,
            transport_factory: Box::new(transport_factory),
            spawner,
            _transport: PhantomData,
        }
    }

    pub fn start(self) -> (ClientSession, mpsc::Receiver<ClientEvent>) {
        let (events_tx, events_rx) = mpsc::channel::<ClientEvent>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<TerminalCmd>(64);
        let (session_cmd_tx, session_cmd_rx) = mpsc::channel::<SessionCommand>(64);
        let terminal = Rc::new(RefCell::new(None));
        let stop_requested = Rc::new(Cell::new(false));
        let session = ClientSession {
            terminal: Rc::clone(&terminal),
            session_cmd_tx,
            stop_requested: Rc::clone(&stop_requested),
        };

        self.spawner.spawn(
            run_session_loop::<T, _, _, _>(
                self.identity,
                self.config,
                self.clock,
                self.rng,
                self.kv,
                self.transport_factory,
                events_tx,
                cmd_tx,
                cmd_rx,
                session_cmd_rx,
                terminal,
                stop_requested,
            )
            .boxed_local(),
        );

        (session, events_rx)
    }
}

impl ClientSession {
    #[allow(clippy::unused_async)]
    pub async fn terminal(&self) -> Option<TerminalHandle> {
        self.terminal.borrow().clone()
    }

    pub async fn create_terminal(&self, params: TerminalCreateParams) -> ClientResult<()> {
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::CreateTerminal(params))
            .await
            .map_err(|_| ClientError::Closed)
    }

    pub async fn open_terminal(&self, terminal: TerminalId) -> ClientResult<TerminalSnapshot> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::OpenTerminal {
                terminal,
                reply: Rc::new(RefCell::new(Some(reply_tx))),
                emit_created_event: false,
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        reply_rx.await.map_err(|_| ClientError::Closed)?
    }

    pub async fn list_terminals(&self) -> ClientResult<Vec<TerminalInfo>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::ListTerminals {
                reply: Rc::new(RefCell::new(Some(reply_tx))),
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        reply_rx.await.map_err(|_| ClientError::Closed)?
    }

    pub async fn get_server_config(&self) -> ClientResult<ServerConfig> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::GetServerConfig {
                reply: Rc::new(RefCell::new(Some(reply_tx))),
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        reply_rx.await.map_err(|_| ClientError::Closed)?
    }

    pub async fn set_server_config(&self, config: ServerConfig) -> ClientResult<ServerConfig> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::SetServerConfig {
                config,
                reply: Rc::new(RefCell::new(Some(reply_tx))),
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        reply_rx.await.map_err(|_| ClientError::Closed)?
    }

    pub async fn kill_terminal(&self, terminal: TerminalId) -> ClientResult<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::KillTerminal {
                terminal,
                reply: Rc::new(RefCell::new(Some(reply_tx))),
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        reply_rx.await.map_err(|_| ClientError::Closed)?
    }

    pub async fn shutdown(&self) {
        self.stop_requested.set(true);
        let _ = self
            .session_cmd_tx
            .clone()
            .send(SessionCommand::Shutdown)
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session_loop<T, C, R, K>(
    identity: ClientIdentity,
    config: SessionConfig,
    clock: C,
    rng: R,
    _kv: K,
    mut transport_factory: Box<dyn FnMut() -> LocalBoxFuture<'static, ClientResult<T>>>,
    events_tx: mpsc::Sender<ClientEvent>,
    cmd_tx: mpsc::Sender<TerminalCmd>,
    mut cmd_rx: mpsc::Receiver<TerminalCmd>,
    mut session_cmd_rx: mpsc::Receiver<SessionCommand>,
    terminal: Rc<RefCell<Option<TerminalHandle>>>,
    stop_requested: Rc<Cell<bool>>,
) where
    T: Transport + 'static,
    C: Clock + 'static,
    R: Rng + 'static,
    K: KeyValueStore + 'static,
{
    let (start, max, mul) = config.backoff;
    let mut delay = start;
    let mut pending_cmds = VecDeque::<TerminalCmd>::new();
    let mut pending_session_cmds = VecDeque::<SessionCommand>::new();

    loop {
        if stop_requested.get() {
            return;
        }

        let _ = events_tx.clone().send(ClientEvent::Connecting).await;
        let mut reached_connected = false;
        let mut transport = match transport_factory().await {
            Ok(transport) => transport,
            Err(err) => {
                if stop_requested.get() {
                    return;
                }
                let _ = events_tx
                    .clone()
                    .send(ClientEvent::Disconnected {
                        will_retry: true,
                        reason: err.to_string(),
                    })
                    .await;
                sleep_backoff(&clock, &rng, delay).await;
                delay = crate::reconnect::next_delay(delay, max, mul);
                continue;
            }
        };

        let outcome = run_one_connection(
            &identity,
            &config,
            &clock,
            &rng,
            &mut transport,
            &mut cmd_rx,
            &mut session_cmd_rx,
            &mut pending_cmds,
            &mut pending_session_cmds,
            ConnectionState {
                events_tx: events_tx.clone(),
                cmd_tx: &cmd_tx,
                terminal: &terminal,
                reached_connected: &mut reached_connected,
            },
        )
        .await;

        if stop_requested.get() {
            *terminal.borrow_mut() = None;
            return;
        }

        *terminal.borrow_mut() = None;
        if reached_connected {
            delay = start;
        }

        match outcome {
            Ok(()) => {
                let _ = events_tx
                    .clone()
                    .send(ClientEvent::Disconnected {
                        will_retry: true,
                        reason: "remote closed".to_owned(),
                    })
                    .await;
            }
            Err(err) => {
                let will_retry = !matches!(err, ClientError::Rejected(_));
                let _ = events_tx
                    .clone()
                    .send(ClientEvent::Disconnected {
                        will_retry,
                        reason: err.to_string(),
                    })
                    .await;
                if !will_retry {
                    return;
                }
            }
        }

        if stop_requested.get() {
            return;
        }
        sleep_backoff(&clock, &rng, delay).await;
        delay = crate::reconnect::next_delay(delay, max, mul);
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_one_connection<T, C, R>(
    identity: &ClientIdentity,
    config: &SessionConfig,
    clock: &C,
    rng: &R,
    transport: &mut T,
    cmd_rx: &mut mpsc::Receiver<TerminalCmd>,
    session_cmd_rx: &mut mpsc::Receiver<SessionCommand>,
    pending_cmds: &mut VecDeque<TerminalCmd>,
    pending_session_cmds: &mut VecDeque<SessionCommand>,
    state: ConnectionState<'_>,
) -> ClientResult<()>
where
    T: Transport,
    C: Clock,
    R: Rng,
{
    let _ = &config.resume_token;

    let mut session = match &config.endpoint {
        SessionEndpoint::Direct(_) => {
            let mut noise = NoiseAnonymousInitiator::new(&identity.keypair)?;
            let m1 = noise.write_handshake()?;
            transport.send(m1).await?;

            let m2 = recv_transport(transport).await?;
            noise.read_handshake(&m2)?;

            let m3 = noise.write_handshake()?;
            transport.send(m3).await?;
            noise.finish()?
        }
        SessionEndpoint::Relay {
            server_id,
            psk_hex,
            server_public,
            ..
        } => {
            open_client_pair(transport, *server_id).await?;
            let psk = parse_psk_hex(psk_hex)?;
            let mut noise = NoiseInitiator::new(&identity.keypair, server_public, Some(&psk))?;
            let m1 = noise.write_handshake()?;
            transport.send(m1).await?;

            let m2 = recv_transport(transport).await?;
            noise.read_handshake(&m2)?;

            let m3 = noise.write_handshake()?;
            transport.send(m3).await?;
            noise.finish()?
        }
    };
    let mut next_request_id = 1_u32;

    let hello = Frame::body(FrameBody::Hello(Hello {
        protocol_min: PROTOCOL_VERSION,
        protocol_max: PROTOCOL_VERSION,
        resume: None,
    }));
    send_encrypted(transport, &mut session, &hello).await?;

    let hello_reply = recv_encrypted(transport, &mut session).await?;
    let (session_id, server_label) = match hello_reply.body {
        FrameBody::HelloOk(ok) => (ok.session_id, ok.server_info.server_label),
        other => {
            return Err(ClientError::Proto(format!(
                "unexpected hello reply: {other:?}"
            )));
        }
    };

    let _ = state
        .events_tx
        .clone()
        .send(ClientEvent::Connected {
            session_id,
            server_label,
        })
        .await;
    *state.reached_connected = true;
    let mut heartbeat = HeartbeatState::new(clock.now_ms());
    let mut runtime = RuntimeState::default();
    let mut pending_lists = HashMap::<u32, TerminalListReply>::new();
    let mut pending_server_config = PendingServerConfigReplies::new();
    let mut pending_kills = HashMap::<u32, UnitReply>::new();

    loop {
        match heartbeat.poll(clock.now_ms(), rng) {
            HeartbeatPoll::SendPing(nonce) => {
                send_encrypted(
                    transport,
                    &mut session,
                    &Frame::body(FrameBody::Ping { nonce }),
                )
                .await?;
                continue;
            }
            HeartbeatPoll::Timeout => {
                let _ = transport.close().await;
                return Err(ClientError::Transport("heartbeat timeout".to_owned()));
            }
            HeartbeatPoll::Wait => {}
        }

        let action = {
            let next = if let Some(cmd) = pending_session_cmds.pop_front() {
                let frame = frame_for_session_command(
                    &mut runtime,
                    state.terminal,
                    cmd.clone(),
                    &mut next_request_id,
                    &mut pending_lists,
                    &mut pending_server_config,
                    &mut pending_kills,
                );
                pending_session_cmds.push_front(cmd);
                Either::Left((Some(frame), ()))
            } else if let Some(cmd) = pending_cmds.pop_front() {
                let frame = frame_for_command(state.terminal.borrow().as_ref(), cmd.clone());
                pending_cmds.push_front(cmd);
                Either::Left((frame, ()))
            } else {
                let next_cmd = cmd_rx.next().fuse();
                let next_session_cmd = session_cmd_rx.next().fuse();
                let next_frame =
                    recv_with_heartbeat(transport, &mut session, clock, rng, &mut heartbeat).fuse();
                futures_util::pin_mut!(next_cmd, next_session_cmd, next_frame);

                match futures_util::future::select(
                    next_session_cmd,
                    futures_util::future::select(next_cmd, next_frame),
                )
                .await
                {
                    Either::Left((cmd, _pending_other)) => {
                        let Some(cmd) = cmd else {
                            return Ok(());
                        };
                        if matches!(cmd, SessionCommand::Shutdown) {
                            return Ok(());
                        }
                        let frame = frame_for_session_command(
                            &mut runtime,
                            state.terminal,
                            cmd.clone(),
                            &mut next_request_id,
                            &mut pending_lists,
                            &mut pending_server_config,
                            &mut pending_kills,
                        );
                        pending_session_cmds.push_back(cmd);
                        Either::Left((Some(frame), ()))
                    }
                    Either::Right((Either::Left((cmd, _pending_frame)), _pending_session_cmd)) => {
                        let Some(cmd) = cmd else {
                            return Ok(());
                        };
                        let frame =
                            frame_for_command(state.terminal.borrow().as_ref(), cmd.clone());
                        pending_cmds.push_back(cmd);
                        Either::Left((frame, ()))
                    }
                    Either::Right((Either::Right((frame, _pending_cmd)), _pending_session_cmd)) => {
                        Either::Right((frame, ()))
                    }
                }
            };

            match next {
                Either::Left((frame, ())) => {
                    if pending_session_cmds.front().is_some() {
                        frame.map_or(Action::None, Action::SendPendingSessionCommand)
                    } else {
                        frame.map_or(Action::None, Action::SendPendingCommand)
                    }
                }
                Either::Right((frame, ())) => {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(err) => return Err(err),
                    };
                    match frame.body {
                        FrameBody::Output { stream, seq, bytes } => {
                            if runtime.active_stream() != Some(stream) {
                                Action::None
                            } else if let Some(handle) = state.terminal.borrow_mut().as_mut() {
                                handle.last_seq = Some(seq);
                                runtime.update_active_seq(seq);
                                Action::Emit(ClientEvent::TerminalOutput {
                                    terminal_id: handle.terminal_id(),
                                    stream_seq: seq,
                                    bytes: Bytes::from(bytes.into_vec()),
                                })
                            } else {
                                Action::None
                            }
                        }
                        FrameBody::TerminalCreateOk {
                            request_id,
                            terminal: created_terminal,
                            stream,
                        } => {
                            if let Some((info, stream)) =
                                runtime.complete_create(request_id, created_terminal, stream)
                            {
                                runtime.store_info(info);
                                let detach_request_id = next_request_id;
                                next_request_id = next_request_id.saturating_add(1);
                                Action::Send(Frame::body(FrameBody::TerminalDetach {
                                    request_id: detach_request_id,
                                    stream,
                                }))
                            } else {
                                Action::Emit(ClientEvent::Error(format!(
                                    "terminal create ok missing metadata for {created_terminal:?}"
                                )))
                            }
                        }
                        FrameBody::TerminalCreateErr { request_id, error } => {
                            if runtime.remove_pending_create(request_id).is_some() {
                                Action::Emit(ClientEvent::Error(format!(
                                    "terminal create failed: {error}"
                                )))
                            } else {
                                Action::Emit(ClientEvent::Error(format!(
                                    "terminal create failed for unknown request {request_id}: {error}"
                                )))
                            }
                        }
                        FrameBody::TerminalAttachOk {
                            request_id,
                            snapshot,
                            stream,
                            head_seq,
                            ..
                        } => {
                            let mut event = None;
                            if let Some(pending) = runtime.take_pending_open(request_id) {
                                let info = runtime.info(pending.terminal).cloned().unwrap_or(
                                    TerminalInfo {
                                        terminal: pending.terminal,
                                        cols: snapshot.cols,
                                        rows: snapshot.rows,
                                        created_at_unix_ms: 0,
                                        label: snapshot.anchor_state.title.clone(),
                                        attached_clients: 1,
                                    },
                                );
                                runtime.store_info(info.clone());
                                runtime.set_active(&info, stream, Some(head_seq));
                                *state.terminal.borrow_mut() = Some(TerminalHandle::new(
                                    info.clone(),
                                    stream,
                                    Some(head_seq),
                                    state.cmd_tx.clone(),
                                ));

                                if pending.emit_created_event {
                                    event = Some(ClientEvent::TerminalCreated(info.clone()));
                                }

                                if let Some(reply) = pending.notify {
                                    if let Some(reply) = reply.borrow_mut().take() {
                                        let _ = reply
                                            .send(Ok(TerminalSnapshot::from_parts(info, snapshot)));
                                    }
                                }
                            }
                            event.map_or(Action::None, Action::Emit)
                        }
                        FrameBody::TerminalAttachErr { request_id, error } => {
                            if let Some(pending) = runtime.take_pending_open(request_id) {
                                if let Some(reply) = pending.notify {
                                    if let Some(reply) = reply.borrow_mut().take() {
                                        let _ = reply.send(Err(ClientError::Proto(format!(
                                            "terminal open failed: {error}"
                                        ))));
                                    }
                                }
                            }
                            Action::None
                        }
                        FrameBody::TerminalDetachOk { request_id }
                        | FrameBody::TerminalDetachErr { request_id, .. } => {
                            if let Some(pending) = runtime.take_pending_detach(request_id) {
                                clear_active_terminal(state.terminal, &mut runtime);
                                if let Some(open_request_id) = pending.next_open_request_id {
                                    if let Some(open) = runtime.pending_open(open_request_id) {
                                        Action::Send(Frame::body(FrameBody::TerminalAttach {
                                            request_id: open.request_id,
                                            terminal: open.terminal,
                                            since: None,
                                        }))
                                    } else {
                                        Action::None
                                    }
                                } else {
                                    Action::None
                                }
                            } else {
                                Action::None
                            }
                        }
                        FrameBody::TerminalListOk {
                            request_id,
                            terminals,
                        } => {
                            for terminal in &terminals {
                                runtime.store_info(terminal.clone());
                            }
                            if let Some(reply) = pending_lists.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Ok(terminals));
                                }
                            }
                            Action::None
                        }
                        FrameBody::ServerConfigGetOk { request_id, config } => {
                            if let Some(reply) = pending_server_config.reads.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Ok(config));
                                }
                            }
                            Action::None
                        }
                        FrameBody::ServerConfigGetErr { request_id, error } => {
                            if let Some(reply) = pending_server_config.reads.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Err(ClientError::Proto(format!(
                                        "server config get failed: {error}"
                                    ))));
                                }
                            }
                            Action::None
                        }
                        FrameBody::ServerConfigSetOk { request_id, config } => {
                            if let Some(reply) = pending_server_config.writes.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Ok(config));
                                }
                            }
                            Action::None
                        }
                        FrameBody::ServerConfigSetErr { request_id, error } => {
                            if let Some(reply) = pending_server_config.writes.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Err(ClientError::Proto(format!(
                                        "server config set failed: {error}"
                                    ))));
                                }
                            }
                            Action::None
                        }
                        FrameBody::TerminalKillOk { request_id } => {
                            if let Some(reply) = pending_kills.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Ok(()));
                                }
                            }
                            Action::None
                        }
                        FrameBody::TerminalKillErr { request_id, error } => {
                            if let Some(reply) = pending_kills.remove(&request_id) {
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Err(ClientError::Proto(format!(
                                        "terminal kill failed: {error}"
                                    ))));
                                }
                            }
                            Action::None
                        }
                        FrameBody::TerminalExit { terminal, exit } => {
                            if runtime.active_terminal() == Some(terminal) {
                                clear_active_terminal(state.terminal, &mut runtime);
                            }
                            Action::Emit(ClientEvent::TerminalExited {
                                terminal_id: terminal,
                                info: exit,
                            })
                        }
                        FrameBody::Bye { reason } => {
                            if is_recoverable_bye(&reason) {
                                Action::Return(Err(ClientError::Closed))
                            } else {
                                Action::Return(Err(ClientError::Rejected(reason)))
                            }
                        }
                        FrameBody::Ping { nonce } => {
                            Action::Send(Frame::body(FrameBody::Pong { nonce }))
                        }
                        _ => Action::None,
                    }
                }
            }
        };

        match action {
            Action::Send(frame) => {
                send_encrypted(transport, &mut session, &frame).await?;
            }
            Action::SendPendingCommand(frame) => {
                send_encrypted(transport, &mut session, &frame).await?;
                pending_cmds.pop_front();
            }
            Action::SendPendingSessionCommand(frame) => {
                send_encrypted(transport, &mut session, &frame).await?;
                pending_session_cmds.pop_front();
            }
            Action::Emit(event) => {
                let _ = state.events_tx.clone().send(event).await;
            }
            Action::Return(result) => return result,
            Action::None => {}
        }
    }
}

async fn recv_with_heartbeat<T, C, R>(
    transport: &mut T,
    session: &mut NoiseSession,
    clock: &C,
    rng: &R,
    heartbeat: &mut HeartbeatState,
) -> ClientResult<Frame>
where
    T: Transport,
    C: Clock,
    R: Rng,
{
    loop {
        match heartbeat.poll(clock.now_ms(), rng) {
            HeartbeatPoll::SendPing(nonce) => {
                send_encrypted(transport, session, &Frame::body(FrameBody::Ping { nonce })).await?;
            }
            HeartbeatPoll::Timeout => {
                let _ = transport.close().await;
                return Err(ClientError::Transport("heartbeat timeout".to_owned()));
            }
            HeartbeatPoll::Wait => {
                let recv = recv_encrypted(transport, session).fuse();
                let sleep = clock.sleep_ms(heartbeat.sleep_ms(clock.now_ms())).fuse();
                futures_util::pin_mut!(recv, sleep);
                match futures_util::future::select(recv, sleep).await {
                    Either::Left((frame, _)) => {
                        heartbeat.on_inbound(clock.now_ms());
                        return frame;
                    }
                    Either::Right(((), _)) => {}
                }
            }
        }
    }
}

async fn send_encrypted<T: Transport>(
    transport: &mut T,
    session: &mut NoiseSession,
    frame: &Frame,
) -> ClientResult<()> {
    let plaintext = encode_frame(frame)?;
    let ciphertext = session.encrypt(&plaintext)?;
    transport.send(ciphertext).await?;
    Ok(())
}

async fn recv_encrypted<T: Transport>(
    transport: &mut T,
    session: &mut NoiseSession,
) -> ClientResult<Frame> {
    let ciphertext = recv_transport(transport).await?;
    let plaintext = session.decrypt(&ciphertext)?;
    Ok(decode_frame(&plaintext)?)
}

async fn recv_transport<T: Transport>(transport: &mut T) -> ClientResult<Vec<u8>> {
    transport.recv().await?.ok_or(ClientError::Closed)
}

fn frame_for_command(active: Option<&TerminalHandle>, cmd: TerminalCmd) -> Option<Frame> {
    match cmd {
        TerminalCmd::Input { terminal, bytes } => active
            .filter(|handle| handle.terminal_id() == terminal)
            .map(|handle| {
                Frame::body(FrameBody::Input {
                    stream: handle.stream_id(),
                    bytes: bytes.to_vec().into(),
                })
            }),
        TerminalCmd::Resize {
            terminal,
            cols,
            rows,
        } => active
            .filter(|handle| handle.terminal_id() == terminal)
            .map(|handle| {
                Frame::body(FrameBody::Resize {
                    stream: handle.stream_id(),
                    cols,
                    rows,
                })
            }),
        TerminalCmd::Kill { .. } => None,
    }
}

fn frame_for_session_command(
    runtime: &mut RuntimeState,
    active_terminal: &Rc<RefCell<Option<TerminalHandle>>>,
    cmd: SessionCommand,
    next_request_id: &mut u32,
    pending_lists: &mut HashMap<u32, TerminalListReply>,
    pending_server_config: &mut PendingServerConfigReplies,
    pending_kills: &mut HashMap<u32, UnitReply>,
) -> Frame {
    match cmd {
        SessionCommand::CreateTerminal(params) => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_create(request_id, params.clone());
            Frame::body(FrameBody::TerminalCreate { request_id, params })
        }
        SessionCommand::OpenTerminal {
            terminal: target,
            reply,
            emit_created_event,
        } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.start_open(request_id, target, reply, emit_created_event);

            if let Some(active) = active_terminal.borrow().as_ref() {
                let detach_request_id = *next_request_id;
                *next_request_id = next_request_id.saturating_add(1);
                runtime.record_detach(detach_request_id, Some(request_id));
                Frame::body(FrameBody::TerminalDetach {
                    request_id: detach_request_id,
                    stream: active.stream_id(),
                })
            } else {
                Frame::body(FrameBody::TerminalAttach {
                    request_id,
                    terminal: target,
                    since: None,
                })
            }
        }
        SessionCommand::ListTerminals { reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            pending_lists.insert(request_id, reply);
            Frame::body(FrameBody::TerminalList { request_id })
        }
        SessionCommand::GetServerConfig { reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            pending_server_config.reads.insert(request_id, reply);
            Frame::body(FrameBody::ServerConfigGet { request_id })
        }
        SessionCommand::SetServerConfig { config, reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            pending_server_config.writes.insert(request_id, reply);
            Frame::body(FrameBody::ServerConfigSet { request_id, config })
        }
        SessionCommand::KillTerminal { terminal, reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            pending_kills.insert(request_id, reply);
            if runtime.active_terminal() == Some(terminal) {
                clear_active_terminal(active_terminal, runtime);
            }
            Frame::body(FrameBody::TerminalKill {
                request_id,
                terminal,
            })
        }
        SessionCommand::Shutdown => unreachable!("shutdown commands are handled before framing"),
    }
}

struct ConnectionState<'a> {
    events_tx: mpsc::Sender<ClientEvent>,
    cmd_tx: &'a mpsc::Sender<TerminalCmd>,
    terminal: &'a Rc<RefCell<Option<TerminalHandle>>>,
    reached_connected: &'a mut bool,
}

enum Action {
    Send(Frame),
    SendPendingCommand(Frame),
    SendPendingSessionCommand(Frame),
    Emit(ClientEvent),
    Return(ClientResult<()>),
    None,
}

async fn sleep_backoff<C: Clock, R: Rng>(clock: &C, rng: &R, delay_ms: u64) {
    let mut byte = [0_u8; 1];
    rng.fill(&mut byte);
    clock
        .sleep_ms(crate::reconnect::jitter(delay_ms, byte[0]))
        .await;
}

#[derive(Debug, Clone)]
struct HeartbeatState {
    last_inbound_at_ms: u64,
    ping_outstanding: bool,
}

enum HeartbeatPoll {
    SendPing(u32),
    Timeout,
    Wait,
}

impl HeartbeatState {
    fn new(now_ms: u64) -> Self {
        Self {
            last_inbound_at_ms: now_ms,
            ping_outstanding: false,
        }
    }

    fn on_inbound(&mut self, now_ms: u64) {
        self.last_inbound_at_ms = now_ms;
        self.ping_outstanding = false;
    }

    fn poll<R: Rng>(&mut self, now_ms: u64, rng: &R) -> HeartbeatPoll {
        let elapsed_ms = now_ms.saturating_sub(self.last_inbound_at_ms);
        if self.ping_outstanding {
            if elapsed_ms >= HEARTBEAT_TIMEOUT_MS {
                HeartbeatPoll::Timeout
            } else {
                HeartbeatPoll::Wait
            }
        } else if elapsed_ms >= HEARTBEAT_IDLE_MS {
            let mut bytes = [0_u8; 4];
            rng.fill(&mut bytes);
            self.ping_outstanding = true;
            HeartbeatPoll::SendPing(u32::from_le_bytes(bytes))
        } else {
            HeartbeatPoll::Wait
        }
    }

    fn sleep_ms(&self, now_ms: u64) -> u64 {
        let elapsed_ms = now_ms.saturating_sub(self.last_inbound_at_ms);
        if self.ping_outstanding {
            HEARTBEAT_TIMEOUT_MS.saturating_sub(elapsed_ms)
        } else {
            HEARTBEAT_IDLE_MS.saturating_sub(elapsed_ms)
        }
        .max(1)
    }
}

#[derive(Debug, Clone, Default)]
struct RuntimeState {
    active: Option<ActiveTerminalState>,
    info_cache: HashMap<TerminalId, TerminalInfo>,
    pending_opens: Vec<PendingOpen>,
    pending_detaches: HashMap<u32, PendingDetach>,
    pending_creates: Vec<PendingCreate>,
}

#[derive(Debug, Clone)]
struct ActiveTerminalState {
    terminal: TerminalId,
    stream: StreamId,
    last_seq: Option<cli_pocket_proto::StreamSeq>,
}

#[derive(Debug, Clone)]
struct PendingOpen {
    request_id: u32,
    terminal: TerminalId,
    notify: Option<TerminalSnapshotReply>,
    emit_created_event: bool,
}

#[derive(Debug, Clone)]
struct PendingDetach {
    next_open_request_id: Option<u32>,
}

#[derive(Debug, Clone)]
struct PendingCreate {
    request_id: u32,
    params: TerminalCreateParams,
}

impl RuntimeState {
    fn active_stream(&self) -> Option<StreamId> {
        self.active.as_ref().map(|active| active.stream)
    }

    fn active_terminal(&self) -> Option<TerminalId> {
        self.active.as_ref().map(|active| active.terminal)
    }

    fn update_active_seq(&mut self, seq: cli_pocket_proto::StreamSeq) {
        if let Some(active) = self.active.as_mut() {
            active.last_seq = Some(seq);
        }
    }

    fn set_active(
        &mut self,
        info: &TerminalInfo,
        stream: StreamId,
        last_seq: Option<cli_pocket_proto::StreamSeq>,
    ) {
        self.info_cache.insert(info.terminal, info.clone());
        self.active = Some(ActiveTerminalState {
            terminal: info.terminal,
            stream,
            last_seq,
        });
    }

    fn clear_active(&mut self) {
        self.active = None;
    }

    fn store_info(&mut self, info: TerminalInfo) {
        self.info_cache.insert(info.terminal, info);
    }

    fn info(&self, terminal: TerminalId) -> Option<&TerminalInfo> {
        self.info_cache.get(&terminal)
    }

    fn record_create(&mut self, request_id: u32, params: TerminalCreateParams) {
        self.pending_creates
            .push(PendingCreate { request_id, params });
    }

    fn remove_pending_create(&mut self, request_id: u32) -> Option<PendingCreate> {
        let idx = self
            .pending_creates
            .iter()
            .position(|pending| pending.request_id == request_id)?;
        Some(self.pending_creates.remove(idx))
    }

    fn complete_create(
        &mut self,
        request_id: u32,
        terminal: TerminalId,
        stream: StreamId,
    ) -> Option<(TerminalInfo, StreamId)> {
        let pending = self.remove_pending_create(request_id)?;
        let info = TerminalInfo {
            terminal,
            cols: pending.params.cols,
            rows: pending.params.rows,
            created_at_unix_ms: 0,
            label: pending.params.cmd.first().cloned(),
            attached_clients: 1,
        };
        Some((info, stream))
    }

    fn start_open(
        &mut self,
        request_id: u32,
        terminal: TerminalId,
        notify: TerminalSnapshotReply,
        emit_created_event: bool,
    ) {
        self.pending_opens.push(PendingOpen {
            request_id,
            terminal,
            notify: Some(notify),
            emit_created_event,
        });
    }

    fn take_pending_open(&mut self, request_id: u32) -> Option<PendingOpen> {
        let idx = self
            .pending_opens
            .iter()
            .position(|pending| pending.request_id == request_id)?;
        Some(self.pending_opens.remove(idx))
    }

    fn pending_open(&self, request_id: u32) -> Option<&PendingOpen> {
        self.pending_opens
            .iter()
            .find(|pending| pending.request_id == request_id)
    }

    fn record_detach(&mut self, request_id: u32, next_open_request_id: Option<u32>) {
        self.pending_detaches.insert(
            request_id,
            PendingDetach {
                next_open_request_id,
            },
        );
    }

    fn take_pending_detach(&mut self, request_id: u32) -> Option<PendingDetach> {
        self.pending_detaches.remove(&request_id)
    }
}

fn clear_active_terminal(
    terminal: &Rc<RefCell<Option<TerminalHandle>>>,
    runtime: &mut RuntimeState,
) {
    runtime.clear_active();
    *terminal.borrow_mut() = None;
}

fn is_recoverable_bye(reason: &ByeReason) -> bool {
    matches!(
        reason,
        ByeReason::ServerShutdown | ByeReason::ProtocolError(ProtocolError::BackpressureExceeded)
    )
}

fn parse_psk_hex(psk_hex: &str) -> ClientResult<[u8; 32]> {
    let bytes = hex::decode(psk_hex)
        .map_err(|error| ClientError::Transport(format!("relay psk_hex: {error}")))?;
    bytes
        .try_into()
        .map_err(|_| ClientError::Transport("relay psk_hex must be 32 bytes".to_owned()))
}
