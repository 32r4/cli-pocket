//! Per-connection state machine: Noise handshake → Hello/HelloOk → frame dispatch loop.
//!
//! The connection task:
//! 1. Runs `responder_handshake` to authenticate the client.
//! 2. Wraps transport + `NoiseSession` into an `EncryptedChannel`.
//! 3. Reads first frame (must be `Hello`), validates, returns `HelloOk`.
//! 4. Loops over inbound frames, dispatching lifecycle and data-plane operations.
//! 5. For each attached terminal stream, spawns a writer task pulling output
//!    via `Terminal::subscribe()`.
//! 6. On revocation watch fire, if client revoked, sends `Bye` and closes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cli_pocket_crypto::NoiseSession;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::HelloOk;
use cli_pocket_proto::{
    ByeReason, Hello, KillSignal, ProtocolError, SessionId, StreamId, StreamSeq,
    TerminalCreateParams, TerminalId, PROTOCOL_VERSION,
};
use cli_pocket_pty::output::{OutputChunk, OutputRecv};
use cli_pocket_pty::Terminal;
use cli_pocket_transport::Transport;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::client_db::{ClientDb, ClientRecord};
use crate::handshake::{responder_handshake, AcceptedHandshake};
use crate::session::SessionManager;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Dependencies needed to run a single client connection.
pub struct ConnectionDeps {
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
    pub server_info: cli_pocket_proto::ServerInfo,
}

/// Run the full per-connection state machine on an already-accepted transport.
///
/// Returns `Ok(())` on clean client disconnect, `Err` on protocol or
/// transport-level failures.
pub async fn run_connection<T: Transport>(
    transport: T,
    deps: ConnectionDeps,
) -> crate::DaemonResult<()> {
    // This function delegates to `run_connection_with_handshake` in practice.
    // The placeholder panics if reached; use `run_connection_with_handshake` instead.
    let _ = (transport, deps);
    accepted_placeholder();
    Ok(())
}

/// Run the connection state machine including the Noise handshake step.
///
/// This is the primary entry point used by the WS listener.
pub async fn run_connection_with_handshake<T: Transport>(
    mut transport: T,
    identity: &cli_pocket_crypto::KeyPair,
    psk: Option<&[u8; 32]>,
    deps: ConnectionDeps,
) -> crate::DaemonResult<()> {
    let AcceptedHandshake { session, client } =
        responder_handshake(&mut transport, identity, psk, &deps.client_db).await?;

    info!(client_id = ?client.client_id, "client authenticated via Noise XK");

    run_connection_post_handshake(transport, deps, AcceptedSession { session, client }).await
}

