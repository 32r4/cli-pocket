use bytes::Bytes;
use cli_pocket_proto::{StreamId, StreamSeq, TerminalId, TerminalInfo};
use futures_channel::mpsc;
use futures_util::SinkExt;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TerminalHandle {
    pub info: TerminalInfo,
    stream: Rc<RefCell<StreamId>>,
    pub(crate) last_seq: Option<StreamSeq>,
    pub(crate) cmd_tx: mpsc::Sender<TerminalCmd>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalCmd {
    Input {
        terminal: TerminalId,
        bytes: Bytes,
        queued_at: Instant,
    },
    Resize {
        terminal: TerminalId,
        cols: u16,
        rows: u16,
    },
    Kill {
        terminal: TerminalId,
    },
}

impl TerminalHandle {
    pub(crate) fn new(
        info: TerminalInfo,
        stream: StreamId,
        last_seq: Option<StreamSeq>,
        cmd_tx: mpsc::Sender<TerminalCmd>,
    ) -> Self {
        Self {
            info,
            stream: Rc::new(RefCell::new(stream)),
            last_seq,
            cmd_tx,
        }
    }

    #[must_use]
    pub fn terminal_id(&self) -> TerminalId {
        self.info.terminal
    }

    #[must_use]
    pub fn stream_id(&self) -> StreamId {
        *self.stream.borrow()
    }

    pub async fn write_input(&self, bytes: Bytes) -> crate::ClientResult<()> {
        self.cmd_tx
            .clone()
            .send(TerminalCmd::Input {
                terminal: self.terminal_id(),
                bytes,
                queued_at: Instant::now(),
            })
            .await
            .map_err(|_| crate::ClientError::Closed)
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> crate::ClientResult<()> {
        self.cmd_tx
            .clone()
            .send(TerminalCmd::Resize {
                terminal: self.terminal_id(),
                cols,
                rows,
            })
            .await
            .map_err(|_| crate::ClientError::Closed)
    }

    pub async fn kill(&self) -> crate::ClientResult<()> {
        self.cmd_tx
            .clone()
            .send(TerminalCmd::Kill {
                terminal: self.terminal_id(),
            })
            .await
            .map_err(|_| crate::ClientError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    fn handle() -> (TerminalHandle, mpsc::Receiver<TerminalCmd>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(1);
        let terminal = TerminalId::new();
        let handle = TerminalHandle::new(
            TerminalInfo {
                terminal,
                cols: 80,
                rows: 24,
                created_at_unix_ms: 0,
                label: Some("shell".to_owned()),
                attached_clients: 1,
            },
            StreamId(7),
            None,
            cmd_tx,
        );

        (handle, cmd_rx)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_input_sends_stream_scoped_command() {
        let (handle, mut cmd_rx) = handle();

        handle
            .write_input(Bytes::from_static(b"ls\n"))
            .await
            .unwrap();

        match cmd_rx.next().await.unwrap() {
            TerminalCmd::Input {
                terminal,
                bytes,
                queued_at: _,
            } => {
                assert_eq!(terminal, handle.terminal_id());
                assert_eq!(bytes, Bytes::from_static(b"ls\n"));
            }
            other => panic!("expected input command, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn kill_sends_terminal_scoped_command() {
        let (handle, mut cmd_rx) = handle();

        handle.kill().await.unwrap();

        assert_eq!(
            cmd_rx.next().await.unwrap(),
            TerminalCmd::Kill {
                terminal: handle.terminal_id(),
            }
        );
    }
}
