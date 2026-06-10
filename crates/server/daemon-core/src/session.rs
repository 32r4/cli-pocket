//! Session manager: owns TerminalId -> Arc<Terminal> with background reaper.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use tokio::task::JoinHandle;

use cli_pocket_proto::{KillSignal, TerminalCreateParams, TerminalId, TerminalInfo};
use cli_pocket_pty::Terminal;

struct TerminalEntry {
    terminal: Arc<Terminal>,
    created_at_unix_ms: u64,
}

struct Inner {
    terminals: HashMap<TerminalId, TerminalEntry>,
    max_terminals: usize,
}

pub struct SessionManager {
    inner: Arc<Mutex<Inner>>,
    _reaper: JoinHandle<()>,
}

impl SessionManager {
    /// Create a new session manager.
    ///
    /// `max_terminals` caps the number of concurrently alive terminals.
    pub fn new(max_terminals: usize) -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            terminals: HashMap::new(),
            max_terminals,
        }));

        let reaper_inner = Arc::clone(&inner);
        let reaper = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let dead: Vec<TerminalId> = {
                    let g = reaper_inner.lock();
                    g.terminals
                        .iter()
                        .filter_map(|(id, entry)| {
                            // Try a brief wait with timeout to detect exited terminals.
                            // Using a short timeout avoids blocking the reaper indefinitely.
                            if entry.terminal.is_exited() {
                                Some(*id)
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                if !dead.is_empty() {
                    let mut g = reaper_inner.lock();
                    for id in &dead {
                        g.terminals.remove(id);
                    }
                }
            }
        });

        Self {
            inner,
            _reaper: reaper,
        }
    }

    /// Create a new terminal and register it with the session manager.
    ///
    /// Returns `TerminalInfo` on success. Fails if the terminal limit is reached
    /// or if the PTY subsystem cannot spawn the terminal.
    pub async fn create(
        &self,
        params: TerminalCreateParams,
        scrollback_bytes: usize,
    ) -> crate::DaemonResult<TerminalInfo> {
        {
            let g = self.inner.lock();
            if g.terminals.len() >= g.max_terminals {
                return Err(crate::DaemonError::Internal(format!(
                    "terminal limit reached ({})",
                    g.max_terminals
                )));
            }
        }

        let terminal =
            Terminal::spawn(&params, scrollback_bytes).map_err(crate::DaemonError::Pty)?;
        let id = terminal.id();
        let (cols, rows) = terminal.dims();
        let created_at_unix_ms = now_unix_ms();

        let terminal = Arc::new(terminal);
        let label = terminal.title();
        let entry = TerminalEntry {
            terminal: Arc::clone(&terminal),
            created_at_unix_ms,
        };

        {
            let mut g = self.inner.lock();
            g.terminals.insert(id, entry);
        }

        Ok(TerminalInfo {
            terminal: id,
            cols,
            rows,
            created_at_unix_ms,
            label,
            attached_clients: 0,
        })
    }

    /// Attach to an existing terminal by ID.
    ///
    /// Returns `Some(Arc<Terminal>)` if the terminal exists and has not been reaped,
    /// or `None` if the terminal is unknown.
    #[must_use]
    pub fn attach(&self, id: &TerminalId) -> Option<Arc<Terminal>> {
        self.inner
            .lock()
            .terminals
            .get(id)
            .map(|entry| Arc::clone(&entry.terminal))
    }

    /// Kill a terminal by ID, sending the specified signal to the PTY child.
    ///
    /// Returns an error if the terminal is not found. Kill is idempotent:
    /// killing an already-exited terminal is a no-op.
    pub async fn kill(&self, id: &TerminalId, signal: KillSignal) -> crate::DaemonResult<()> {
        let terminal = self
            .inner
            .lock()
            .terminals
            .get(id)
            .map(|entry| Arc::clone(&entry.terminal))
            .ok_or_else(|| crate::DaemonError::Internal(format!("unknown terminal {id:?}")))?;
        terminal.kill(signal).map_err(crate::DaemonError::Pty)
    }

    /// List all currently registered terminals.
    #[must_use]
    pub fn list(&self) -> Vec<TerminalInfo> {
        let g = self.inner.lock();
        g.terminals
            .iter()
            .map(|(id, entry)| {
                let (cols, rows) = entry.terminal.dims();
                TerminalInfo {
                    terminal: *id,
                    cols,
                    rows,
                    created_at_unix_ms: entry.created_at_unix_ms,
                    label: entry.terminal.title(),
                    attached_clients: 0,
                }
            })
            .collect()
    }

    /// Return the number of currently registered terminals.
    #[must_use]
    pub fn count(&self) -> usize {
        self.inner.lock().terminals.len()
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn new_manager_has_zero_count() {
        let mgr = SessionManager::new(8);
        assert_eq!(mgr.count(), 0);
        assert!(mgr.list().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn attach_returns_none_for_missing_id() {
        let mgr = SessionManager::new(8);
        let id = TerminalId::new();
        assert!(mgr.attach(&id).is_none());
    }
}
