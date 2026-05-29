use bytes::Bytes;
use cli_pocket_crypto::{NoiseAnonymousInitiator, NoiseInitiator, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    ByeReason, Frame, FrameBody, Hello, ProtocolError, ResumeAttachment, ResumeToken, ServerId,
    SessionId, StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo,
    PROTOCOL_VERSION,
};
use futures_channel::mpsc;
use futures_util::{
    future::{Either, LocalBoxFuture},
    FutureExt, SinkExt, StreamExt,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::events::ClientEvent;
use crate::identity::ClientIdentity;
use crate::relay::open_client_pair;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionCommand {
    CreateTerminal(TerminalCreateParams),
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
        let session = ClientSession {
            terminal: Rc::clone(&terminal),
            session_cmd_tx,
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
) where
    T: Transport + 'static,
    C: Clock + 'static,
    R: Rng + 'static,
    K: KeyValueStore + 'static,
{
    let (start, max, mul) = config.backoff;
    let mut delay = start;
    let mut resume_state = ResumeState::from_config(config.resume_token.clone());
    let mut pending_cmds = VecDeque::<TerminalCmd>::new();
    let mut pending_session_cmds = VecDeque::<SessionCommand>::new();

    loop {
        let _ = events_tx.clone().send(ClientEvent::Connecting).await;
        let mut transport = match transport_factory().await {
            Ok(transport) => transport,
            Err(err) => {
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
            &mut transport,
            &mut cmd_rx,
            &mut session_cmd_rx,
            &mut pending_cmds,
            &mut pending_session_cmds,
            ConnectionState {
                resume: &mut resume_state,
                events_tx: events_tx.clone(),
                cmd_tx: &cmd_tx,
                terminal: &terminal,
            },
        )
        .await;

        resume_state.clear_pending_creates();

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

        sleep_backoff(&clock, &rng, delay).await;
        delay = crate::reconnect::next_delay(delay, max, mul);
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_one_connection<T: Transport>(
    identity: &ClientIdentity,
    config: &SessionConfig,
    transport: &mut T,
    cmd_rx: &mut mpsc::Receiver<TerminalCmd>,
    session_cmd_rx: &mut mpsc::Receiver<SessionCommand>,
    pending_cmds: &mut VecDeque<TerminalCmd>,
    pending_session_cmds: &mut VecDeque<SessionCommand>,
    state: ConnectionState<'_>,
) -> ClientResult<()> {
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
    let mut next_request_id = 2;

    let hello = Frame::body(FrameBody::Hello(Hello {
        protocol_min: PROTOCOL_VERSION,
        protocol_max: PROTOCOL_VERSION,
        resume: state.resume.token(),
    }));
    send_encrypted(transport, &mut session, &hello).await?;

    let hello_reply = recv_encrypted(transport, &mut session).await?;
    let (session_id, resumed, server_label) = match hello_reply.body {
        FrameBody::HelloOk(ok) => (ok.session_id, ok.resumed, ok.server_info.server_label),
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
    state.resume.set_session_id(session_id);
    if resumed {
        let attachment = {
            let handle = state.terminal.borrow().as_ref().cloned();
            handle
                .map(|handle| (handle.terminal_id(), handle.last_seq))
                .or_else(|| {
                    state
                        .resume
                        .first_terminal()
                        .map(|terminal| (terminal, None))
                })
        };
        if let Some((terminal, handle_last_seq)) = attachment {
            let since = handle_last_seq.or_else(|| state.resume.last_seq(terminal));
            state.resume.mark_pending_attach(1, terminal, false);
            send_encrypted(
                transport,
                &mut session,
                &Frame::body(FrameBody::TerminalAttach {
                    request_id: 1,
                    terminal,
                    since,
                }),
            )
            .await?;
        }
    } else if let Some(terminal) = {
        let terminal_guard = state.terminal.borrow();
        terminal_guard.as_ref().map(TerminalHandle::terminal_id)
    } {
        state.resume.mark_pending_attach(1, terminal, false);
        send_encrypted(
            transport,
            &mut session,
            &Frame::body(FrameBody::TerminalAttach {
                request_id: 1,
                terminal,
                since: None,
            }),
        )
        .await?;
    } else if let Some(terminal) = state.resume.first_terminal() {
        state.resume.mark_pending_attach(1, terminal, false);
        send_encrypted(
            transport,
            &mut session,
            &Frame::body(FrameBody::TerminalAttach {
                request_id: 1,
                terminal,
                since: None,
            }),
        )
        .await?;
    } else {
        send_encrypted(
            transport,
            &mut session,
            &Frame::body(FrameBody::TerminalList { request_id: 1 }),
        )
        .await?;
    }

    loop {
        enum Action {
            Send(Frame),
            SendPendingCommand(Frame),
            SendPendingSessionCommand(Frame),
            Emit(ClientEvent),
            Return(ClientResult<()>),
            None,
        }

        let action = {
            let next = if let Some(cmd) = pending_session_cmds.pop_front() {
                let frame =
                    frame_for_session_command(state.resume, cmd.clone(), &mut next_request_id);
                pending_session_cmds.push_front(cmd);
                Either::Left((Some(frame), ()))
            } else if let Some(cmd) = pending_cmds.pop_front() {
                if let Some(frame) = frame_for_command(state.resume, cmd.clone()) {
                    pending_cmds.push_front(cmd);
                    Either::Left((Some(frame), ()))
                } else {
                    pending_cmds.push_front(cmd);
                    Either::Right((recv_encrypted(transport, &mut session).await, ()))
                }
            } else {
                let next_cmd = cmd_rx.next().fuse();
                let next_session_cmd = session_cmd_rx.next().fuse();
                let next_frame = recv_encrypted(transport, &mut session).fuse();
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
                        let frame = frame_for_session_command(
                            state.resume,
                            cmd.clone(),
                            &mut next_request_id,
                        );
                        pending_session_cmds.push_back(cmd);
                        Either::Left((Some(frame), ()))
                    }
                    Either::Right((Either::Left((cmd, _pending_frame)), _pending_session_cmd)) => {
                        let Some(cmd) = cmd else {
                            return Ok(());
                        };
                        if let Some(frame) = frame_for_command(state.resume, cmd.clone()) {
                            pending_cmds.push_back(cmd);
                            Either::Left((Some(frame), ()))
                        } else {
                            pending_cmds.push_back(cmd);
                            Either::Left((None, ()))
                        }
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
                            let terminal_id = {
                                let mut guard = state.terminal.borrow_mut();
                                guard
                                    .as_mut()
                                    .and_then(|handle| {
                                        if handle.stream_id() == stream {
                                            handle.last_seq = Some(seq);
                                            state.resume.update_attachment(
                                                handle.terminal_id(),
                                                seq,
                                                handle.stream_id(),
                                            );
                                            Some(handle.terminal_id())
                                        } else {
                                            None
                                        }
                                    })
                                    .or_else(|| {
                                        let terminal = state.resume.terminal_for_stream(stream)?;
                                        state.resume.update_attachment(terminal, seq, stream);
                                        Some(terminal)
                                    })
                            };
                            terminal_id.map_or(Action::None, |terminal_id| {
                                Action::Emit(ClientEvent::TerminalOutput {
                                    terminal_id,
                                    stream_seq: seq,
                                    bytes: Bytes::from(bytes.into_vec()),
                                })
                            })
                        }
                        FrameBody::TerminalCreateOk {
                            request_id,
                            terminal: created_terminal,
                            stream,
                        } => {
                            if state
                                .resume
                                .complete_create(request_id, created_terminal, stream)
                                .is_some()
                            {
                                state.resume.mark_pending_attach(
                                    request_id,
                                    created_terminal,
                                    true,
                                );
                                Action::Send(Frame::body(FrameBody::TerminalAttach {
                                    request_id,
                                    terminal: created_terminal,
                                    since: None,
                                }))
                            } else {
                                Action::Emit(ClientEvent::Error(format!(
                                    "terminal create ok missing metadata for {created_terminal:?}"
                                )))
                            }
                        }
                        FrameBody::TerminalCreateErr { request_id, error } => {
                            if state.resume.remove_pending_create(request_id).is_some() {
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
                            stream,
                            head_seq,
                            ..
                        } => {
                            let pending = state.resume.take_pending_attach(request_id);
                            let mut created = None;
                            if let Some(pending) = pending {
                                let handled_existing = {
                                    let mut terminal_guard = state.terminal.borrow_mut();
                                    if let Some(handle) = terminal_guard.as_mut() {
                                        let terminal = handle.terminal_id();
                                        if pending.terminal == terminal {
                                            handle.set_stream_id(stream);
                                            handle.last_seq = Some(head_seq);
                                            state
                                                .resume
                                                .update_attachment(terminal, head_seq, stream);
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                };

                                if !handled_existing {
                                    state.resume.update_attachment(
                                        pending.terminal,
                                        head_seq,
                                        stream,
                                    );
                                    if let Some(info) = state.resume.info(pending.terminal).cloned()
                                    {
                                        let handle = TerminalHandle::new(
                                            info.clone(),
                                            stream,
                                            Some(head_seq),
                                            state.cmd_tx.clone(),
                                        );
                                        *state.terminal.borrow_mut() = Some(handle);
                                        if pending.emit_created {
                                            created = Some(ClientEvent::TerminalCreated(info));
                                        }
                                    }
                                }
                            }
                            created.map_or(Action::None, Action::Emit)
                        }
                        FrameBody::TerminalListOk { terminals, .. } => {
                            if state.terminal.borrow().is_some() {
                                Action::None
                            } else if let Some(info) = terminals.into_iter().next() {
                                state.resume.store_info(info.clone());
                                state.resume.mark_pending_attach(1, info.terminal, true);
                                let frame = Frame::body(FrameBody::TerminalAttach {
                                    request_id: 1,
                                    terminal: info.terminal,
                                    since: None,
                                });
                                Action::Send(frame)
                            } else {
                                Action::None
                            }
                        }
                        FrameBody::TerminalExit { terminal, exit } => {
                            *state.terminal.borrow_mut() = None;
                            state.resume.remove_attachment(terminal);
                            Action::Emit(ClientEvent::TerminalExited {
                                terminal_id: terminal,
                                info: exit,
                            })
                        }
                        FrameBody::Bye { reason } => {
                            if is_recoverable_bye(&reason) {
                                if matches!(
                                    reason,
                                    ByeReason::ProtocolError(ProtocolError::ResumeStale)
                                ) {
                                    state.resume.clear();
                                }
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

fn frame_for_command(resume: &ResumeState, cmd: TerminalCmd) -> Option<Frame> {
    match cmd {
        TerminalCmd::Input { terminal, bytes } => resume.stream(terminal).map(|stream| {
            Frame::body(FrameBody::Input {
                stream,
                bytes: bytes.to_vec().into(),
            })
        }),
        TerminalCmd::Resize {
            terminal,
            cols,
            rows,
        } => resume
            .stream(terminal)
            .map(|stream| Frame::body(FrameBody::Resize { stream, cols, rows })),
        TerminalCmd::Kill { terminal } => Some(Frame::body(FrameBody::TerminalKill {
            request_id: 0,
            terminal,
        })),
    }
}

fn frame_for_session_command(
    resume: &mut ResumeState,
    cmd: SessionCommand,
    next_request_id: &mut u32,
) -> Frame {
    match cmd {
        SessionCommand::CreateTerminal(params) => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            resume.record_create(request_id, params.clone());
            Frame::body(FrameBody::TerminalCreate { request_id, params })
        }
    }
}

struct ConnectionState<'a> {
    resume: &'a mut ResumeState,
    events_tx: mpsc::Sender<ClientEvent>,
    cmd_tx: &'a mpsc::Sender<TerminalCmd>,
    terminal: &'a Rc<RefCell<Option<TerminalHandle>>>,
}

async fn sleep_backoff<C: Clock, R: Rng>(clock: &C, rng: &R, delay_ms: u64) {
    let mut byte = [0_u8; 1];
    rng.fill(&mut byte);
    clock
        .sleep_ms(crate::reconnect::jitter(delay_ms, byte[0]))
        .await;
}

#[derive(Debug, Clone, Default)]
struct ResumeState {
    session_id: Option<SessionId>,
    attachments: Vec<AttachmentState>,
    pending_attach: Option<PendingAttach>,
    pending_creates: Vec<PendingCreate>,
}

#[derive(Debug, Clone)]
struct AttachmentState {
    terminal: TerminalId,
    stream: Option<StreamId>,
    last_seq: Option<StreamSeq>,
    info: Option<TerminalInfo>,
}

#[derive(Debug, Clone)]
struct PendingAttach {
    request_id: u32,
    terminal: TerminalId,
    emit_created: bool,
}

#[derive(Debug, Clone)]
struct PendingCreate {
    request_id: u32,
    params: TerminalCreateParams,
}

impl ResumeState {
    fn from_config(token: Option<ResumeToken>) -> Self {
        let Some(token) = token else {
            return Self::default();
        };

        Self {
            session_id: Some(token.session_id),
            attachments: token
                .attachments
                .into_iter()
                .map(|attachment| AttachmentState {
                    terminal: attachment.terminal,
                    stream: None,
                    last_seq: Some(attachment.last_seq),
                    info: None,
                })
                .collect(),
            pending_attach: None,
            pending_creates: Vec::new(),
        }
    }

    fn token(&self) -> Option<ResumeToken> {
        let session_id = self.session_id?;
        let attachments: Vec<_> = self
            .attachments
            .iter()
            .filter_map(|attachment| {
                attachment.last_seq.map(|last_seq| ResumeAttachment {
                    terminal: attachment.terminal,
                    last_seq,
                })
            })
            .collect();

        (!attachments.is_empty()).then_some(ResumeToken {
            session_id,
            attachments,
        })
    }

    fn set_session_id(&mut self, session_id: SessionId) {
        self.session_id = Some(session_id);
    }

    fn clear(&mut self) {
        self.session_id = None;
        for attachment in &mut self.attachments {
            attachment.last_seq = None;
            attachment.stream = None;
        }
        self.pending_attach = None;
        self.pending_creates.clear();
    }

    fn first_terminal(&self) -> Option<TerminalId> {
        self.attachments
            .first()
            .map(|attachment| attachment.terminal)
    }

    fn last_seq(&self, terminal: TerminalId) -> Option<StreamSeq> {
        self.attachments
            .iter()
            .find(|attachment| attachment.terminal == terminal)?
            .last_seq
    }

    fn stream(&self, terminal: TerminalId) -> Option<StreamId> {
        self.attachments
            .iter()
            .find(|attachment| attachment.terminal == terminal)?
            .stream
    }

    fn terminal_for_stream(&self, stream: StreamId) -> Option<TerminalId> {
        self.attachments
            .iter()
            .find(|attachment| attachment.stream == Some(stream))
            .map(|attachment| attachment.terminal)
    }

    fn store_info(&mut self, info: TerminalInfo) {
        let attachment = self.entry(info.terminal);
        attachment.info = Some(info);
    }

    fn record_create(&mut self, request_id: u32, params: TerminalCreateParams) {
        self.pending_creates
            .push(PendingCreate { request_id, params });
    }

    fn clear_pending_creates(&mut self) {
        self.pending_creates.clear();
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
    ) -> Option<TerminalInfo> {
        let idx = self
            .pending_creates
            .iter()
            .position(|pending| pending.request_id == request_id)?;
        let pending = self.pending_creates.remove(idx);
        let info = TerminalInfo {
            terminal,
            cols: pending.params.cols,
            rows: pending.params.rows,
            created_at_unix_ms: 0,
            label: pending.params.cmd.first().cloned(),
            attached_clients: 1,
        };
        let attachment = self.entry(terminal);
        attachment.info = Some(info.clone());
        attachment.stream = Some(stream);
        Some(info)
    }

    fn info(&self, terminal: TerminalId) -> Option<&TerminalInfo> {
        self.attachments
            .iter()
            .find(|attachment| attachment.terminal == terminal)?
            .info
            .as_ref()
    }

    fn update_attachment(&mut self, terminal: TerminalId, last_seq: StreamSeq, stream: StreamId) {
        let attachment = self.entry(terminal);
        attachment.stream = Some(stream);
        attachment.last_seq = Some(last_seq);
    }

    fn mark_pending_attach(&mut self, request_id: u32, terminal: TerminalId, emit_created: bool) {
        self.entry(terminal).stream = None;
        self.pending_attach = Some(PendingAttach {
            request_id,
            terminal,
            emit_created,
        });
    }

    fn take_pending_attach(&mut self, request_id: u32) -> Option<PendingAttach> {
        if self
            .pending_attach
            .as_ref()
            .is_some_and(|pending| pending.request_id == request_id)
        {
            self.pending_attach.take()
        } else {
            None
        }
    }

    fn remove_attachment(&mut self, terminal: TerminalId) {
        self.attachments
            .retain(|attachment| attachment.terminal != terminal);
    }

    fn entry(&mut self, terminal: TerminalId) -> &mut AttachmentState {
        if let Some(idx) = self
            .attachments
            .iter()
            .position(|attachment| attachment.terminal == terminal)
        {
            return &mut self.attachments[idx];
        }

        let idx = self.attachments.len();
        self.attachments.push(AttachmentState {
            terminal,
            stream: None,
            last_seq: None,
            info: None,
        });
        &mut self.attachments[idx]
    }
}

fn is_recoverable_bye(reason: &ByeReason) -> bool {
    matches!(
        reason,
        ByeReason::ServerShutdown
            | ByeReason::ProtocolError(
                ProtocolError::BackpressureExceeded | ProtocolError::ResumeStale
            )
    )
}

fn parse_psk_hex(psk_hex: &str) -> ClientResult<[u8; 32]> {
    let bytes = hex::decode(psk_hex)
        .map_err(|error| ClientError::Transport(format!("relay psk_hex: {error}")))?;
    bytes
        .try_into()
        .map_err(|_| ClientError::Transport("relay psk_hex must be 32 bytes".to_owned()))
}
