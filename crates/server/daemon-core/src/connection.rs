//! Per-connection state machine: Noise handshake → Hello/HelloOk → frame dispatch loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use cli_pocket_crypto::NoiseSession;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::HelloOk;
use cli_pocket_proto::{
    ByeReason, Hello, KillSignal, ProtocolError, RequestBody, RequestFrame, RequestId,
    ResponseBody, ResponseError, ResponseFrame, ServerConfig, SessionId, StreamDataFrame, StreamId,
    StreamSeq, TerminalId, PROTOCOL_VERSION,
};
use cli_pocket_pty::output::{OutputChunk, OutputRecv};
use cli_pocket_pty::{OutputStream, Terminal};
use cli_pocket_transport::Transport;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};

use crate::client_db::{ClientDb, ClientRecord};
use crate::handshake::{anonymous_responder_handshake, responder_handshake, AcceptedHandshake};
use crate::session::SessionManager;

const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
const OPEN_TERMINAL_MAX_BYTES: usize = 48 * 1024;
const INPUT_PROBE_SUMMARY_SAMPLES: usize = 32;
const DAEMON_INPUT_PROBE_SLOW_US: u64 = 5_000;

fn input_probe_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("CLI_POCKET_INPUT_PROBE")
            .is_ok_and(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
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
            slow_threshold_us: DAEMON_INPUT_PROBE_SLOW_US,
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
struct DaemonInputProbeStats {
    pty_write: InputLatencyWindow,
}

impl DaemonInputProbeStats {
    fn record_sample(
        window: &mut InputLatencyWindow,
        probe: &PendingInputProbe,
        metric: &'static str,
        slow_message: &'static str,
        sample_us: u64,
    ) {
        if sample_us >= DAEMON_INPUT_PROBE_SLOW_US {
            info!(
                target: "cli_pocket_probe::input_latency",
                component = "daemon",
                metric,
                terminal_id = ?probe.terminal_id,
                request_id = probe.request_id,
                bytes_len = probe.bytes_len,
                delay_us = sample_us,
                slow_threshold_us = DAEMON_INPUT_PROBE_SLOW_US,
                "{slow_message}"
            );
        }
        if let Some(summary) = window.record(sample_us) {
            info!(
                target: "cli_pocket_probe::input_latency",
                component = "daemon",
                metric,
                samples = summary.count,
                min_us = summary.min_us,
                p50_us = summary.p50_us,
                p95_us = summary.p95_us,
                max_us = summary.max_us,
                slow_threshold_us = DAEMON_INPUT_PROBE_SLOW_US,
                slow_samples = summary.slow_count,
                "daemon input latency summary"
            );
        }
    }

    fn record_pty_write(&mut self, probe: &PendingInputProbe, pty_write_us: u64) {
        Self::record_sample(
            &mut self.pty_write,
            probe,
            "pty_write_us",
            "daemon send_input pty write slow",
            pty_write_us,
        );
    }
}

pub struct ConnectionDeps {
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
    pub server_info: cli_pocket_proto::ServerInfo,
    pub config: Arc<Mutex<crate::config::DaemonConfig>>,
}

#[derive(Clone, Copy)]
pub enum HandshakeKind<'a> {
    Direct {
        auto_pair: bool,
    },
    Relay {
        psk: Option<&'a [u8; 32]>,
        auto_pair: bool,
    },
}

pub async fn run_connection<T: Transport>(
    transport: T,
    deps: ConnectionDeps,
) -> crate::DaemonResult<()> {
    let _ = (transport, deps);
    accepted_placeholder();
    Ok(())
}

pub async fn run_connection_with_handshake<T: Transport>(
    mut transport: T,
    identity: &cli_pocket_crypto::KeyPair,
    handshake: HandshakeKind<'_>,
    deps: ConnectionDeps,
) -> crate::DaemonResult<()> {
    let AcceptedHandshake { session, client } = match handshake {
        HandshakeKind::Direct { auto_pair } => {
            anonymous_responder_handshake(&mut transport, identity, &deps.client_db, auto_pair)
                .await?
        }
        HandshakeKind::Relay { psk, auto_pair } => {
            responder_handshake(&mut transport, identity, psk, &deps.client_db, auto_pair).await?
        }
    };

    info!(client_id = ?client.client_id, "client authenticated via Noise");
    run_connection_post_handshake(transport, deps, AcceptedSession { session, client }).await
}

