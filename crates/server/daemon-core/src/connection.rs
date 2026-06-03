//! Per-connection state machine: Noise handshake → Hello/HelloOk → frame dispatch loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cli_pocket_crypto::NoiseSession;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::HelloOk;
use cli_pocket_proto::{
    ByeReason, Hello, KillSignal, ProtocolError, RequestBody, RequestFrame, RequestId,
    ResponseBody, ResponseError, ResponseFrame, ServerConfig, SessionId, StreamDataFrame, StreamId,
    StreamSeq, TerminalBaseline, TerminalId, PROTOCOL_VERSION,
};
use cli_pocket_pty::output::{OutputChunk, OutputRecv};
use cli_pocket_pty::Terminal;
use cli_pocket_transport::Transport;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::client_db::{ClientDb, ClientRecord};
use crate::handshake::{anonymous_responder_handshake, responder_handshake, AcceptedHandshake};
use crate::session::SessionManager;

const SNAPSHOT_CHUNK_BYTES: usize = 16 * 1024;
const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
const HISTORY_CHUNK_BYTES: usize = 16 * 1024;

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
            maybe_output = output_rx.recv() => {
                let Some(output) = maybe_output else {
                    continue;
                };
                if let Some(attached) = streams.map.get(&output.stream) {
                    send_live_output(&mut chan, output.stream, &output.chunk, attached.terminal_id).await?;
                }
                continue;
            }
            frame = chan.recv_frame() => frame?,
        };

        match frame.body {
            FrameBody::Request(request) => {
                handle_request(&mut chan, &deps, request, &mut streams).await?;
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
        decode_frame(&plaintext).map_err(crate::DaemonError::Proto)
    }
}

struct AttachedStream {
    terminal: Arc<Terminal>,
    terminal_id: TerminalId,
    _writer: tokio::task::JoinHandle<()>,
}

struct OutboundOutput {
    stream: StreamId,
    chunk: OutputChunk,
}

struct StreamContext {
    map: HashMap<StreamId, AttachedStream>,
    next_local_stream_id: u32,
    output_tx: mpsc::Sender<OutboundOutput>,
}

async fn handle_request(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request: RequestFrame,
    streams: &mut StreamContext,
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
        RequestBody::AttachTerminal { terminal_id } => {
            handle_attach_request(chan, deps, request.id, terminal_id, streams).await
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
                let _ = attached.terminal.write_input(&bytes);
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

async fn handle_attach_request(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: RequestId,
    terminal_id: TerminalId,
    streams: &mut StreamContext,
) -> crate::DaemonResult<()> {
    let Some(terminal) = deps.session_mgr.attach(&terminal_id) else {
        send_error_response(chan, request_id, ProtocolError::UnknownTerminal).await?;
        return Ok(());
    };

    streams.map.clear();

    let stream_id = StreamId(streams.next_local_stream_id);
    streams.next_local_stream_id = streams.next_local_stream_id.saturating_add(1);

    let snapshot = terminal.snapshot();
    let baseline = TerminalBaseline::from(&snapshot);
    let Some(info) = deps
        .session_mgr
        .list()
        .into_iter()
        .find(|info| info.terminal == terminal_id)
    else {
        send_error_response(chan, request_id, ProtocolError::UnknownTerminal).await?;
        return Ok(());
    };

    let start_seq = StreamSeq(
        baseline
            .head_seq
            .0
            .saturating_sub(u64::try_from(snapshot.bytes.len()).unwrap_or(u64::MAX)),
    );
    send_ok_response(
        chan,
        request_id,
        ResponseBody::AttachTerminal {
            stream_id,
            terminal_info: info,
            baseline_start_seq: start_seq,
            baseline_end_seq: baseline.head_seq,
            render_prefix: String::new(),
        },
    )
    .await?;

    send_stream_chunks(
        chan,
        stream_id,
        baseline.head_seq,
        Some(0),
        snapshot.bytes.as_ref(),
        SNAPSHOT_CHUNK_BYTES,
        false,
    )
    .await?;

    let writer = spawn_output_forwarder(stream_id, terminal.subscribe(), streams.output_tx.clone());
    streams.map.insert(
        stream_id,
        AttachedStream {
            terminal,
            terminal_id,
            _writer: writer,
        },
    );

    info!(terminal_id = ?terminal_id, stream_id = ?stream_id, "terminal attached");
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
    let Some(terminal) = deps.session_mgr.attach(&terminal_id) else {
        send_error_response(chan, request_id, ProtocolError::UnknownTerminal).await?;
        return Ok(());
    };

    let stream_id = StreamId(request_id.0);
    let page = terminal.history_page(before, usize::try_from(max_bytes).unwrap_or(usize::MAX));

    send_ok_response(
        chan,
        request_id,
        ResponseBody::ReadHistory {
            stream_id,
            terminal_id,
            start_seq: page.start_seq,
            end_seq: page.end_seq,
        },
    )
    .await?;

    send_stream_chunks(
        chan,
        stream_id,
        page.start_seq,
        Some(0),
        page.bytes.as_ref(),
        HISTORY_CHUNK_BYTES,
        true,
    )
    .await
}

async fn send_stream_chunks(
    chan: &mut EncryptedChannel<impl Transport>,
    stream_id: StreamId,
    seq: StreamSeq,
    first_offset: Option<u32>,
    bytes: &[u8],
    chunk_size: usize,
    advance_seq_per_chunk: bool,
) -> crate::DaemonResult<()> {
    if bytes.is_empty() {
        chan.send_frame(&Frame::body(FrameBody::StreamData(StreamDataFrame {
            stream_id,
            seq,
            offset: first_offset,
            bytes: Vec::new().into(),
            last: true,
        })))
        .await?;
        return Ok(());
    }

    for (index, chunk) in bytes.chunks(chunk_size).enumerate() {
        let offset = index.saturating_mul(chunk_size);
        let last = offset + chunk.len() >= bytes.len();
        let chunk_seq = if advance_seq_per_chunk {
            StreamSeq(
                seq.0
                    .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX)),
            )
        } else {
            seq
        };
        chan.send_frame(&Frame::body(FrameBody::StreamData(StreamDataFrame {
            stream_id,
            seq: chunk_seq,
            offset: first_offset
                .map(|base| base.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX))),
            bytes: chunk.to_vec().into(),
            last,
        })))
        .await?;
    }

    Ok(())
}

async fn send_live_output(
    chan: &mut EncryptedChannel<impl Transport>,
    stream: StreamId,
    chunk: &OutputChunk,
    terminal_id: TerminalId,
) -> crate::DaemonResult<()> {
    let total = chunk.bytes.len();
    if total == 0 {
        return Ok(());
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
    for piece in chunk.bytes.as_ref().chunks(OUTPUT_CHUNK_BYTES) {
        sent = sent.saturating_add(piece.len());
        let seq = StreamSeq(start_seq.saturating_add(u64::try_from(sent).unwrap_or(u64::MAX)));
        chan.send_frame(&Frame::body(FrameBody::StreamData(StreamDataFrame {
            stream_id: stream,
            seq,
            offset: None,
            bytes: piece.to_vec().into(),
            last: false,
        })))
        .await?;
    }

    Ok(())
}

fn spawn_output_forwarder(
    stream_id: StreamId,
    mut subscription: cli_pocket_pty::OutputStream,
    output_tx: mpsc::Sender<OutboundOutput>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match subscription.recv().await {
                OutputRecv::Chunk(chunk) => {
                    if output_tx
                        .send(OutboundOutput {
                            stream: stream_id,
                            chunk,
                        })
                        .await
                        .is_err()
                    {
                        break;
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
}
