use bytes::Bytes;
use cli_pocket_crypto::{NoiseAnonymousInitiator, NoiseInitiator, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    ByeReason, Frame, FrameBody, Hello, ProtocolError, ResumeToken, ServerConfig, ServerId,
    StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo, PROTOCOL_VERSION,
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
use crate::history::TerminalHistoryPage;
use crate::identity::ClientIdentity;
use crate::relay::open_client_pair;
use crate::snapshot::{render_prefix_from_anchor, TerminalSnapshot};
use crate::terminal::{TerminalCmd, TerminalHandle};
use crate::{ClientError, ClientResult, Clock, KeyValueStore, Rng, Transport};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub endpoint: SessionEndpoint,
    pub resume_token: Option<ResumeToken>,
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
type HistoryReply = SharedReply<TerminalHistoryPage>;
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
    },
    ListTerminals {
        reply: TerminalListReply,
    },
    ReadHistory {
        terminal: TerminalId,
        before: Option<StreamSeq>,
        max_bytes: u32,
        reply: HistoryReply,
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

    pub async fn read_history(
        &self,
        terminal: TerminalId,
        before: Option<StreamSeq>,
        max_bytes: u32,
    ) -> ClientResult<TerminalHistoryPage> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::ReadHistory {
                terminal,
                before,
                max_bytes,
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

#[allow(clippy::too_many_arguments)]
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
        resume: config.resume_token.clone(),
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
    let mut pending_history = HashMap::<u32, HistoryReply>::new();
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
                    &mut pending_history,
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
                    Either::Left((cmd, _)) => {
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
                            &mut pending_history,
                            &mut pending_server_config,
                            &mut pending_kills,
                        );
                        pending_session_cmds.push_back(cmd);
                        Either::Left((Some(frame), ()))
                    }
                    Either::Right((Either::Left((cmd, _)), _)) => {
                        let Some(cmd) = cmd else {
                            return Ok(());
                        };
                        let frame =
                            frame_for_command(state.terminal.borrow().as_ref(), cmd.clone());
                        pending_cmds.push_back(cmd);
                        Either::Left((frame, ()))
                    }
                    Either::Right((Either::Right((frame, _)), _)) => Either::Right((frame, ())),
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
                    handle_inbound_frame(
                        frame,
                        &mut runtime,
                        &state,
                        &mut pending_lists,
                        &mut pending_history,
                        &mut pending_server_config,
                        &mut pending_kills,
                    )?
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

#[allow(clippy::too_many_arguments)]
fn handle_inbound_frame(
    frame: Frame,
    runtime: &mut RuntimeState,
    state: &ConnectionState<'_>,
    pending_lists: &mut HashMap<u32, TerminalListReply>,
    pending_history: &mut HashMap<u32, HistoryReply>,
    pending_server_config: &mut PendingServerConfigReplies,
    pending_kills: &mut HashMap<u32, UnitReply>,
) -> ClientResult<Action> {
    let action = match frame.body {
        FrameBody::Output { stream, seq, bytes } => {
            if runtime.snapshot_in_progress_for(stream) {
                return Err(ClientError::Proto(
                    "received live output before snapshot finished".to_owned(),
                ));
            }

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
        FrameBody::TerminalSnapshotChunk {
            stream,
            seq,
            offset,
            bytes,
            last,
        } => {
            runtime
                .push_snapshot_chunk(stream, seq, offset, bytes.as_ref(), last)
                .map_err(ClientError::Proto)?;
            Action::None
        }
        FrameBody::TerminalCreateOk { request_id, info } => {
            if runtime.remove_pending_create(request_id) {
                runtime.store_info(info.clone());
                Action::Emit(ClientEvent::TerminalCreated(info))
            } else {
                Action::Emit(ClientEvent::Error(format!(
                    "terminal create ok for unknown request {request_id}"
                )))
            }
        }
        FrameBody::TerminalCreateErr { request_id, error } => {
            if runtime.remove_pending_create(request_id) {
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
            baseline,
            stream,
            ..
        } => {
            let Some(pending) = runtime.take_pending_open(request_id) else {
                return Ok(Action::None);
            };

            let info = runtime
                .info(pending.terminal)
                .cloned()
                .unwrap_or_else(|| TerminalInfo {
                    terminal: pending.terminal,
                    cols: baseline.cols,
                    rows: baseline.rows,
                    created_at_unix_ms: 0,
                    label: baseline.anchor_state.title.clone(),
                    attached_clients: 1,
                });
            runtime.store_info(info.clone());
            runtime.set_active(&info, stream, Some(baseline.head_seq));
            *state.terminal.borrow_mut() = Some(TerminalHandle::new(
                info.clone(),
                stream,
                Some(baseline.head_seq),
                state.cmd_tx.clone(),
            ));

            if baseline.byte_len == 0 {
                if let Some(reply) = pending.reply {
                    if let Some(reply) = reply.borrow_mut().take() {
                        let _ = reply.send(Ok(TerminalSnapshot::new(
                            info,
                            baseline.head_seq,
                            baseline.head_seq,
                            Bytes::new(),
                            render_prefix_from_anchor(&baseline.anchor_state),
                        )));
                    }
                }
                Action::None
            } else {
                runtime.begin_snapshot(
                    info,
                    stream,
                    baseline.head_seq,
                    baseline.byte_len,
                    render_prefix_from_anchor(&baseline.anchor_state),
                    pending.reply,
                );
                Action::None
            }
        }
        FrameBody::TerminalAttachErr { request_id, error } => {
            if let Some(pending) = runtime.take_pending_open(request_id) {
                if let Some(reply) = pending.reply {
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
                clear_active_terminal(state.terminal, runtime);
                if let Some(open_request_id) = pending.next_open_request_id {
                    if let Some(open) = runtime.pending_open(open_request_id) {
                        Action::Send(Frame::body(FrameBody::TerminalAttach {
                            request_id: open.request_id,
                            terminal: open.terminal,
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
        FrameBody::HistoryChunk {
            request_id,
            terminal,
            start_seq,
            end_seq,
            bytes,
            last,
        } => {
            runtime
                .push_history_chunk(
                    request_id,
                    terminal,
                    start_seq,
                    end_seq,
                    bytes.as_ref(),
                    last,
                )
                .map_err(ClientError::Proto)?;
            if last {
                let completed = runtime
                    .take_completed_history(request_id)
                    .ok_or_else(|| ClientError::Proto("history assembly missing".to_owned()))?;
                if let Some(reply) = pending_history.remove(&request_id) {
                    if let Some(reply) = reply.borrow_mut().take() {
                        let _ = reply.send(Ok(TerminalHistoryPage::new(
                            completed.terminal,
                            completed.start_seq,
                            completed.end_seq,
                            Bytes::from(completed.bytes),
                        )));
                    }
                }
                Action::None
            } else {
                Action::None
            }
        }
        FrameBody::HistoryErr { request_id, error } => {
            runtime.drop_history(request_id);
            if let Some(reply) = pending_history.remove(&request_id) {
                if let Some(reply) = reply.borrow_mut().take() {
                    let _ = reply.send(Err(ClientError::Proto(format!(
                        "history read failed: {error}"
                    ))));
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
                clear_active_terminal(state.terminal, runtime);
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
        FrameBody::Ping { nonce } => Action::Send(Frame::body(FrameBody::Pong { nonce })),
        _ => Action::None,
    };

    Ok(action)
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

#[allow(clippy::too_many_arguments)]
fn frame_for_session_command(
    runtime: &mut RuntimeState,
    active_terminal: &Rc<RefCell<Option<TerminalHandle>>>,
    cmd: SessionCommand,
    next_request_id: &mut u32,
    pending_lists: &mut HashMap<u32, TerminalListReply>,
    pending_history: &mut HashMap<u32, HistoryReply>,
    pending_server_config: &mut PendingServerConfigReplies,
    pending_kills: &mut HashMap<u32, UnitReply>,
) -> Frame {
    match cmd {
        SessionCommand::CreateTerminal(params) => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_create(request_id);
            Frame::body(FrameBody::TerminalCreate { request_id, params })
        }
        SessionCommand::OpenTerminal {
            terminal: target,
            reply,
        } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.start_open(request_id, target, reply);

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
                })
            }
        }
        SessionCommand::ListTerminals { reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            pending_lists.insert(request_id, reply);
            Frame::body(FrameBody::TerminalList { request_id })
        }
        SessionCommand::ReadHistory {
            terminal,
            before,
            max_bytes,
            reply,
        } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.begin_history(request_id, terminal);
            pending_history.insert(request_id, reply);
            Frame::body(FrameBody::HistoryRequest {
                request_id,
                terminal,
                before,
                max_bytes,
            })
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
    pending_creates: Vec<u32>,
    opening_snapshot: Option<SnapshotAssembly>,
    pending_history: HashMap<u32, HistoryAssembly>,
    completed_history: HashMap<u32, CompletedHistory>,
}

#[derive(Debug, Clone)]
struct ActiveTerminalState {
    terminal: TerminalId,
    stream: StreamId,
    last_seq: Option<StreamSeq>,
}

#[derive(Debug, Clone)]
struct PendingOpen {
    request_id: u32,
    terminal: TerminalId,
    reply: Option<TerminalSnapshotReply>,
}

#[derive(Debug, Clone)]
struct PendingDetach {
    next_open_request_id: Option<u32>,
}

#[derive(Debug, Clone)]
struct SnapshotAssembly {
    info: TerminalInfo,
    stream: StreamId,
    start_seq: StreamSeq,
    head_seq: StreamSeq,
    expected_len: usize,
    bytes: Vec<u8>,
    render_prefix: String,
    reply: Option<TerminalSnapshotReply>,
}

#[derive(Debug, Clone)]
struct HistoryAssembly {
    terminal: TerminalId,
    start_seq: Option<StreamSeq>,
    next_seq: Option<StreamSeq>,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CompletedHistory {
    terminal: TerminalId,
    start_seq: StreamSeq,
    end_seq: StreamSeq,
    bytes: Vec<u8>,
}

impl RuntimeState {
    fn active_stream(&self) -> Option<StreamId> {
        self.active.as_ref().map(|active| active.stream)
    }

    fn active_terminal(&self) -> Option<TerminalId> {
        self.active.as_ref().map(|active| active.terminal)
    }

    fn update_active_seq(&mut self, seq: StreamSeq) {
        if let Some(active) = self.active.as_mut() {
            active.last_seq = Some(seq);
        }
    }

    fn set_active(&mut self, info: &TerminalInfo, stream: StreamId, last_seq: Option<StreamSeq>) {
        self.info_cache.insert(info.terminal, info.clone());
        self.active = Some(ActiveTerminalState {
            terminal: info.terminal,
            stream,
            last_seq,
        });
    }

    fn clear_active(&mut self) {
        self.active = None;
        self.opening_snapshot = None;
    }

    fn store_info(&mut self, info: TerminalInfo) {
        self.info_cache.insert(info.terminal, info);
    }

    fn info(&self, terminal: TerminalId) -> Option<&TerminalInfo> {
        self.info_cache.get(&terminal)
    }

    fn record_create(&mut self, request_id: u32) {
        self.pending_creates.push(request_id);
    }

    fn remove_pending_create(&mut self, request_id: u32) -> bool {
        let Some(index) = self
            .pending_creates
            .iter()
            .position(|pending| *pending == request_id)
        else {
            return false;
        };
        self.pending_creates.remove(index);
        true
    }

    fn start_open(&mut self, request_id: u32, terminal: TerminalId, reply: TerminalSnapshotReply) {
        self.pending_opens.push(PendingOpen {
            request_id,
            terminal,
            reply: Some(reply),
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

    fn begin_snapshot(
        &mut self,
        info: TerminalInfo,
        stream: StreamId,
        head_seq: StreamSeq,
        expected_len: u32,
        render_prefix: String,
        reply: Option<TerminalSnapshotReply>,
    ) {
        let expected_len = usize::try_from(expected_len).unwrap_or(usize::MAX);
        let start_seq = StreamSeq(
            head_seq
                .0
                .saturating_sub(u64::try_from(expected_len).unwrap_or(u64::MAX)),
        );
        self.opening_snapshot = Some(SnapshotAssembly {
            info,
            stream,
            start_seq,
            head_seq,
            expected_len,
            bytes: Vec::new(),
            render_prefix,
            reply,
        });
    }

    fn snapshot_in_progress_for(&self, stream: StreamId) -> bool {
        self.opening_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.stream == stream)
    }

    fn push_snapshot_chunk(
        &mut self,
        stream: StreamId,
        seq: StreamSeq,
        offset: u32,
        bytes: &[u8],
        last: bool,
    ) -> Result<(), String> {
        let Some(snapshot) = self.opening_snapshot.as_mut() else {
            return Err("snapshot chunk arrived without active attach".to_owned());
        };

        if snapshot.stream != stream {
            return Err("snapshot chunk stream mismatch".to_owned());
        }
        if snapshot.head_seq != seq {
            return Err("snapshot chunk sequence mismatch".to_owned());
        }

        let offset = usize::try_from(offset).map_err(|_| "snapshot offset overflow")?;
        if offset != snapshot.bytes.len() {
            return Err("snapshot chunk offset mismatch".to_owned());
        }

        snapshot.bytes.extend_from_slice(bytes);
        let done = last || snapshot.bytes.len() >= snapshot.expected_len;
        if !done {
            return Ok(());
        }

        let completed = self
            .opening_snapshot
            .take()
            .ok_or_else(|| "snapshot assembly missing".to_owned())?;
        if completed.bytes.len() != completed.expected_len {
            return Err("snapshot chunk length mismatch".to_owned());
        }

        if let Some(active) = self.active.as_mut() {
            if active.stream == completed.stream {
                active.last_seq = Some(completed.head_seq);
            }
        }

        if let Some(reply) = completed.reply {
            if let Some(reply) = reply.borrow_mut().take() {
                let snapshot = TerminalSnapshot::new(
                    completed.info,
                    completed.start_seq,
                    completed.head_seq,
                    Bytes::from(completed.bytes),
                    completed.render_prefix,
                );
                let _ = reply.send(Ok(snapshot));
                return Ok(());
            }
        }

        Ok(())
    }

    fn begin_history(&mut self, request_id: u32, terminal: TerminalId) {
        self.pending_history.insert(
            request_id,
            HistoryAssembly {
                terminal,
                start_seq: None,
                next_seq: None,
                bytes: Vec::new(),
            },
        );
    }

    fn push_history_chunk(
        &mut self,
        request_id: u32,
        terminal: TerminalId,
        start_seq: StreamSeq,
        end_seq: StreamSeq,
        bytes: &[u8],
        last: bool,
    ) -> Result<(), String> {
        let Some(history) = self.pending_history.get_mut(&request_id) else {
            return Err("history chunk arrived without pending request".to_owned());
        };
        if history.terminal != terminal {
            return Err("history chunk terminal mismatch".to_owned());
        }
        if end_seq.0.saturating_sub(start_seq.0) != bytes.len() as u64 {
            return Err("history chunk length mismatch".to_owned());
        }
        if let Some(next_seq) = history.next_seq {
            if next_seq != start_seq {
                return Err("history chunk sequence gap".to_owned());
            }
        }

        history.bytes.extend_from_slice(bytes);
        if history.start_seq.is_none() {
            history.start_seq = Some(start_seq);
        }
        history.next_seq = Some(end_seq);
        if last {
            let completed = self
                .pending_history
                .remove(&request_id)
                .ok_or_else(|| "history assembly missing".to_owned())?;
            self.completed_history.insert(
                request_id,
                CompletedHistory {
                    terminal: completed.terminal,
                    start_seq: completed.start_seq.unwrap_or(start_seq),
                    end_seq,
                    bytes: completed.bytes,
                },
            );
        }

        Ok(())
    }

    fn take_completed_history(&mut self, request_id: u32) -> Option<CompletedHistory> {
        self.completed_history.remove(&request_id)
    }

    fn drop_history(&mut self, request_id: u32) {
        self.pending_history.remove(&request_id);
        self.completed_history.remove(&request_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use cli_pocket_proto::TerminalId;

    #[test]
    fn completed_history_uses_first_chunk_start_seq() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();

        runtime.begin_history(7, terminal);
        runtime
            .push_history_chunk(7, terminal, StreamSeq(10), StreamSeq(14), b"abcd", false)
            .expect("first chunk accepted");
        runtime
            .push_history_chunk(7, terminal, StreamSeq(14), StreamSeq(18), b"efgh", true)
            .expect("second chunk accepted");

        let completed = runtime
            .take_completed_history(7)
            .expect("history should complete");

        assert_eq!(completed.start_seq, StreamSeq(10));
        assert_eq!(completed.end_seq, StreamSeq(18));
        assert_eq!(completed.bytes, b"abcdefgh");
    }
}
