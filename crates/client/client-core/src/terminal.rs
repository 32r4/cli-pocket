use bytes::Bytes;
use cli_pocket_proto::{StreamId, TerminalId, TerminalInfo};
use futures_channel::mpsc;
use futures_util::SinkExt;

#[derive(Debug, Clone)]
pub struct TerminalHandle {
    pub info: TerminalInfo,
    pub stream: StreamId,
    pub(crate) cmd_tx: mpsc::Sender<TerminalCmd>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalCmd {
    Input {
        stream: StreamId,
        bytes: Bytes,
    },
    Resize {
        stream: StreamId,
        cols: u16,
        rows: u16,
    },
    Kill {
        terminal: TerminalId,
    },
    Detach {
        stream: StreamId,
    },
}

impl TerminalHandle {
    #[must_use]
    pub fn terminal_id(&self) -> TerminalId {
        self.info.terminal
    }

    #[must_use]
    pub fn stream_id(&self) -> StreamId {
        self.stream
    }

    pub async fn write_input(&self, bytes: Bytes) -> crate::ClientResult<()> {
        self.cmd_tx
            .clone()
            .send(TerminalCmd::Input {
                stream: self.stream,
                bytes,
            })
            .await
            .map_err(|_| crate::ClientError::Closed)
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> crate::ClientResult<()> {
        self.cmd_tx
            .clone()
            .send(TerminalCmd::Resize {
                stream: self.stream,
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

    pub async fn detach(&self) -> crate::ClientResult<()> {
        self.cmd_tx
            .clone()
            .send(TerminalCmd::Detach {
                stream: self.stream,
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
        let handle = TerminalHandle {
            info: TerminalInfo {
                terminal,
                cols: 80,
                rows: 24,
                created_at_unix_ms: 0,
                label: Some("shell".to_owned()),
                attached_clients: 1,
            },
            stream: StreamId(7),
            cmd_tx,
        };

        (handle, cmd_rx)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_input_sends_stream_scoped_command() {
        let (handle, mut cmd_rx) = handle();

        handle
            .write_input(Bytes::from_static(b"ls\n"))
            .await
            .unwrap();

        assert_eq!(
            cmd_rx.next().await.unwrap(),
            TerminalCmd::Input {
                stream: StreamId(7),
                bytes: Bytes::from_static(b"ls\n"),
            }
        );
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