// ---------------------------------------------------------------------------
// Post-handshake: Hello negotiation + frame loop
// ---------------------------------------------------------------------------

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

    // 2. Wait for Hello.
    let hello_frame = chan.recv_frame().await?;
    let (_client_kind, resumed) = match hello_frame.body {
        FrameBody::Hello(hello) => {
            validate_hello(&hello, &deps)?;
            let resumed = hello.resume.is_some();
            (hello.client_kind, resumed)
        }
        _ => {
            chan.send_frame(&Frame::body(FrameBody::Bye {
                reason: ByeReason::ProtocolError(ProtocolError::ProtocolMismatch),
            }))
            .await?;
            return Err(crate::DaemonError::Internal(
                "expected Hello as first frame".into(),
            ));
        }
    };

    let session_id = SessionId::new();
    debug!(session_id = ?session_id, "sending HelloOk");

    chan.send_frame(&Frame::body(FrameBody::HelloOk(HelloOk {
        protocol: PROTOCOL_VERSION,
        server_info: deps.server_info.clone(),
        session_id,
        resumed,
    })))
    .await?;

    // 3. Main frame loop.
    let mut stream_map: HashMap<StreamId, AttachedStream> = HashMap::new();
    let mut next_local_stream_id = 1u32;

    // Subscribe to revocation updates so we can drop a live session as soon as
    // its `client_id` is revoked. `watch::Receiver::changed()` resolves after
    // any send into the channel, so we re-check `is_revoked` against the
    // current set rather than trusting the changed signal alone.
    let mut rev_rx = deps.client_db.watch_revocations();

    loop {
        let frame = tokio::select! {
            biased;
            res = rev_rx.changed() => {
                // The watch sender is owned by `ClientDb`, which lives as long
                // as `deps`; a recv error here means it was dropped (shutdown).
                if res.is_err() {
                    return Ok(());
                }
                if deps.client_db.is_revoked(&client_id).await {
                    // Best-effort Bye; if the transport is already gone we
                    // still want to tear down cleanly.
                    let _ = chan
                        .send_frame(&Frame::body(FrameBody::Bye {
                            reason: ByeReason::Revoked,
                        }))
                        .await;
                    info!(?client_id, "client revoked; closing live session");
                    return Ok(());
                }
                continue;
            }
            frame = chan.recv_frame() => frame?,
        };
        match frame.body {
            // ---- Lifecycle requests ----
            FrameBody::TerminalCreate { request_id, params } => {
                handle_terminal_create(
                    &mut chan,
                    &deps,
                    &client_id,
                    request_id,
                    params,
                    &mut stream_map,
                    &mut next_local_stream_id,
                )
                .await?;
            }

            FrameBody::TerminalAttach {
                request_id,
                terminal,
                since,
            } => {
                handle_terminal_attach(
                    &mut chan,
                    &deps,
                    request_id,
                    terminal,
                    since,
                    &mut stream_map,
                    &mut next_local_stream_id,
                )
                .await?;
            }

            FrameBody::TerminalDetach { stream } => {
                if let Some(attached) = stream_map.remove(&stream) {
                    debug!(?stream, "terminal detached by client");
                    drop(attached);
                }
            }

            FrameBody::TerminalKill {
                request_id,
                terminal,
            } => {
                let result = deps.session_mgr.kill(&terminal, KillSignal::Term).await;
                let resp = match result {
                    Ok(()) => Frame::body(FrameBody::TerminalKillOk { request_id }),
                    Err(e) => Frame::body(FrameBody::TerminalKillErr {
                        request_id,
                        error: ProtocolError::Other(e.to_string()),
                    }),
                };
                chan.send_frame(&resp).await?;
            }

            FrameBody::TerminalList { request_id } => {
                let terminals = deps.session_mgr.list();
                chan.send_frame(&Frame::body(FrameBody::TerminalListOk {
                    request_id,
                    terminals,
                }))
                .await?;
            }

            // ---- Data plane ----
            FrameBody::Input { stream, bytes } => {
                if let Some(attached) = stream_map.get(&stream) {
                    let _ = attached.terminal.write_input(&bytes);
                }
            }

            FrameBody::Resize { stream, cols, rows } => {
                if let Some(attached) = stream_map.get(&stream) {
                    let _ = attached.terminal.resize(cols, rows);
                }
            }

            FrameBody::Window { stream, credit } => {
                if let Some(attached) = stream_map.get_mut(&stream) {
                    attached.permitted_bytes += credit as usize;
                }
            }

            // ---- Connection control ----
            FrameBody::Ping { nonce } => {
                chan.send_frame(&Frame::body(FrameBody::Pong { nonce }))
                    .await?;
            }

            FrameBody::Bye { reason } => {
                info!(?reason, "client sent Bye");
                return Ok(());
            }

            // Server-side frames from client are unexpected; ignore.
            other => {
                warn!("unexpected server-side frame from client: {other:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Encrypted channel: transport + NoiseSession framing
// ---------------------------------------------------------------------------

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
                crate::DaemonError::Internal("transport closed during frame recv".into())
            })?;
        let plaintext = self
            .session
            .decrypt(&ciphertext)
            .map_err(crate::DaemonError::Crypto)?;
        decode_frame(&plaintext).map_err(crate::DaemonError::Proto)
    }
}

// ---------------------------------------------------------------------------
// Attached stream state
// ---------------------------------------------------------------------------

