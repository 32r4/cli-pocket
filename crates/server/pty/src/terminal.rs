use crate::output::{OutputBroadcaster, OutputChunk, OutputStream};
use crate::platform;
use crate::ring::{RingError, ScrollbackRing};
use bytes::Bytes;
use cli_pocket_proto::{
    DeltaSlice, ExitInfo, KillSignal, Snapshot, StreamSeq, TerminalCreateParams, TerminalId,
};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtyPair, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch;

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("ring error: {0}")]
    Ring(#[from] RingError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("portable-pty error: {0}")]
    Pty(String),
    #[error("terminal already exited")]
    Exited,
    #[error("command argv cannot be empty")]
    InvalidCommand,
}

pub struct Terminal {
    id: TerminalId,
    inner: Arc<TerminalInner>,
}

struct TerminalInner {
    ring: Mutex<ScrollbackRing>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    broadcaster: Arc<OutputBroadcaster>,
    exit_rx: Mutex<watch::Receiver<Option<ExitInfo>>>,
    exited: AtomicBool,
}

impl Terminal {
    pub fn spawn(params: &TerminalCreateParams) -> Result<Self, TerminalError> {
        let cols = params.cols.max(1);
        let rows = params.rows.max(1);
        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pty_system = portable_pty::native_pty_system();
        let PtyPair { master, slave } = pty_system
            .openpty(pty_size)
            .map_err(|error| TerminalError::Pty(error.to_string()))?;

        let command = build_command(params)?;
        let child = slave
            .spawn_command(command)
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        drop(slave);

        let killer = child.clone_killer();
        let writer = master
            .take_writer()
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        let reader = master
            .try_clone_reader()
            .map_err(|error| TerminalError::Pty(error.to_string()))?;

        let ring = ScrollbackRing::new(
            cols,
            rows,
            params.scrollback_bytes.map(|value| value as usize),
        )?;
        let broadcaster = Arc::new(OutputBroadcaster::new());
        let (exit_tx, exit_rx) = watch::channel(None);

        let inner = Arc::new(TerminalInner {
            ring: Mutex::new(ring),
            master: Mutex::new(master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            broadcaster: Arc::clone(&broadcaster),
            exit_rx: Mutex::new(exit_rx),
            exited: AtomicBool::new(false),
        });

        spawn_reader(Arc::clone(&inner), reader, broadcaster);
        spawn_waiter(Arc::clone(&inner), child, exit_tx);

        Ok(Self {
            id: TerminalId::new(),
            inner,
        })
    }

    #[must_use]
    pub fn id(&self) -> TerminalId {
        self.id
    }

    #[must_use]
    pub fn dims(&self) -> (u16, u16) {
        let ring = lock_or_recover(&self.inner.ring, "ring");
        ring.dims()
    }

    #[must_use]
    pub fn head_seq(&self) -> StreamSeq {
        let ring = lock_or_recover(&self.inner.ring, "ring");
        ring.head_seq()
    }

    pub fn write_input(&self, bytes: &[u8]) -> Result<(), TerminalError> {
        if self.inner.exited.load(Ordering::Acquire) {
            return Err(TerminalError::Exited);
        }

        let mut writer = lock_or_recover(&self.inner.writer, "writer");
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    #[must_use]
    pub fn subscribe(&self) -> OutputStream {
        self.inner.broadcaster.subscribe()
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let ring = lock_or_recover(&self.inner.ring, "ring");
        ring.snapshot()
    }

    #[must_use]
    pub fn since(&self, seq: StreamSeq) -> Option<DeltaSlice> {
        let ring = lock_or_recover(&self.inner.ring, "ring");
        ring.since(seq)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        if self.inner.exited.load(Ordering::Acquire) {
            return Err(TerminalError::Exited);
        }

        let pty_size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };

        {
            let master = lock_or_recover(&self.inner.master, "master");
            master
                .resize(pty_size)
                .map_err(|error| TerminalError::Pty(error.to_string()))?;
        }

        let mut ring = lock_or_recover(&self.inner.ring, "ring");
        ring.set_dims(pty_size.cols, pty_size.rows);
        Ok(())
    }

    pub fn kill(&self, _signal: KillSignal) -> Result<(), TerminalError> {
        if self.inner.exited.load(Ordering::Acquire) {
            return Ok(());
        }

        let kill_result = {
            let mut killer = lock_or_recover(&self.inner.killer, "killer");
            killer.kill()
        };

        match kill_result {
            Ok(()) => Ok(()),
            Err(_error) if self.inner.exited.load(Ordering::Acquire) => Ok(()),
            Err(error) => map_kill_error(&error, self.wait_briefly_for_exit()),
        }
    }

    /// Check whether the PTY child process has exited.
    ///
    /// This is a non-blocking snapshot of the exited flag set by the
    /// internal waiter thread.
    #[must_use]
    pub fn is_exited(&self) -> bool {
        self.inner.exited.load(Ordering::Acquire)
    }

    pub async fn wait(&self) -> ExitInfo {
        if let Some(exit) = self.current_exit() {
            return exit;
        }

        let mut receiver = {
            let receiver = lock_or_recover(&self.inner.exit_rx, "exit receiver");
            receiver.clone()
        };

        loop {
            if let Some(exit) = receiver.borrow().clone() {
                return exit;
            }

            if receiver.changed().await.is_err() {
                return self.current_exit().unwrap_or_else(default_exit_info);
            }
        }
    }

    fn current_exit(&self) -> Option<ExitInfo> {
        let receiver = lock_or_recover(&self.inner.exit_rx, "exit receiver");
        let current = receiver.borrow().clone();
        current
    }

    fn wait_briefly_for_exit(&self) -> bool {
        for _ in 0..10 {
            if self.inner.exited.load(Ordering::Acquire) || self.current_exit().is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        false
    }
}

fn map_kill_error(error: &std::io::Error, exited_after_error: bool) -> Result<(), TerminalError> {
    if exited_after_error {
        tracing::debug!("pty kill reported an error after process exit: {error}");
        Ok(())
    } else {
        Err(TerminalError::Pty(format!(
            "failed to kill PTY child: {error}"
        )))
    }
}

fn build_command(params: &TerminalCreateParams) -> Result<CommandBuilder, TerminalError> {
    let argv = if params.cmd.is_empty() {
        platform::default_shell()
    } else {
        params.cmd.clone()
    };
    let Some(program) = argv.first() else {
        return Err(TerminalError::InvalidCommand);
    };

    let mut command = CommandBuilder::new(program);
    command.args(argv.iter().skip(1));

    if let Some(cwd) = &params.cwd {
        command.cwd(cwd);
    }

    for (key, value) in &params.env {
        command.env(key, value);
    }

    Ok(command)
}

fn spawn_reader(
    inner: Arc<TerminalInner>,
    mut reader: Box<dyn Read + Send>,
    broadcaster: Arc<OutputBroadcaster>,
) {
    let _ = std::thread::Builder::new()
        .name("cli-pocket-pty-reader".to_string())
        .spawn(move || {
            let mut buffer = [0_u8; 8192];

            loop {
                let read = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        tracing::debug!("pty reader exiting after read error: {error}");
                        break;
                    }
                };

                let chunk = Bytes::copy_from_slice(&buffer[..read]);
                let seq_at_end = {
                    let mut ring = lock_or_recover(&inner.ring, "ring");
                    ring.push(&chunk);
                    ring.head_seq()
                };

                broadcaster.send(OutputChunk {
                    seq_at_end,
                    bytes: chunk,
                });
            }
        });
}

