//! Per-connection state machine: Noise handshake → Hello/HelloOk → frame dispatch loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cli_pocket_crypto::NoiseSession;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::HelloOk;
use cli_pocket_proto::{
    ByeReason, Hello, KillSignal, ProtocolError, ServerConfig, SessionId, StreamId, StreamSeq,
    TerminalBaseline, TerminalCreateParams, TerminalId, PROTOCOL_VERSION,
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
            FrameBody::TerminalCreate { request_id, params } => {
                handle_terminal_create(&mut chan, &deps, request_id, params).await?;
            }
            FrameBody::TerminalAttach {
                request_id,
                terminal,
            } => {
                handle_terminal_attach(&mut chan, &deps, request_id, terminal, &mut streams)
                    .await?;
            }
            FrameBody::TerminalDetach { request_id, stream } => {
                let resp = if streams.map.remove(&stream).is_some() {
                    Frame::body(FrameBody::TerminalDetachOk { request_id })
                } else {
                    Frame::body(FrameBody::TerminalDetachErr {
                        request_id,
                        error: ProtocolError::Other("unknown stream".to_owned()),
                    })
                };
                chan.send_frame(&resp).await?;
            }
            FrameBody::TerminalKill {
                request_id,
                terminal,
            } => {
                let result = deps.session_mgr.kill(&terminal, KillSignal::Term).await;
                let resp = match result {
                    Ok(()) => Frame::body(FrameBody::TerminalKillOk { request_id }),
                    Err(error) => Frame::body(FrameBody::TerminalKillErr {
                        request_id,
                        error: ProtocolError::Other(error.to_string()),
                    }),
                };
                chan.send_frame(&resp).await?;
            }
            FrameBody::TerminalList { request_id } => {
                chan.send_frame(&Frame::body(FrameBody::TerminalListOk {
                    request_id,
                    terminals: deps.session_mgr.list(),
                }))
                .await?;
            }
            FrameBody::ServerConfigGet { request_id } => {
                let resp = match server_config_from_deps(&deps) {
                    Ok(config) => Frame::body(FrameBody::ServerConfigGetOk { request_id, config }),
                    Err(error) => Frame::body(FrameBody::ServerConfigGetErr {
                        request_id,
                        error: ProtocolError::Other(error.to_string()),
                    }),
                };
                chan.send_frame(&resp).await?;
            }
            FrameBody::ServerConfigSet { request_id, config } => {
                let resp = match apply_server_config_update(&deps, &config) {
                    Ok(config) => Frame::body(FrameBody::ServerConfigSetOk { request_id, config }),
                    Err(error) => Frame::body(FrameBody::ServerConfigSetErr {
                        request_id,
                        error: ProtocolError::Other(error.to_string()),
                    }),
                };
                chan.send_frame(&resp).await?;
            }
            FrameBody::HistoryRequest {
                request_id,
                terminal,
                before,
                max_bytes,
            } => {
                handle_history_request(&mut chan, &deps, request_id, terminal, before, max_bytes)
                    .await?;
            }
            FrameBody::Input { stream, bytes } => {
                if let Some(attached) = streams.map.get(&stream) {
                    let _ = attached.terminal.write_input(&bytes);
                }
            }
            FrameBody::Resize { stream, cols, rows } => {
                if let Some(attached) = streams.map.get(&stream) {
                    let _ = attached.terminal.resize(cols, rows);
                }
            }
            FrameBody::Window {
                stream: _,
                credit: _,
            } => {}
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

async fn handle_terminal_create(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: u32,
    params: TerminalCreateParams,
) -> crate::DaemonResult<()> {
    let info = deps
        .session_mgr
        .create(params, current_scrollback_bytes(deps))
        .await?;
    chan.send_frame(&Frame::body(FrameBody::TerminalCreateOk {
        request_id,
        info,
    }))
    .await
}

async fn handle_terminal_attach(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: u32,
    terminal_id: TerminalId,
    streams: &mut StreamContext,
) -> crate::DaemonResult<()> {
    let Some(terminal) = deps.session_mgr.attach(&terminal_id) else {
        chan.send_frame(&Frame::body(FrameBody::TerminalAttachErr {
            request_id,
            error: ProtocolError::UnknownTerminal,
        }))
        .await?;
        return Ok(());
    };

    let stream_id = StreamId(streams.next_local_stream_id);
    streams.next_local_stream_id = streams.next_local_stream_id.saturating_add(1);

    let snapshot = terminal.snapshot();
    let baseline = TerminalBaseline::from(&snapshot);

    chan.send_frame(&Frame::body(FrameBody::TerminalAttachOk {
        request_id,
        baseline: baseline.clone(),
        stream: stream_id,
        initial_window: u32::try_from(SNAPSHOT_CHUNK_BYTES).unwrap_or(u32::MAX),
    }))
    .await?;

    if !snapshot.bytes.is_empty() {
        send_snapshot_chunks(chan, stream_id, baseline.head_seq, snapshot.bytes.as_ref()).await?;
    }

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

async fn send_snapshot_chunks(
    chan: &mut EncryptedChannel<impl Transport>,
    stream: StreamId,
    seq: StreamSeq,
    bytes: &[u8],
) -> crate::DaemonResult<()> {
    let total = bytes.len();
    for (index, chunk) in bytes.chunks(SNAPSHOT_CHUNK_BYTES).enumerate() {
        let offset = index.saturating_mul(SNAPSHOT_CHUNK_BYTES);
        let last = offset + chunk.len() >= total;
        chan.send_frame(&Frame::body(FrameBody::TerminalSnapshotChunk {
            stream,
            seq,
            offset: u32::try_from(offset).map_err(|_| {
                crate::DaemonError::Internal("snapshot offset exceeds u32::MAX".to_owned())
            })?,
            bytes: chunk.to_vec().into(),
            last,
        }))
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
        chan.send_frame(&Frame::body(FrameBody::Output {
            stream,
            seq,
            bytes: piece.to_vec().into(),
        }))
        .await?;
    }

    Ok(())
}

async fn handle_history_request(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: u32,
    terminal_id: TerminalId,
    before: Option<StreamSeq>,
    max_bytes: u32,
) -> crate::DaemonResult<()> {
    let Some(terminal) = deps.session_mgr.attach(&terminal_id) else {
        chan.send_frame(&Frame::body(FrameBody::HistoryErr {
            request_id,
            error: ProtocolError::UnknownTerminal,
        }))
        .await?;
        return Ok(());
    };

    let page = terminal.history_page(before, usize::try_from(max_bytes).unwrap_or(usize::MAX));
    let total = page.bytes.len();

    if total == 0 {
        chan.send_frame(&Frame::body(FrameBody::HistoryChunk {
            request_id,
            terminal: terminal_id,
            start_seq: page.start_seq,
            end_seq: page.end_seq,
            bytes: Vec::new().into(),
            last: true,
        }))
        .await?;
        return Ok(());
    }

    for (index, chunk) in page.bytes.as_ref().chunks(HISTORY_CHUNK_BYTES).enumerate() {
        let offset = index.saturating_mul(HISTORY_CHUNK_BYTES);
        let start_seq = StreamSeq(
            page.start_seq
                .0
                .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX)),
        );
        let end_seq = StreamSeq(
            start_seq
                .0
                .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX)),
        );
        let last = offset + chunk.len() >= total;
        chan.send_frame(&Frame::body(FrameBody::HistoryChunk {
            request_id,
            terminal: terminal_id,
            start_seq,
            end_seq,
            bytes: chunk.to_vec().into(),
            last,
        }))
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