struct AttachedStream {
    terminal: Arc<Terminal>,
    #[allow(dead_code)]
    terminal_id: TerminalId,
    permitted_bytes: usize,
    _writer: tokio::task::JoinHandle<()>,
}

// ---------------------------------------------------------------------------
// Lifecycle handlers
// ---------------------------------------------------------------------------

async fn handle_terminal_create(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    client_id: &cli_pocket_proto::ClientId,
    request_id: u32,
    params: TerminalCreateParams,
    stream_map: &mut HashMap<StreamId, AttachedStream>,
    next_local_stream_id: &mut u32,
) -> crate::DaemonResult<()> {
    let info = deps.session_mgr.create(params).await?;
    let terminal_id = info.terminal;

    // Create a local stream ID for this terminal's output.
    let stream_id = StreamId(*next_local_stream_id);
    *next_local_stream_id += 1;

    // Subscribe to terminal output and spawn writer.
    let terminal = deps.session_mgr.attach(&terminal_id).ok_or_else(|| {
        crate::DaemonError::Internal("terminal vanished immediately after create".into())
    })?;

    let (out_tx, out_rx) = mpsc::channel::<OutputChunk>(64);
    let writer = spawn_output_writer(
        stream_id,
        chan,
        out_rx,
        deps.session_mgr.clone(),
        terminal_id,
        terminal.clone(),
    );

    stream_map.insert(
        stream_id,
        AttachedStream {
            terminal: terminal.clone(),
            terminal_id,
            permitted_bytes: usize::MAX,
            _writer: writer,
        },
    );

    // Send the response and the initial snapshot as Output.
    let snapshot = terminal.snapshot();
    let head_seq = terminal.head_seq();

    chan.send_frame(&Frame::body(FrameBody::TerminalCreateOk {
        request_id,
        terminal: terminal_id,
        stream: stream_id,
    }))
    .await?;

    // Send initial snapshot as an Output frame so client has the screen state.
    if !snapshot.bytes.is_empty() {
        chan.send_frame(&Frame::body(FrameBody::Output {
            stream: stream_id,
            seq: head_seq,
            bytes: snapshot.bytes,
        }))
        .await?;
    }

    // Kick off the writer task with the output channel.
    spawn_writer_task(stream_id, out_tx, terminal.subscribe(), terminal_id);

    info!(terminal_id = ?terminal_id, stream_id = ?stream_id, client_id = ?client_id, "terminal created");
    Ok(())
}

async fn handle_terminal_attach(
    chan: &mut EncryptedChannel<impl Transport>,
    deps: &ConnectionDeps,
    request_id: u32,
    terminal_id: TerminalId,
    since_seq: Option<StreamSeq>,
    stream_map: &mut HashMap<StreamId, AttachedStream>,
    next_local_stream_id: &mut u32,
) -> crate::DaemonResult<()> {
    let terminal = match deps.session_mgr.attach(&terminal_id) {
        Some(t) => t,
        None => {
            chan.send_frame(&Frame::body(FrameBody::TerminalAttachErr {
                request_id,
                error: ProtocolError::UnknownTerminal,
            }))
            .await?;
            return Ok(());
        }
    };

    let stream_id = StreamId(*next_local_stream_id);
    *next_local_stream_id += 1;

    let snapshot = terminal.snapshot();
    let head_seq = terminal.head_seq();

    // Subscribe for live output after snapshot.
    let (out_tx, out_rx) = mpsc::channel::<OutputChunk>(64);
    let writer = spawn_output_writer(
        stream_id,
        chan,
        out_rx,
        deps.session_mgr.clone(),
        terminal_id,
        terminal.clone(),
    );

    stream_map.insert(
        stream_id,
        AttachedStream {
            terminal: terminal.clone(),
            terminal_id,
            permitted_bytes: usize::MAX,
            _writer: writer,
        },
    );

    chan.send_frame(&Frame::body(FrameBody::TerminalAttachOk {
        request_id,
        snapshot: snapshot.clone(),
        head_seq,
        stream: stream_id,
        initial_window: 65536,
    }))
    .await?;

    // If client requested `since`, send the delta before starting live subscription.
    if let Some(seq) = since_seq {
        if let Some(delta) = terminal.since(seq) {
            chan.send_frame(&Frame::body(FrameBody::Output {
                stream: stream_id,
                seq: delta.head_seq,
                bytes: delta.bytes,
            }))
            .await?;
        }
    }

    spawn_writer_task(stream_id, out_tx, terminal.subscribe(), terminal_id);

    info!(terminal_id = ?terminal_id, stream_id = ?stream_id, "terminal attached");
    Ok(())
}