fn spawn_waiter(
    inner: Arc<TerminalInner>,
    mut child: Box<dyn Child + Send + Sync>,
    exit_tx: watch::Sender<Option<ExitInfo>>,
) {
    let _ = std::thread::Builder::new()
        .name("cli-pocket-pty-waiter".to_string())
        .spawn(move || {
            let exit = match child.wait() {
                Ok(status) => exit_info_from_status(&status),
                Err(error) => {
                    tracing::debug!("pty waiter observed wait error: {error}");
                    default_exit_info()
                }
            };

            inner.exited.store(true, Ordering::Release);
            if exit_tx.send(Some(exit)).is_err() {
                tracing::debug!(
                    "pty waiter dropped exit notification because all receivers closed"
                );
            }
        });
}

fn exit_info_from_status(status: &portable_pty::ExitStatus) -> ExitInfo {
    // portable-pty 0.8.x keeps its signal field private and exposes no stable
    // numeric accessor. Do not parse Display text; unknown or localized signal
    // names must not become invented signal numbers.
    ExitInfo {
        code: i32::try_from(status.exit_code()).ok(),
        signal: None,
        at_unix_ms: now_unix_ms(),
    }
}

fn default_exit_info() -> ExitInfo {
    ExitInfo {
        code: Some(1),
        signal: None,
        at_unix_ms: now_unix_ms(),
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(u64::MAX)
}

fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, name: &'static str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("recovering poisoned terminal {name} lock");
            poisoned.into_inner()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputRecv;
    use tokio::time::{sleep, timeout, Duration};

    fn echo_params() -> TerminalCreateParams {
        TerminalCreateParams {
            cols: 80,
            rows: 24,
            cwd: None,
            cmd: echo_command(),
            env: Vec::new(),
            scrollback_bytes: None,
        }
    }

    #[cfg(unix)]
    fn echo_command() -> Vec<String> {
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "echo hello".to_string(),
        ]
    }

    #[cfg(windows)]
    fn echo_command() -> Vec<String> {
        vec![
            "C:\\Windows\\System32\\cmd.exe".to_string(),
            "/C".to_string(),
            "echo hello".to_string(),
        ]
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn echo_hello_appears_in_snapshot() {
        let params = echo_params();
        let terminal = Terminal::spawn(&params).expect("terminal should spawn");

        let _exit = timeout(Duration::from_secs(5), terminal.wait())
            .await
            .expect("terminal should exit");

        for _ in 0..10 {
            let snapshot = terminal.snapshot();
            if String::from_utf8_lossy(&snapshot.bytes).contains("hello") {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }

        let snapshot = terminal.snapshot();
        let output = String::from_utf8_lossy(&snapshot.bytes);
        panic!("snapshot did not contain expected output: {output:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_receives_live_output() {
        let params = echo_params();
        let terminal = Terminal::spawn(&params).expect("terminal should spawn");
        let mut stream = terminal.subscribe();

        let bytes = timeout(Duration::from_secs(5), async {
            loop {
                match stream.recv().await {
                    OutputRecv::Chunk(chunk) if chunk.bytes.as_ref().contains(&b'h') => {
                        return chunk.bytes;
                    }
                    OutputRecv::Chunk(_) | OutputRecv::Lagged { .. } => {}
                    OutputRecv::Closed => return Bytes::new(),
                }
            }
        })
        .await
        .expect("terminal should emit output");

        let output = String::from_utf8_lossy(&bytes);
        assert!(
            output.contains('h'),
            "unexpected live output chunk: {output:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_command_spawns_default_shell_that_can_be_killed() {
        let params = TerminalCreateParams {
            cols: 80,
            rows: 24,
            cwd: None,
            cmd: Vec::new(),
            env: Vec::new(),
            scrollback_bytes: None,
        };
        let terminal = Terminal::spawn(&params).expect("default shell should spawn");

        terminal
            .kill(KillSignal::Term)
            .expect("default shell kill should succeed");

        let exit = timeout(Duration::from_secs(5), terminal.wait())
            .await
            .expect("default shell should exit after kill");
        assert!(exit.code.is_some() || exit.signal.is_some());
    }

    #[test]
    fn exit_info_falls_back_to_code_when_signal_number_is_not_exposed() {
        let status = portable_pty::ExitStatus::with_signal("SIGHUP");

        let exit = exit_info_from_status(&status);

        assert_eq!(exit.code, Some(1));
        assert_eq!(exit.signal, None);
    }

    #[test]
    fn exit_info_never_invents_zero_signal_for_unknown_signal_text() {
        let status = portable_pty::ExitStatus::with_signal("SIGABRT");

        let exit = exit_info_from_status(&status);

        assert_eq!(exit.code, Some(1));
        assert_ne!(exit.signal, Some(0));
    }

    #[test]
    fn kill_error_is_reported_when_process_has_not_exited() {
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");

        let result = map_kill_error(&error, false);

        assert!(matches!(result, Err(TerminalError::Pty(message)) if message.contains("denied")));
    }
}
