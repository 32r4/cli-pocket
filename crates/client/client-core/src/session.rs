use bytes::Bytes;
use cli_pocket_crypto::{NoiseAnonymousInitiator, NoiseInitiator, NoiseSession};
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::{
    ByeReason, EventBody, EventFrame, Frame, FrameBody, Hello, ProtocolError, RequestBody,
    RequestFrame, RequestId, ResponseBody, ResponseFrame, ResumeToken, ServerConfig, ServerId,
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
use std::sync::OnceLock;
use std::time::Instant;
use tracing::{info, trace};

use crate::events::ClientEvent;
use crate::history::TerminalHistoryPage;
use crate::identity::ClientIdentity;
use crate::open_ack::TerminalOpenAck;
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

const HEARTBEAT_IDLE_MS: u64 = 10_000;
const HEARTBEAT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_INPUT_BATCH_WINDOW_MS: u64 = 8;
const MAX_INPUT_BATCH_WINDOW_MS: u64 = 50;
const INPUT_PROBE_SUMMARY_SAMPLES: usize = 32;
const INPUT_PROBE_SLOW_US: u64 = 100_000;

type SharedReply<T> = Rc<RefCell<Option<oneshot::Sender<ClientResult<T>>>>>;
type TerminalOpenAckReply = SharedReply<TerminalOpenAck>;
type HistoryReply = SharedReply<TerminalHistoryPage>;
type TerminalCreateReply = SharedReply<TerminalInfo>;
type TerminalListReply = SharedReply<Vec<TerminalInfo>>;
type ServerConfigReply = SharedReply<ServerConfig>;
type UnitReply = SharedReply<()>;

fn input_probe_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("CLI_POCKET_INPUT_PROBE")
            .is_ok_and(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
    })
}

fn input_batch_window_ms() -> u64 {
    static WINDOW_MS: OnceLock<u64> = OnceLock::new();
    *WINDOW_MS.get_or_init(|| {
        std::env::var("CLI_POCKET_INPUT_BATCH_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map_or(DEFAULT_INPUT_BATCH_WINDOW_MS, |value| {
                value.min(MAX_INPUT_BATCH_WINDOW_MS)
            })
    })
}