struct AcceptedSession {
    session: NoiseSession,
    client: ClientRecord,
}

async fn run_connection_post_handshake<T: Transport>(
    transport: T,
    deps: ConnectionDeps,
    accepted: AcceptedSession,
) -> crate::DaemonResult<()> {
    let client_id = accepted.client.client_id;
    let mut chan = EncryptedChannel::new(transport, accepted.session);

    let hello_frame = chan.recv_frame().await?;
    let resumed = match hello_frame.body {
        FrameBody::Hello(hello) => {
            validate_hello(&hello, &deps)?;
            hello.resume.is_some()
        }
        _ => {
            chan.send_frame(&Frame::body(FrameBody::Bye {
                reason: ByeReason::ProtocolError(ProtocolError::ProtocolMismatch),
            }))
            .await?;
            return Err(crate::DaemonError::Internal(
                "expected Hello as first frame".to_owned(),
            ));
        }
    };

    let session_id = SessionId::new();
    chan.send_frame(&Frame::body(FrameBody::HelloOk(HelloOk {
        protocol: PROTOCOL_VERSION,
        server_info: deps.server_info.clone(),
        session_id,
        resumed,
    })))
    .await?;

    let (output_tx, mut output_rx) = mpsc::channel::<OutboundOutput>(64);
    let mut streams = StreamContext {
        map: HashMap::new(),
        next_local_stream_id: 1,
        output_tx,
    };
    let mut input_probe_stats = DaemonInputProbeStats::default();
    let mut rev_rx = deps.client_db.watch_revocations();

    loop {
        let frame = tokio::select! {
            biased;
            res = rev_rx.changed() => {
                if res.is_err() {
                    return Ok(());
                }
                if deps.client_db.is_revoked(&client_id).await {
                    let _ = chan.send_frame(&Frame::body(FrameBody::Bye {
                        reason: ByeReason::Revoked,
                    })).await;
                    info!(?client_id, "client revoked; closing live session");
                    return Ok(());
                }
                continue;
            }
            frame = chan.recv_frame() => frame?,
            maybe_output = output_rx.recv() => {
                let Some(output) = maybe_output else {
                    continue;
                };
                if let Some(attached) = streams.map.get(&output.stream) {
                    if attached.terminal_id == output.terminal_id {
                        chan.send_frame(&Frame::body(FrameBody::StreamData(output.frame)))
                            .await?;
                    }
                }
                continue;
            }
        };

        match frame.body {
            FrameBody::Request(request) => {
                handle_request(
                    &mut chan,
                    &deps,
                    request,
                    &mut streams,
                    &mut input_probe_stats,
                )
                .await?;
            }
            FrameBody::Ping { nonce } => {
                chan.send_frame(&Frame::body(FrameBody::Pong { nonce }))
                    .await?;
            }
            FrameBody::Bye { reason } => {
                info!(?reason, "client sent Bye");
                return Ok(());
            }
            other => {
                warn!("unexpected server-side frame from client: {other:?}");
            }
        }
    }
}

struct EncryptedChannel<T> {
    transport: T,
    session: NoiseSession,
}

impl<T: Transport> EncryptedChannel<T> {
    fn new(transport: T, session: NoiseSession) -> Self {
        Self { transport, session }
    }

    async fn send_frame(&mut self, frame: &Frame) -> crate::DaemonResult<()> {
        trace!(direction = "send", ?frame, "daemon wire frame");
        let plaintext = encode_frame(frame).map_err(crate::DaemonError::Proto)?;
        let ciphertext = self
            .session
            .encrypt(&plaintext)
            .map_err(crate::DaemonError::Crypto)?;
        self.transport
            .send(ciphertext)
            .await
            .map_err(crate::DaemonError::Transport)
    }

    async fn recv_frame(&mut self) -> crate::DaemonResult<Frame> {
        let ciphertext = self
            .transport
            .recv()
            .await
            .map_err(crate::DaemonError::Transport)?
            .ok_or_else(|| {
                crate::DaemonError::Internal("transport closed during frame recv".to_owned())
            })?;
        let plaintext = self
            .session
            .decrypt(&ciphertext)
            .map_err(crate::DaemonError::Crypto)?;
        let frame = decode_frame(&plaintext).map_err(crate::DaemonError::Proto)?;
        trace!(direction = "recv", ?frame, "daemon wire frame");
        Ok(frame)
    }
}