// ---------------------------------------------------------------------------
// Output writer tasks
// ---------------------------------------------------------------------------

/// Writer task: receives `OutputChunk` from terminal subscribers and
/// forwards them as `FrameBody::Output` through the encrypted channel.
///
/// This task owns the `EncryptedChannel` for writes. The main loop handles
/// reads. This split avoids `NoiseSession` non-Send/non-Sync issues.
fn spawn_output_writer<T: Transport + 'static>(
    stream_id: StreamId,
    chan: &mut EncryptedChannel<T>,
    out_rx: mpsc::Receiver<OutputChunk>,
    _session_mgr: Arc<SessionManager>,
    terminal_id: TerminalId,
    terminal: Arc<Terminal>,
) -> tokio::task::JoinHandle<()> {
    // The full writer-task design (mpsc<Frame> feeding a single task that owns
    // `EncryptedChannel`) graduates from skeleton to full impl in Plan F's
    // client-driven integration. For now, the per-attached-stream JoinHandle is
    // a no-op sentinel: the initial snapshot Output is sent inline by
    // `handle_terminal_create` / `handle_terminal_attach` before they return,
    // and live PTY output is consumed by `spawn_writer_task` below (which reads
    // from `terminal.subscribe()` and drains it via `out_tx` so the channel
    // doesn't fill up). End-to-end TerminalCreateOk roundtrips therefore work;
    // streaming Output past the snapshot is the Plan F integration milestone.
    let _ = (stream_id, chan, out_rx, terminal_id, terminal);
    tokio::spawn(async {})
}

/// Spawn a task that reads OutputChunk from the terminal subscription and
/// sends serialized Output frames to the main loop via mpsc.
fn spawn_writer_task(
    stream_id: StreamId,
    out_tx: mpsc::Sender<OutputChunk>,
    mut subscription: cli_pocket_pty::OutputStream,
    terminal_id: TerminalId,
) -> tokio::task::JoinHandle<()> {
    let _ = (stream_id, terminal_id);
    tokio::spawn(async move {
        loop {
            match subscription.recv().await {
                OutputRecv::Chunk(chunk) => {
                    if out_tx.send(chunk).await.is_err() {
                        break;
                    }
                }
                OutputRecv::Lagged { skipped: _ } => {
                    debug!(stream_id = ?stream_id, "output subscription lagged; attempting catch-up");
                }
                OutputRecv::Closed => {
                    debug!(stream_id = ?stream_id, "output subscription closed");
                    break;
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Hello validation
// ---------------------------------------------------------------------------

fn validate_hello(hello: &Hello, _deps: &ConnectionDeps) -> crate::DaemonResult<()> {
    if hello.protocol_min > PROTOCOL_VERSION || hello.protocol_max < PROTOCOL_VERSION {
        return Err(crate::DaemonError::Internal(format!(
            "protocol version mismatch: client supports {}..{}, server is {}",
            hello.protocol_min, hello.protocol_max, PROTOCOL_VERSION
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX)
}

fn accepted_placeholder() -> AcceptedSession {
    // This should never be called; run_connection delegates to
    // run_connection_with_handshake in practice.
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
        // Should be after 2025-01-01 and before 2030-01-01
        assert!(ms > 1_735_689_600_000u64);
        assert!(ms < 1_893_456_000_000u64);
    }
}