fn elapsed_micros_u64(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn percentile_value(sorted_samples: &[u64], percentile: usize) -> u64 {
    debug_assert!(!sorted_samples.is_empty());
    let len = sorted_samples.len();
    let rank = percentile.saturating_mul(len).div_ceil(100).max(1);
    sorted_samples[rank.saturating_sub(1).min(len.saturating_sub(1))]
}

#[derive(Debug, Clone)]
struct InputLatencySummary {
    count: usize,
    min_us: u64,
    p50_us: u64,
    p95_us: u64,
    max_us: u64,
    slow_count: usize,
}

#[derive(Debug, Clone)]
struct InputLatencyWindow {
    slow_threshold_us: u64,
    samples_us: Vec<u64>,
}

impl Default for InputLatencyWindow {
    fn default() -> Self {
        Self {
            slow_threshold_us: INPUT_PROBE_SLOW_US,
            samples_us: Vec::with_capacity(INPUT_PROBE_SUMMARY_SAMPLES),
        }
    }
}

impl InputLatencyWindow {
    fn record(&mut self, sample_us: u64) -> Option<InputLatencySummary> {
        self.samples_us.push(sample_us);
        if self.samples_us.len() < INPUT_PROBE_SUMMARY_SAMPLES {
            return None;
        }

        let mut samples = std::mem::take(&mut self.samples_us);
        samples.sort_unstable();
        let count = samples.len();
        let min_us = samples[0];
        let max_us = samples[count.saturating_sub(1)];
        let p50_us = percentile_value(&samples, 50);
        let p95_us = percentile_value(&samples, 95);
        let slow_count = samples
            .iter()
            .filter(|&&sample| sample >= self.slow_threshold_us)
            .count();
        self.samples_us = Vec::with_capacity(INPUT_PROBE_SUMMARY_SAMPLES);

        Some(InputLatencySummary {
            count,
            min_us,
            p50_us,
            p95_us,
            max_us,
            slow_count,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct ClientInputProbeStats {
    ack_delay: InputLatencyWindow,
}

impl ClientInputProbeStats {
    fn record_ack(&mut self, probe: &PendingInputProbe, ack_delay_us: u64) {
        if ack_delay_us >= INPUT_PROBE_SLOW_US {
            info!(
                target: "cli_pocket_probe::input_latency",
                component = "client",
                metric = "ack_delay_us",
                terminal_id = ?probe.terminal_id,
                request_id = probe.request_id,
                bytes_len = probe.bytes_len,
                queue_delay_us = probe.queue_delay_us,
                delay_us = ack_delay_us,
                slow_threshold_us = INPUT_PROBE_SLOW_US,
                "client send_input ack slow"
            );
        }
        if let Some(summary) = self.ack_delay.record(ack_delay_us) {
            info!(
                target: "cli_pocket_probe::input_latency",
                component = "client",
                metric = "ack_delay_us",
                samples = summary.count,
                min_us = summary.min_us,
                p50_us = summary.p50_us,
                p95_us = summary.p95_us,
                max_us = summary.max_us,
                slow_threshold_us = INPUT_PROBE_SLOW_US,
                slow_samples = summary.slow_count,
                "client input latency summary"
            );
        }
    }
}

fn fail_shared_reply<T>(reply: &SharedReply<T>, error: &ClientError) {
    if let Some(reply) = reply.borrow_mut().take() {
        let _ = reply.send(Err(error.clone()));
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

#[derive(Clone)]
pub struct ClientSession {
    terminal: Rc<RefCell<Option<TerminalHandle>>>,
    session_cmd_tx: mpsc::Sender<SessionCommand>,
    stop_requested: Rc<Cell<bool>>,
}

#[derive(Debug, Clone)]
enum SessionCommand {
    CreateTerminal {
        params: TerminalCreateParams,
        reply: TerminalCreateReply,
    },
    OpenTerminal {
        terminal: TerminalId,
        reply: TerminalOpenAckReply,
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

    pub async fn create_terminal(
        &self,
        params: TerminalCreateParams,
    ) -> ClientResult<TerminalInfo> {
        let (reply_tx, reply_rx) = oneshot::channel();
        trace!(terminal_create = true, "queue create terminal request");
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::CreateTerminal {
                params,
                reply: Rc::new(RefCell::new(Some(reply_tx))),
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        let result = reply_rx.await.map_err(|_| ClientError::Closed)?;
        trace!(terminal_create = true, "create terminal resolved");
        result
    }

    pub async fn open_terminal(&self, terminal: TerminalId) -> ClientResult<TerminalOpenAck> {
        let (reply_tx, reply_rx) = oneshot::channel();
        trace!(terminal = ?terminal, "queue open terminal request");
        self.session_cmd_tx
            .clone()
            .send(SessionCommand::OpenTerminal {
                terminal,
                reply: Rc::new(RefCell::new(Some(reply_tx))),
            })
            .await
            .map_err(|_| ClientError::Closed)?;

        let result = reply_rx.await.map_err(|_| ClientError::Closed)?;
        trace!(terminal = ?terminal, "open terminal resolved");
        result
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

        let result = reply_rx.await.map_err(|_| ClientError::Closed)?;
        result
    }

    pub async fn read_history(
        &self,
        terminal: TerminalId,
        before: Option<StreamSeq>,
        max_bytes: u32,
    ) -> ClientResult<TerminalHistoryPage> {
        let (reply_tx, reply_rx) = oneshot::channel();
        trace!(terminal = ?terminal, before = ?before, max_bytes, "queue history request");
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

        let result = reply_rx.await.map_err(|_| ClientError::Closed)?;
        trace!(terminal = ?terminal, before = ?before, max_bytes, "history request resolved");
        result
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
                let will_retry = should_retry_after_disconnect(&err);
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

    let outcome = loop {
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
                break Err(ClientError::Transport("heartbeat timeout".to_owned()));
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
                );
                pending_session_cmds.push_front(cmd);
                Either::Left((Some(frame), ()))
            } else if let Some(cmd) = pending_cmds.pop_front() {
                pending_cmds.push_front(cmd);
                coalesce_pending_input_front(pending_cmds);
                let Some(cmd) = pending_cmds.pop_front() else {
                    return Err(ClientError::Closed);
                };
                let active = state.terminal.borrow().clone();
                let frame = frame_for_command(
                    &mut runtime,
                    active.as_ref(),
                    cmd.clone(),
                    &mut next_request_id,
                );
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
                            return Err(ClientError::Closed);
                        };
                        if matches!(cmd, SessionCommand::Shutdown) {
                            return Ok(());
                        }
                        let frame = frame_for_session_command(
                            &mut runtime,
                            state.terminal,
                            cmd.clone(),
                            &mut next_request_id,
                        );
                        pending_session_cmds.push_back(cmd);
                        Either::Left((Some(frame), ()))
                    }
                    Either::Right((Either::Left((cmd, _)), _)) => {
                        let Some(cmd) = cmd else {
                            return Err(ClientError::Closed);
                        };
                        let (cmd, deferred_cmds) = batch_input_command(clock, cmd, cmd_rx).await;
                        let active = state.terminal.borrow().clone();
                        let frame = frame_for_command(
                            &mut runtime,
                            active.as_ref(),
                            cmd.clone(),
                            &mut next_request_id,
                        );
                        pending_cmds.push_back(cmd);
                        pending_cmds.extend(deferred_cmds);
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
                        frame.map_or(Action::DropPendingCommand, Action::SendPendingCommand)
                    }
                }
                Either::Right((frame, ())) => {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(err) => return Err(err),
                    };
                    handle_inbound_frame(frame, &mut runtime, &state)?
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
            Action::DropPendingCommand => {
                pending_cmds.pop_front();
            }
            Action::SendPendingSessionCommand(frame) => {
                send_encrypted(transport, &mut session, &frame).await?;
                pending_session_cmds.pop_front();
            }
            Action::Emit(event) => {
                let _ = state.events_tx.clone().send(event).await;
            }
            Action::Return(result) => break result,
            Action::None => {}
        }
    };

    let disconnect_error = match &outcome {
        Ok(()) => ClientError::Closed,
        Err(error) => error.clone(),
    };
    fail_all_pending_requests(&mut runtime, &disconnect_error);

    outcome
}

#[allow(clippy::too_many_arguments)]
fn handle_inbound_frame(
    frame: Frame,
    runtime: &mut RuntimeState,
    state: &ConnectionState<'_>,
) -> ClientResult<Action> {
    let action = match frame.body {
        FrameBody::Response(response) => handle_response_frame(response, runtime, state)?,
        FrameBody::StreamData(stream) => handle_stream_data_frame(stream, runtime, state)?,
        FrameBody::Event(event) => handle_event_frame(event, runtime),
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

fn handle_event_frame(event: EventFrame, runtime: &mut RuntimeState) -> Action {
    match event.body {
        EventBody::TerminalCreated { info } => {
            runtime.store_info(info.clone());
            Action::Emit(ClientEvent::TerminalCreated(info))
        }
        EventBody::Error { message, .. } => Action::Emit(ClientEvent::Error(message)),
        EventBody::Disconnected { reason } => Action::Emit(ClientEvent::Disconnected {
            will_retry: true,
            reason,
        }),
        EventBody::Connected => Action::None,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_response_frame(
    response: ResponseFrame,
    runtime: &mut RuntimeState,
    state: &ConnectionState<'_>,
) -> ClientResult<Action> {
    let request_id = response.id.0;
    let body = match response.result {
        Ok(body) => body,
        Err(error) => {
            fail_request(request_id, &ClientError::Proto(error.message), runtime)?;
            return Ok(Action::None);
        }
    };
    match body {
        ResponseBody::ListTerminals { terminals } => {
            for terminal in &terminals {
                runtime.store_info(terminal.clone());
            }
            let Some(RequestTxn::ListTerminals { reply }) =
                runtime.pending_requests.remove(&request_id)
            else {
                return Err(ClientError::Proto(format!(
                    "terminal list response for unknown request {request_id}"
                )));
            };
            let reply = reply.borrow_mut().take();
            if let Some(reply) = reply {
                let _ = reply.send(Ok(terminals));
            }
        }
        ResponseBody::CreateTerminal { info } => {
            let Some(reply) = runtime.remove_pending_create(request_id) else {
                return Err(ClientError::Proto(format!(
                    "terminal create response for unknown request {request_id}"
                )));
            };
            runtime.store_info(info.clone());
            if let Some(reply) = reply.borrow_mut().take() {
                let _ = reply.send(Ok(info.clone()));
            }
            return Ok(Action::Emit(ClientEvent::TerminalCreated(info)));
        }
        ResponseBody::OpenTerminal { ack } => {
            let stream_id = ack.stream_id;
            let terminal_info = ack.info.clone();
            if !matches!(
                runtime.pending_requests.get(&request_id),
                Some(RequestTxn::OpenTerminal { .. })
            ) {
                return Err(ClientError::Proto(format!(
                    "open terminal response for unknown request {request_id}"
                )));
            }
            runtime.store_info(terminal_info.clone());
            runtime.set_active(&terminal_info, stream_id, Some(ack.end_seq));
            *state.terminal.borrow_mut() = Some(TerminalHandle::new(
                terminal_info.clone(),
                stream_id,
                Some(ack.end_seq),
                state.cmd_tx.clone(),
            ));
            runtime
                .complete_open_terminal(
                    request_id,
                    stream_id,
                    TerminalOpenAck::new(
                        ack.stream_id,
                        ack.info,
                        ack.start_seq,
                        ack.end_seq,
                        Bytes::from(ack.render_bytes.into_vec()),
                        ack.has_more_history,
                    ),
                )
                .map_err(ClientError::Proto)?;
            return Ok(Action::None);
        }
        ResponseBody::ReadHistory { page } => {
            let Some(RequestTxn::ReadHistory { terminal_id, reply }) =
                runtime.pending_requests.remove(&request_id)
            else {
                return Err(ClientError::Proto(format!(
                    "history response for unknown request {request_id}"
                )));
            };
            if terminal_id != page.terminal_id {
                return Err(ClientError::Proto(
                    "history response terminal mismatch".to_owned(),
                ));
            }
            let reply = reply.borrow_mut().take();
            if let Some(reply) = reply {
                let _ = reply.send(Ok(TerminalHistoryPage::new(
                    page.terminal_id,
                    page.start_seq,
                    page.end_seq,
                    Bytes::from(page.bytes.into_vec()),
                    page.has_more,
                )));
            }
        }
        ResponseBody::KillTerminal => {
            let Some(RequestTxn::KillTerminal { reply }) =
                runtime.pending_requests.remove(&request_id)
            else {
                return Err(ClientError::Proto(format!(
                    "kill response for unknown request {request_id}"
                )));
            };
            if let Some(reply) = reply {
                let reply = reply.borrow_mut().take();
                if let Some(reply) = reply {
                    let _ = reply.send(Ok(()));
                }
            }
        }
        ResponseBody::GetServerConfig { config } => {
            let Some(RequestTxn::GetConfig { reply }) =
                runtime.pending_requests.remove(&request_id)
            else {
                return Err(ClientError::Proto(format!(
                    "server config get response for unknown request {request_id}"
                )));
            };
            let reply = reply.borrow_mut().take();
            if let Some(reply) = reply {
                let _ = reply.send(Ok(config));
            }
        }
        ResponseBody::SetServerConfig { config } => {
            let Some(RequestTxn::SetConfig { reply }) =
                runtime.pending_requests.remove(&request_id)
            else {
                return Err(ClientError::Proto(format!(
                    "server config set response for unknown request {request_id}"
                )));
            };
            let reply = reply.borrow_mut().take();
            if let Some(reply) = reply {
                let _ = reply.send(Ok(config));
            }
        }
        ResponseBody::SendInput => {
            let Some(probe) = runtime.take_pending_input_ack(request_id) else {
                return Err(ClientError::Proto(format!(
                    "ack response for unknown request {request_id}"
                )));
            };
            let ack_delay_us = elapsed_micros_u64(probe.sent_at);
            if input_probe_enabled() {
                runtime.input_probe_stats.record_ack(&probe, ack_delay_us);
            }
        }
        ResponseBody::ResizeTerminal => {
            if !runtime.take_pending_ack(request_id) {
                return Err(ClientError::Proto(format!(
                    "ack response for unknown request {request_id}"
                )));
            }
        }
    }

    Ok(Action::None)
}

fn fail_request(
    request_id: u32,
    error: &ClientError,
    runtime: &mut RuntimeState,
) -> ClientResult<()> {
    let Some(txn) = runtime.pending_requests.remove(&request_id) else {
        return Err(ClientError::Proto(format!(
            "response for unknown request {request_id}"
        )));
    };

    match txn {
        RequestTxn::OpenTerminal { reply, .. } => fail_shared_reply(&reply, error),
        RequestTxn::ReadHistory { reply, .. } => fail_shared_reply(&reply, error),
        RequestTxn::ListTerminals { reply } => fail_shared_reply(&reply, error),
        RequestTxn::GetConfig { reply } | RequestTxn::SetConfig { reply } => {
            fail_shared_reply(&reply, error);
        }
        RequestTxn::KillTerminal { reply } => {
            if let Some(reply) = reply {
                fail_shared_reply(&reply, error);
            }
        }
        RequestTxn::CreateTerminal { reply } => fail_shared_reply(&reply, error),
        RequestTxn::SendInputAck { probe } => {
            runtime.discard_pending_input_output_probe(&probe);
        }
        RequestTxn::ResizeAck => {}
    }

    Ok(())
}

fn fail_all_pending_requests(runtime: &mut RuntimeState, error: &ClientError) {
    let pending = std::mem::take(&mut runtime.pending_requests);
    for txn in pending.into_values() {
        match txn {
            RequestTxn::OpenTerminal { reply, .. } => fail_shared_reply(&reply, error),
            RequestTxn::ReadHistory { reply, .. } => fail_shared_reply(&reply, error),
            RequestTxn::ListTerminals { reply } => fail_shared_reply(&reply, error),
            RequestTxn::GetConfig { reply } | RequestTxn::SetConfig { reply } => {
                fail_shared_reply(&reply, error);
            }
            RequestTxn::KillTerminal { reply } => {
                if let Some(reply) = reply {
                    fail_shared_reply(&reply, error);
                }
            }
            RequestTxn::CreateTerminal { reply } => fail_shared_reply(&reply, error),
            RequestTxn::SendInputAck { probe } => {
                runtime.discard_pending_input_output_probe(&probe);
            }
            RequestTxn::ResizeAck => {}
        }
    }

    runtime.active_streams.clear();
    runtime.clear_active();
}

fn handle_stream_data_frame(
    stream: cli_pocket_proto::StreamDataFrame,
    runtime: &mut RuntimeState,
    state: &ConnectionState<'_>,
) -> ClientResult<Action> {
    match runtime.active_streams.get(&stream.stream_id).cloned() {
        Some(StreamState::Output {
            terminal_id,
            last_seq,
        }) => {
            if stream.offset.is_some() {
                return Err(ClientError::Proto(
                    "output stream chunk must not include offset".to_owned(),
                ));
            }
            if runtime.active_stream() != Some(stream.stream_id) {
                return Ok(Action::None);
            }
            if let Some(active) = runtime.active.as_ref() {
                if active.terminal != terminal_id {
                    return Err(ClientError::Proto("output terminal mismatch".to_owned()));
                }
                if stream.seq < last_seq {
                    return Err(ClientError::Proto(
                        "output sequence moved backwards".to_owned(),
                    ));
                }
            }
            runtime.active_streams.insert(
                stream.stream_id,
                StreamState::Output {
                    terminal_id,
                    last_seq: stream.seq,
                },
            );
            if let Some(handle) = state.terminal.borrow_mut().as_mut() {
                handle.last_seq = Some(stream.seq);
                runtime.update_active_seq(stream.seq);
                if let Some(probe) = runtime.pop_pending_input_output_probe(terminal_id) {
                    let _ = probe;
                }
                Ok(Action::Emit(ClientEvent::TerminalOutput {
                    terminal_id: handle.terminal_id(),
                    stream_seq: stream.seq,
                    bytes: Bytes::from(stream.bytes.into_vec()),
                }))
            } else {
                Ok(Action::None)
            }
        }
        None => Err(ClientError::Proto(
            "stream data on unknown stream".to_owned(),
        )),
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
    trace!(direction = "send", ?frame, "client wire frame");
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
    let frame = decode_frame(&plaintext)?;
    trace!(direction = "recv", ?frame, "client wire frame");
    Ok(frame)
}

async fn recv_transport<T: Transport>(transport: &mut T) -> ClientResult<Vec<u8>> {
    transport.recv().await?.ok_or(ClientError::Closed)
}

fn frame_for_command(
    runtime: &mut RuntimeState,
    active: Option<&TerminalHandle>,
    cmd: TerminalCmd,
    next_request_id: &mut u32,
) -> Option<Frame> {
    match cmd {
        TerminalCmd::Input {
            terminal,
            bytes,
            queued_at,
        } => active
            .filter(|handle| handle.terminal_id() == terminal)
            .map(|handle| {
                let request_id = *next_request_id;
                *next_request_id = next_request_id.saturating_add(1);
                let probe = PendingInputProbe {
                    request_id,
                    terminal_id: handle.terminal_id(),
                    bytes_len: bytes.len(),
                    queue_delay_us: elapsed_micros_u64(queued_at),
                    sent_at: Instant::now(),
                };
                runtime.record_send_input_ack(probe);
                request_frame(
                    request_id,
                    RequestBody::SendInput {
                        terminal_id: handle.terminal_id(),
                        bytes: bytes.to_vec().into(),
                    },
                )
            }),
        TerminalCmd::Resize {
            terminal,
            cols,
            rows,
        } => active
            .filter(|handle| handle.terminal_id() == terminal)
            .map(|handle| {
                let request_id = *next_request_id;
                *next_request_id = next_request_id.saturating_add(1);
                runtime.record_resize_ack(request_id);
                request_frame(
                    request_id,
                    RequestBody::ResizeTerminal {
                        terminal_id: handle.terminal_id(),
                        cols,
                        rows,
                    },
                )
            }),
        TerminalCmd::Kill { terminal } => active
            .filter(|handle| handle.terminal_id() == terminal)
            .map(|handle| {
                let request_id = *next_request_id;
                *next_request_id = next_request_id.saturating_add(1);
                runtime.record_kill_ack(request_id);
                request_frame(
                    request_id,
                    RequestBody::KillTerminal {
                        terminal_id: handle.terminal_id(),
                    },
                )
            }),
    }
}

fn try_merge_input_command(base: &mut TerminalCmd, next: TerminalCmd) -> Result<(), TerminalCmd> {
    match (base, next) {
        (
            TerminalCmd::Input {
                terminal: base_terminal,
                bytes: base_bytes,
                ..
            },
            TerminalCmd::Input {
                terminal,
                bytes,
                queued_at: _,
            },
        ) if *base_terminal == terminal => {
            let mut merged = Vec::with_capacity(base_bytes.len().saturating_add(bytes.len()));
            merged.extend_from_slice(base_bytes.as_ref());
            merged.extend_from_slice(bytes.as_ref());
            *base_bytes = Bytes::from(merged);
            Ok(())
        }
        (_, next) => Err(next),
    }
}

fn coalesce_pending_input_front(pending_cmds: &mut VecDeque<TerminalCmd>) {
    let Some(mut front) = pending_cmds.pop_front() else {
        return;
    };
    while let Some(next) = pending_cmds.pop_front() {
        match try_merge_input_command(&mut front, next) {
            Ok(()) => {}
            Err(next) => {
                pending_cmds.push_front(next);
                break;
            }
        }
    }
    pending_cmds.push_front(front);
}

async fn batch_input_command<C: Clock>(
    clock: &C,
    first_cmd: TerminalCmd,
    cmd_rx: &mut mpsc::Receiver<TerminalCmd>,
) -> (TerminalCmd, Vec<TerminalCmd>) {
    let TerminalCmd::Input { .. } = first_cmd else {
        return (first_cmd, Vec::new());
    };

    let window_ms = input_batch_window_ms();
    if window_ms == 0 {
        return (first_cmd, Vec::new());
    }

    let deadline_ms = clock.now_ms().saturating_add(window_ms);
    let mut cmd = first_cmd;
    let mut deferred_cmds = Vec::new();

    loop {
        let remaining_ms = deadline_ms.saturating_sub(clock.now_ms());
        if remaining_ms == 0 {
            break;
        }

        let next_cmd = cmd_rx.next().fuse();
        let sleep = clock.sleep_ms(remaining_ms).fuse();
        futures_util::pin_mut!(next_cmd, sleep);

        match futures_util::future::select(next_cmd, sleep).await {
            Either::Left((Some(next_cmd), _)) => {
                match try_merge_input_command(&mut cmd, next_cmd) {
                    Ok(()) => {}
                    Err(next_cmd) => deferred_cmds.push(next_cmd),
                }
            }
            Either::Left((None, _)) | Either::Right(((), _)) => break,
        }
    }

    (cmd, deferred_cmds)
}

#[allow(clippy::too_many_arguments)]
fn frame_for_session_command(
    runtime: &mut RuntimeState,
    active_terminal: &Rc<RefCell<Option<TerminalHandle>>>,
    cmd: SessionCommand,
    next_request_id: &mut u32,
) -> Frame {
    match cmd {
        SessionCommand::CreateTerminal { params, reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_create(request_id, reply);
            request_frame(request_id, RequestBody::CreateTerminal { params })
        }
        SessionCommand::OpenTerminal {
            terminal: target,
            reply,
        } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.start_open_terminal(request_id, target, reply);

            if active_terminal.borrow().is_some() {
                clear_active_terminal(active_terminal, runtime);
                request_frame(
                    request_id,
                    RequestBody::OpenTerminal {
                        terminal_id: target,
                    },
                )
            } else {
                request_frame(
                    request_id,
                    RequestBody::OpenTerminal {
                        terminal_id: target,
                    },
                )
            }
        }
        SessionCommand::ListTerminals { reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_list(request_id, reply);
            request_frame(request_id, RequestBody::ListTerminals)
        }
        SessionCommand::ReadHistory {
            terminal,
            before,
            max_bytes,
            reply,
        } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_history(request_id, terminal, reply);
            request_frame(
                request_id,
                RequestBody::ReadHistory {
                    terminal_id: terminal,
                    before,
                    max_bytes,
                },
            )
        }
        SessionCommand::GetServerConfig { reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_get_config(request_id, reply);
            request_frame(request_id, RequestBody::GetServerConfig)
        }
        SessionCommand::SetServerConfig { config, reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_set_config(request_id, reply);
            request_frame(request_id, RequestBody::SetServerConfig { config })
        }
        SessionCommand::KillTerminal { terminal, reply } => {
            let request_id = *next_request_id;
            *next_request_id = next_request_id.saturating_add(1);
            runtime.record_kill_terminal(request_id, reply);
            if runtime.active_terminal() == Some(terminal) {
                clear_active_terminal(active_terminal, runtime);
            }
            request_frame(
                request_id,
                RequestBody::KillTerminal {
                    terminal_id: terminal,
                },
            )
        }
        SessionCommand::Shutdown => unreachable!("shutdown commands are handled before framing"),
    }
}

fn request_frame(request_id: u32, body: RequestBody) -> Frame {
    Frame::body(FrameBody::Request(RequestFrame {
        id: RequestId(request_id),
        body,
    }))
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
    DropPendingCommand,
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
    pending_requests: HashMap<u32, RequestTxn>,
    active_streams: HashMap<StreamId, StreamState>,
    pending_input_output_probes: HashMap<TerminalId, VecDeque<PendingInputProbe>>,
    input_probe_stats: ClientInputProbeStats,
}

#[derive(Debug, Clone)]
struct ActiveTerminalState {
    terminal: TerminalId,
    stream: StreamId,
    last_seq: Option<StreamSeq>,
}

#[derive(Debug, Clone)]
enum RequestTxn {
    OpenTerminal {
        terminal_id: TerminalId,
        reply: TerminalOpenAckReply,
    },
    ReadHistory {
        terminal_id: TerminalId,
        reply: HistoryReply,
    },
    CreateTerminal {
        reply: TerminalCreateReply,
    },
    ListTerminals {
        reply: TerminalListReply,
    },
    GetConfig {
        reply: ServerConfigReply,
    },
    SetConfig {
        reply: ServerConfigReply,
    },
    SendInputAck {
        probe: PendingInputProbe,
    },
    ResizeAck,
    KillTerminal {
        reply: Option<UnitReply>,
    },
}

#[derive(Debug, Clone)]
enum StreamState {
    Output {
        terminal_id: TerminalId,
        last_seq: StreamSeq,
    },
}

#[derive(Debug, Clone)]
struct PendingInputProbe {
    request_id: u32,
    terminal_id: TerminalId,
    bytes_len: usize,
    queue_delay_us: u64,
    sent_at: Instant,
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
        if let Some(active_stream) = self.active.as_ref().map(|active| active.stream) {
            if matches!(
                self.active_streams.get(&active_stream),
                Some(StreamState::Output { .. })
            ) {
                self.active_streams.remove(&active_stream);
            }
        }
        self.active = None;
    }

    fn store_info(&mut self, info: TerminalInfo) {
        self.info_cache.insert(info.terminal, info);
    }

    fn record_create(&mut self, request_id: u32, reply: TerminalCreateReply) {
        self.pending_requests
            .insert(request_id, RequestTxn::CreateTerminal { reply });
    }

    fn record_list(&mut self, request_id: u32, reply: TerminalListReply) {
        self.pending_requests
            .insert(request_id, RequestTxn::ListTerminals { reply });
    }

    fn record_get_config(&mut self, request_id: u32, reply: ServerConfigReply) {
        self.pending_requests
            .insert(request_id, RequestTxn::GetConfig { reply });
    }

    fn record_set_config(&mut self, request_id: u32, reply: ServerConfigReply) {
        self.pending_requests
            .insert(request_id, RequestTxn::SetConfig { reply });
    }

    fn record_kill_terminal(&mut self, request_id: u32, reply: UnitReply) {
        self.pending_requests
            .insert(request_id, RequestTxn::KillTerminal { reply: Some(reply) });
    }

    fn remove_pending_create(&mut self, request_id: u32) -> Option<TerminalCreateReply> {
        match self.pending_requests.remove(&request_id) {
            Some(RequestTxn::CreateTerminal { reply }) => Some(reply),
            _ => None,
        }
    }

    fn record_send_input_ack(&mut self, probe: PendingInputProbe) {
        self.pending_input_output_probes
            .entry(probe.terminal_id)
            .or_default()
            .push_back(probe.clone());
        self.pending_requests
            .insert(probe.request_id, RequestTxn::SendInputAck { probe });
    }

    fn record_resize_ack(&mut self, request_id: u32) {
        self.pending_requests
            .insert(request_id, RequestTxn::ResizeAck);
    }

    fn record_kill_ack(&mut self, request_id: u32) {
        self.pending_requests
            .insert(request_id, RequestTxn::KillTerminal { reply: None });
    }

    fn take_pending_ack(&mut self, request_id: u32) -> bool {
        matches!(
            self.pending_requests.remove(&request_id),
            Some(RequestTxn::ResizeAck)
        )
    }

    fn take_pending_input_ack(&mut self, request_id: u32) -> Option<PendingInputProbe> {
        match self.pending_requests.remove(&request_id) {
            Some(RequestTxn::SendInputAck { probe }) => Some(probe),
            Some(other) => {
                self.pending_requests.insert(request_id, other);
                None
            }
            None => None,
        }
    }

    fn pop_pending_input_output_probe(
        &mut self,
        terminal_id: TerminalId,
    ) -> Option<PendingInputProbe> {
        let queue = self.pending_input_output_probes.get_mut(&terminal_id)?;
        let probe = queue.pop_front();
        if queue.is_empty() {
            self.pending_input_output_probes.remove(&terminal_id);
        }
        probe
    }

    fn discard_pending_input_output_probe(&mut self, probe: &PendingInputProbe) {
        let Some(queue) = self.pending_input_output_probes.get_mut(&probe.terminal_id) else {
            return;
        };
        if let Some(index) = queue
            .iter()
            .position(|candidate| candidate.request_id == probe.request_id)
        {
            queue.remove(index);
        }
        if queue.is_empty() {
            self.pending_input_output_probes.remove(&probe.terminal_id);
        }
    }

    fn start_open_terminal(
        &mut self,
        request_id: u32,
        terminal_id: TerminalId,
        reply: TerminalOpenAckReply,
    ) {
        self.pending_requests
            .insert(request_id, RequestTxn::OpenTerminal { terminal_id, reply });
    }

    fn complete_open_terminal(
        &mut self,
        request_id: u32,
        stream_id: StreamId,
        ack: TerminalOpenAck,
    ) -> Result<(), String> {
        let Some(RequestTxn::OpenTerminal { terminal_id, reply }) =
            self.pending_requests.remove(&request_id)
        else {
            return Err(format!(
                "open terminal response for unknown request {request_id}"
            ));
        };

        if terminal_id != ack.info.terminal {
            return Err("open terminal response terminal mismatch".to_owned());
        }

        self.active_streams.insert(
            stream_id,
            StreamState::Output {
                terminal_id: ack.info.terminal,
                last_seq: ack.end_seq,
            },
        );
        if let Some(active) = self.active.as_mut() {
            if active.stream == stream_id {
                active.last_seq = Some(ack.end_seq);
            }
        }
        if let Some(reply) = reply.borrow_mut().take() {
            let _ = reply.send(Ok(ack));
        }
        Ok(())
    }

    fn record_history(&mut self, request_id: u32, terminal: TerminalId, reply: HistoryReply) {
        self.pending_requests.insert(
            request_id,
            RequestTxn::ReadHistory {
                terminal_id: terminal,
                reply,
            },
        );
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

fn should_retry_after_disconnect(error: &ClientError) -> bool {
    !matches!(error, ClientError::Rejected(_) | ClientError::Closed)
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
    use cli_pocket_proto::{ResponseError, StreamDataFrame};

    fn terminal_info(terminal: TerminalId) -> TerminalInfo {
        TerminalInfo {
            terminal,
            cols: 80,
            rows: 24,
            created_at_unix_ms: 0,
            label: Some("shell".to_owned()),
            attached_clients: 1,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_attach_resolves_empty_snapshot_and_sets_active_stream() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();
        let info = terminal_info(terminal);
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime.start_open_terminal(1, terminal, Rc::new(RefCell::new(Some(reply_tx))));

        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let terminal_handle = Rc::new(RefCell::new(None));
        let mut reached_connected = true;
        let state = ConnectionState {
            events_tx: mpsc::channel(1).0,
            cmd_tx: &cmd_tx,
            terminal: &terminal_handle,
            reached_connected: &mut reached_connected,
        };

        let action = handle_response_frame(
            ResponseFrame {
                id: RequestId(1),
                result: Ok(ResponseBody::OpenTerminal {
                    ack: cli_pocket_proto::OpenTerminalAck {
                        stream_id: StreamId(9),
                        info: info.clone(),
                        start_seq: StreamSeq(10),
                        end_seq: StreamSeq(10),
                        render_bytes: Vec::new().into(),
                        has_more_history: true,
                    },
                }),
            },
            &mut runtime,
            &state,
        )
        .expect("attach response handled");
        assert!(matches!(action, Action::None));

        let ack = reply_rx
            .await
            .expect("reply sender should resolve")
            .expect("attach should succeed");
        assert_eq!(ack.info, info);
        assert_eq!(ack.start_seq, StreamSeq(10));
        assert_eq!(ack.end_seq, StreamSeq(10));
        assert_eq!(ack.stream_id, StreamId(9));
        assert_eq!(ack.render_bytes, Bytes::new());
        assert!(ack.has_more_history);
        assert!(matches!(
            runtime.active_streams.get(&StreamId(9)),
            Some(StreamState::Output {
                terminal_id,
                last_seq,
            }) if *terminal_id == info.terminal && *last_seq == StreamSeq(10)
        ));
        assert_eq!(runtime.active_stream(), Some(StreamId(9)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_output_on_active_stream_is_emitted() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();
        runtime.active_streams.insert(
            StreamId(9),
            StreamState::Output {
                terminal_id: terminal,
                last_seq: StreamSeq(5),
            },
        );
        runtime.set_active(&terminal_info(terminal), StreamId(9), Some(StreamSeq(5)));

        let terminal_handle = Rc::new(RefCell::new(Some(TerminalHandle::new(
            terminal_info(terminal),
            StreamId(9),
            Some(StreamSeq(5)),
            mpsc::channel(1).0,
        ))));
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let mut reached_connected = true;
        let state = ConnectionState {
            events_tx: mpsc::channel(1).0,
            cmd_tx: &cmd_tx,
            terminal: &terminal_handle,
            reached_connected: &mut reached_connected,
        };

        let action = handle_stream_data_frame(
            StreamDataFrame {
                stream_id: StreamId(9),
                seq: StreamSeq(8),
                offset: None,
                bytes: b"live".to_vec().into(),
                last: false,
            },
            &mut runtime,
            &state,
        )
        .expect("active output should emit");
        assert!(matches!(
            action,
            Action::Emit(ClientEvent::TerminalOutput {
                terminal_id,
                stream_seq: StreamSeq(8),
                ref bytes,
            }) if terminal_id == terminal && bytes.as_ref() == b"live"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generic_history_multi_chunk_resolves_page() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime.record_history(7, terminal, Rc::new(RefCell::new(Some(reply_tx))));
        let terminal_handle = Rc::new(RefCell::new(None));
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let mut reached_connected = true;
        let state = ConnectionState {
            events_tx: mpsc::channel(1).0,
            cmd_tx: &cmd_tx,
            terminal: &terminal_handle,
            reached_connected: &mut reached_connected,
        };

        handle_response_frame(
            ResponseFrame {
                id: RequestId(7),
                result: Ok(ResponseBody::ReadHistory {
                    page: cli_pocket_proto::HistoryPage {
                        terminal_id: terminal,
                        start_seq: StreamSeq(100),
                        end_seq: StreamSeq(108),
                        bytes: b"old text".to_vec().into(),
                        has_more: false,
                    },
                }),
            },
            &mut runtime,
            &state,
        )
        .expect("history response handled");

        let page = reply_rx
            .await
            .expect("reply sender should resolve")
            .expect("history should succeed");
        assert_eq!(page.terminal_id, terminal);
        assert_eq!(page.start_seq, StreamSeq(100));
        assert_eq!(page.end_seq, StreamSeq(108));
        assert_eq!(page.bytes, Bytes::from_static(b"old text"));
        assert!(!page.has_more);
    }

    #[test]
    fn generic_error_response_fails_matching_attach_reply() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();
        let (reply_tx, mut reply_rx) = oneshot::channel();
        runtime.start_open_terminal(5, terminal, Rc::new(RefCell::new(Some(reply_tx))));
        let terminal_handle = Rc::new(RefCell::new(None));
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let mut reached_connected = true;
        let state = ConnectionState {
            events_tx: mpsc::channel(1).0,
            cmd_tx: &cmd_tx,
            terminal: &terminal_handle,
            reached_connected: &mut reached_connected,
        };

        handle_response_frame(
            ResponseFrame {
                id: RequestId(5),
                result: Err(ResponseError {
                    code: ProtocolError::UnknownTerminal,
                    message: "unknown terminal".to_owned(),
                }),
            },
            &mut runtime,
            &state,
        )
        .expect("error response handled");

        let reply = reply_rx
            .try_recv()
            .expect("reply should be ready")
            .expect("sender should not be dropped");
        assert!(matches!(reply, Err(ClientError::Proto(message)) if message == "unknown terminal"));
    }

    #[test]
    fn generic_error_response_fails_matching_history_reply() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();
        let (reply_tx, mut reply_rx) = oneshot::channel();
        runtime.record_history(5, terminal, Rc::new(RefCell::new(Some(reply_tx))));
        let terminal_handle = Rc::new(RefCell::new(None));
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let mut reached_connected = true;
        let state = ConnectionState {
            events_tx: mpsc::channel(1).0,
            cmd_tx: &cmd_tx,
            terminal: &terminal_handle,
            reached_connected: &mut reached_connected,
        };

        handle_response_frame(
            ResponseFrame {
                id: RequestId(5),
                result: Err(ResponseError {
                    code: ProtocolError::UnknownTerminal,
                    message: "unknown terminal".to_owned(),
                }),
            },
            &mut runtime,
            &state,
        )
        .expect("error response handled");

        let reply = reply_rx
            .try_recv()
            .expect("reply should be ready")
            .expect("sender should not be dropped");
        assert!(matches!(reply, Err(ClientError::Proto(message)) if message == "unknown terminal"));
    }

    #[test]
    fn generic_output_on_unknown_stream_is_protocol_error() {
        let mut runtime = RuntimeState::default();
        let terminal_handle = Rc::new(RefCell::new(None));
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let mut reached_connected = true;
        let state = ConnectionState {
            events_tx: mpsc::channel(1).0,
            cmd_tx: &cmd_tx,
            terminal: &terminal_handle,
            reached_connected: &mut reached_connected,
        };

        let result = handle_stream_data_frame(
            StreamDataFrame {
                stream_id: StreamId(404),
                seq: StreamSeq(1),
                offset: None,
                bytes: b"x".to_vec().into(),
                last: false,
            },
            &mut runtime,
            &state,
        );
        let Err(error) = result else {
            panic!("unknown output stream should fail");
        };

        assert!(
            matches!(error, ClientError::Proto(message) if message == "stream data on unknown stream")
        );
    }

    #[test]
    fn fail_all_pending_requests_fails_attach_and_history_replies() {
        let mut runtime = RuntimeState::default();
        let terminal = TerminalId::new();
        let (open_tx, mut open_rx) = oneshot::channel();
        let (history_tx, mut history_rx) = oneshot::channel();
        runtime.start_open_terminal(1, terminal, Rc::new(RefCell::new(Some(open_tx))));
        runtime.record_history(2, terminal, Rc::new(RefCell::new(Some(history_tx))));

        fail_all_pending_requests(&mut runtime, &ClientError::Closed);

        let open_reply = open_rx
            .try_recv()
            .expect("open reply should be ready")
            .expect("sender should not be dropped");
        let history_reply = history_rx
            .try_recv()
            .expect("history reply should be ready")
            .expect("sender should not be dropped");

        assert!(matches!(open_reply, Err(ClientError::Closed)));
        assert!(matches!(history_reply, Err(ClientError::Closed)));
        assert!(runtime.pending_requests.is_empty());
        assert!(runtime.active_streams.is_empty());
    }

    #[test]
    fn closed_and_rejected_disconnects_do_not_retry() {
        assert!(!should_retry_after_disconnect(&ClientError::Closed));
        assert!(!should_retry_after_disconnect(&ClientError::Rejected(
            ByeReason::Revoked,
        )));
    }

    #[test]
    fn transport_disconnects_do_retry() {
        assert!(should_retry_after_disconnect(&ClientError::Transport(
            "socket reset".to_owned(),
        )));
    }

    #[test]
    fn coalesce_pending_input_front_merges_adjacent_terminal_input() {
        let terminal = TerminalId::new();
        let mut pending = VecDeque::from([
            TerminalCmd::Input {
                terminal,
                bytes: Bytes::from_static(b"a"),
                queued_at: Instant::now(),
            },
            TerminalCmd::Input {
                terminal,
                bytes: Bytes::from_static(b"b"),
                queued_at: Instant::now(),
            },
            TerminalCmd::Resize {
                terminal,
                cols: 120,
                rows: 40,
            },
        ]);

        coalesce_pending_input_front(&mut pending);

        match pending.pop_front().expect("merged command") {
            TerminalCmd::Input { bytes, .. } => {
                assert_eq!(bytes, Bytes::from_static(b"ab"));
            }
            other => panic!("expected merged input, got {other:?}"),
        }
        assert!(matches!(
            pending.pop_front(),
            Some(TerminalCmd::Resize {
                cols: 120,
                rows: 40,
                ..
            })
        ));
    }

    #[test]
    fn coalesce_pending_input_front_keeps_other_terminal_input_separate() {
        let terminal_a = TerminalId::new();
        let terminal_b = TerminalId::new();
        let mut pending = VecDeque::from([
            TerminalCmd::Input {
                terminal: terminal_a,
                bytes: Bytes::from_static(b"a"),
                queued_at: Instant::now(),
            },
            TerminalCmd::Input {
                terminal: terminal_b,
                bytes: Bytes::from_static(b"b"),
                queued_at: Instant::now(),
            },
        ]);

        coalesce_pending_input_front(&mut pending);

        match pending.pop_front().expect("first command") {
            TerminalCmd::Input {
                terminal, bytes, ..
            } => {
                assert_eq!(terminal, terminal_a);
                assert_eq!(bytes, Bytes::from_static(b"a"));
            }
            other => panic!("expected first input, got {other:?}"),
        }
        match pending.pop_front().expect("second command") {
            TerminalCmd::Input {
                terminal, bytes, ..
            } => {
                assert_eq!(terminal, terminal_b);
                assert_eq!(bytes, Bytes::from_static(b"b"));
            }
            other => panic!("expected second input, got {other:?}"),
        }
    }

    #[test]
    fn input_latency_window_summarizes_full_batch() {
        let mut window = InputLatencyWindow::default();
        for sample in 1..INPUT_PROBE_SUMMARY_SAMPLES {
            assert!(window.record(u64::try_from(sample).unwrap()).is_none());
        }

        let summary = window
            .record(u64::try_from(INPUT_PROBE_SUMMARY_SAMPLES).unwrap())
            .expect("window should summarize once full");

        assert_eq!(summary.count, INPUT_PROBE_SUMMARY_SAMPLES);
        assert_eq!(summary.min_us, 1);
        assert_eq!(summary.p50_us, 16);
        assert_eq!(summary.p95_us, 31);
        assert_eq!(summary.max_us, 32);
        assert_eq!(summary.slow_count, 0);
        assert!(window.samples_us.is_empty());
    }
}