struct AttachedStream {
    terminal: Arc<Terminal>,
    terminal_id: TerminalId,
    writer: tokio::task::JoinHandle<()>,
}

struct OutboundOutput {
    stream: StreamId,
    terminal_id: TerminalId,
    frame: StreamDataFrame,
}

struct StreamContext {
    map: HashMap<StreamId, AttachedStream>,
    next_local_stream_id: u32,
    output_tx: mpsc::Sender<OutboundOutput>,
}

#[derive(Debug, Clone)]
struct PendingInputProbe {
    request_id: u32,
    terminal_id: TerminalId,
    bytes_len: usize,
}

async fn handle_request(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request: RequestFrame,
    streams: &mut StreamContext,
    input_probe_stats: &mut DaemonInputProbeStats,
) -> crate::DaemonResult<()> {
    match request.body {
        RequestBody::ListTerminals => {
            send_ok_response(
                chan,
                request.id,
                ResponseBody::ListTerminals {
                    terminals: deps.session_mgr.list(),
                },
            )
            .await
        }
        RequestBody::CreateTerminal { params } => {
            match deps
                .session_mgr
                .create(params, current_scrollback_bytes(deps))
                .await
            {
                Ok(info) => {
                    send_ok_response(chan, request.id, ResponseBody::CreateTerminal { info }).await
                }
                Err(error) => {
                    send_error_response(chan, request.id, ProtocolError::Other(error.to_string()))
                        .await
                }
            }
        }
        RequestBody::OpenTerminal { terminal_id } => {
            handle_open_terminal_request(chan, deps, request.id, terminal_id, streams).await
        }
        RequestBody::ReadHistory {
            terminal_id,
            before,
            max_bytes,
        } => {
            handle_history_read_request(chan, deps, request.id, terminal_id, before, max_bytes)
                .await
        }
        RequestBody::KillTerminal { terminal_id } => {
            match deps.session_mgr.kill(&terminal_id, KillSignal::Term).await {
                Ok(()) => send_ok_response(chan, request.id, ResponseBody::KillTerminal).await,
                Err(error) => {
                    send_error_response(chan, request.id, ProtocolError::Other(error.to_string()))
                        .await
                }
            }
        }
        RequestBody::GetServerConfig => match server_config_from_deps(deps) {
            Ok(config) => {
                send_ok_response(chan, request.id, ResponseBody::GetServerConfig { config }).await
            }
            Err(error) => {
                send_error_response(chan, request.id, ProtocolError::Other(error.to_string())).await
            }
        },
        RequestBody::SetServerConfig { config } => {
            match apply_server_config_update(deps, &config) {
                Ok(config) => {
                    send_ok_response(chan, request.id, ResponseBody::SetServerConfig { config })
                        .await
                }
                Err(error) => {
                    send_error_response(chan, request.id, ProtocolError::Other(error.to_string()))
                        .await
                }
            }
        }
        RequestBody::SendInput { terminal_id, bytes } => {
            if let Some(attached) = streams
                .map
                .values()
                .find(|attached| attached.terminal_id == terminal_id)
            {
                let probe = PendingInputProbe {
                    request_id: request.id.0,
                    terminal_id,
                    bytes_len: bytes.len(),
                };
                let write_started_at = Instant::now();
                let _ = attached.terminal.write_input(&bytes);
                let pty_write_us = elapsed_micros_u64(write_started_at);
                if input_probe_enabled() {
                    input_probe_stats.record_pty_write(&probe, pty_write_us);
                }
                send_ok_response(chan, request.id, ResponseBody::SendInput).await
            } else {
                send_error_response(chan, request.id, ProtocolError::UnknownTerminal).await
            }
        }
        RequestBody::ResizeTerminal {
            terminal_id,
            cols,
            rows,
        } => {
            if let Some(attached) = streams
                .map
                .values()
                .find(|attached| attached.terminal_id == terminal_id)
            {
                let _ = attached.terminal.resize(cols, rows);
                send_ok_response(chan, request.id, ResponseBody::ResizeTerminal).await
            } else {
                send_error_response(chan, request.id, ProtocolError::UnknownTerminal).await
            }
        }
    }
}

async fn send_ok_response(
    chan: &mut EncryptedChannel<impl Transport>,
    id: RequestId,
    body: ResponseBody,
) -> crate::DaemonResult<()> {
    chan.send_frame(&Frame::body(FrameBody::Response(ResponseFrame {
        id,
        result: Ok(body),
    })))
    .await
}

async fn send_error_response(
    chan: &mut EncryptedChannel<impl Transport>,
    id: RequestId,
    code: ProtocolError,
) -> crate::DaemonResult<()> {
    let message = code.to_string();
    chan.send_frame(&Frame::body(FrameBody::Response(ResponseFrame {
        id,
        result: Err(ResponseError { code, message }),
    })))
    .await
}

async fn handle_open_terminal_request(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: RequestId,
    terminal_id: TerminalId,
    streams: &mut StreamContext,
) -> crate::DaemonResult<()> {
    info!(terminal_id = ?terminal_id, request_id = ?request_id, "open terminal requested");
    let Some(terminal) = deps.session_mgr.attach(&terminal_id) else {
        send_error_response(chan, request_id, ProtocolError::UnknownTerminal).await?;
        return Ok(());
    };

    clear_attached_streams(streams);

    let stream_id = StreamId(streams.next_local_stream_id);
    streams.next_local_stream_id = streams.next_local_stream_id.saturating_add(1);

    let subscription = terminal.subscribe();
    // Keep the inline open payload comfortably below Noise's single-message
    // limit. Older history remains available through ReadHistory.
    let snapshot = terminal.snapshot_with_max_bytes(OPEN_TERMINAL_MAX_BYTES);
    let start_seq = StreamSeq(
        snapshot
            .head_seq
            .0
            .saturating_sub(u64::try_from(snapshot.bytes.len()).unwrap_or(u64::MAX)),
    );
    let mut render_bytes =
        cli_pocket_proto::render_prefix_from_anchor(&snapshot.anchor_state).into_bytes();
    render_bytes.extend_from_slice(snapshot.bytes.as_ref());
    let Some(info) = deps
        .session_mgr
        .list()
        .into_iter()
        .find(|info| info.terminal == terminal_id)
    else {
        send_error_response(chan, request_id, ProtocolError::UnknownTerminal).await?;
        return Ok(());
    };

    send_ok_response(
        chan,
        request_id,
        ResponseBody::OpenTerminal {
            ack: cli_pocket_proto::OpenTerminalAck {
                stream_id,
                info,
                start_seq,
                end_seq: snapshot.head_seq,
                render_bytes: render_bytes.into(),
                has_more_history: terminal.history_page(Some(start_seq), 1).has_more,
            },
        },
    )
    .await?;
    info!(
        terminal_id = ?terminal_id,
        stream_id = ?stream_id,
        start_seq = ?start_seq,
        end_seq = ?snapshot.head_seq,
        "open terminal ack sent"
    );

    let writer = spawn_attached_stream_forwarder(
        stream_id,
        terminal_id,
        subscription,
        streams.output_tx.clone(),
    );
    streams.map.insert(
        stream_id,
        AttachedStream {
            terminal,
            terminal_id,
            writer,
        },
    );

    info!(terminal_id = ?terminal_id, stream_id = ?stream_id, "terminal opened");
    Ok(())
}

async fn handle_history_read_request(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: RequestId,
    terminal_id: TerminalId,
    before: Option<StreamSeq>,
    max_bytes: u32,
) -> crate::DaemonResult<()> {
    info!(
        terminal_id = ?terminal_id,
        request_id = ?request_id,
        before = ?before,
        max_bytes,
        "history requested"
    );
    let Some(terminal) = deps.session_mgr.attach(&terminal_id) else {
        send_error_response(chan, request_id, ProtocolError::UnknownTerminal).await?;
        return Ok(());
    };

    let page = terminal.history_page(before, usize::try_from(max_bytes).unwrap_or(usize::MAX));
    let page_bytes_len = page.bytes.len();

    send_ok_response(
        chan,
        request_id,
        ResponseBody::ReadHistory {
            page: cli_pocket_proto::HistoryPage {
                terminal_id,
                start_seq: page.start_seq,
                end_seq: page.end_seq,
                bytes: page.bytes,
                has_more: page.has_more,
            },
        },
    )
    .await?;
    info!(
        terminal_id = ?terminal_id,
        start_seq = ?page.start_seq,
        end_seq = ?page.end_seq,
        bytes = page_bytes_len,
        "history ack sent"
    );
    Ok(())
}

fn live_output_frames(
    stream: StreamId,
    terminal_id: TerminalId,
    chunk: &OutputChunk,
) -> crate::DaemonResult<Vec<OutboundOutput>> {
    let total = chunk.bytes.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    let start_seq = chunk
        .seq_at_end
        .0
        .checked_sub(u64::try_from(total).unwrap_or(u64::MAX))
        .ok_or_else(|| {
            crate::DaemonError::Internal(format!(
                "output sequence underflow for terminal {terminal_id:?}"
            ))
        })?;

    let mut sent = 0usize;
    Ok(chunk
        .bytes
        .as_ref()
        .chunks(OUTPUT_CHUNK_BYTES)
        .map(|piece| {
            sent = sent.saturating_add(piece.len());
            let seq = StreamSeq(start_seq.saturating_add(u64::try_from(sent).unwrap_or(u64::MAX)));
            OutboundOutput {
                stream,
                terminal_id,
                frame: StreamDataFrame {
                    stream_id: stream,
                    seq,
                    offset: None,
                    bytes: piece.to_vec().into(),
                    last: false,
                },
            }
        })
        .collect())
}

fn spawn_attached_stream_forwarder(
    stream_id: StreamId,
    terminal_id: TerminalId,
    mut subscription: OutputStream,
    output_tx: mpsc::Sender<OutboundOutput>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match subscription.recv().await {
                OutputRecv::Chunk(chunk) => {
                    let frames = match live_output_frames(stream_id, terminal_id, &chunk) {
                        Ok(frames) => frames,
                        Err(error) => {
                            warn!("{error}");
                            break;
                        }
                    };
                    debug!(
                        stream_id = ?stream_id,
                        terminal_id = ?terminal_id,
                        frames = frames.len(),
                        "forwarding live output"
                    );
                    for frame in frames {
                        if output_tx.send(frame).await.is_err() {
                            return;
                        }
                    }
                }
                OutputRecv::Lagged { skipped } => {
                    debug!(stream_id = ?stream_id, skipped, "output subscription lagged");
                }
                OutputRecv::Closed => {
                    debug!(stream_id = ?stream_id, "output subscription closed");
                    break;
                }
            }
        }
    })
}

fn clear_attached_streams(streams: &mut StreamContext) {
    for attached in streams.map.drain().map(|(_, attached)| attached) {
        attached.writer.abort();
    }
}

fn current_scrollback_bytes(deps: &ConnectionDeps) -> usize {
    deps.config.lock().limits.scrollback_bytes
}

fn server_config_from_deps(deps: &ConnectionDeps) -> crate::DaemonResult<ServerConfig> {
    Ok(ServerConfig {
        scrollback_bytes: u32::try_from(current_scrollback_bytes(deps)).map_err(|_| {
            crate::DaemonError::Config("limits.scrollback_bytes exceeds u32::MAX".to_owned())
        })?,
    })
}

fn apply_server_config_update(
    deps: &ConnectionDeps,
    config: &ServerConfig,
) -> crate::DaemonResult<ServerConfig> {
    let scrollback_bytes = usize::try_from(config.scrollback_bytes).map_err(|_| {
        crate::DaemonError::Config("scrollback_bytes exceeds usize::MAX".to_owned())
    })?;
    let updated = {
        let current = deps.config.lock();
        let mut next = current.clone();
        next.limits.scrollback_bytes = scrollback_bytes;
        next
    };
    if let Some(path) = updated.config_path.clone() {
        updated.save_to(&path)?;
    }
    *deps.config.lock() = updated;
    Ok(config.clone())
}

fn validate_hello(hello: &Hello, _deps: &ConnectionDeps) -> crate::DaemonResult<()> {
    if hello.protocol_min > PROTOCOL_VERSION || hello.protocol_max < PROTOCOL_VERSION {
        return Err(crate::DaemonError::Internal(format!(
            "protocol version mismatch: client supports {}..{}, server is {}",
            hello.protocol_min, hello.protocol_max, PROTOCOL_VERSION
        )));
    }
    Ok(())
}

#[allow(dead_code)]
fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(u64::MAX, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn accepted_placeholder() -> AcceptedSession {
    panic!("run_connection should not be called directly; use run_connection_with_handshake")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_constant_is_one() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn now_ms_returns_reasonable_value() {
        let ms = now_unix_ms();
        assert!(ms > 1_735_689_600_000u64);
        assert!(ms < 1_893_456_000_000u64);
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
