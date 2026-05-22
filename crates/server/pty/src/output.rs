use bytes::Bytes;
use cli_pocket_proto::StreamSeq;
use tokio::sync::broadcast;

const SUBSCRIBER_CAP: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChunk {
    pub seq_at_end: StreamSeq,
    pub bytes: Bytes,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OutputRecv {
    Chunk(OutputChunk),
    Lagged { skipped: u64 },
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lagged {
    pub skipped: u64,
}

pub struct OutputBroadcaster {
    tx: broadcast::Sender<OutputChunk>,
}

impl OutputBroadcaster {
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(SUBSCRIBER_CAP);
        Self { tx }
    }

    pub fn send(&self, chunk: OutputChunk) {
        let _ = self.tx.send(chunk);
    }

    #[must_use]
    pub fn subscribe(&self) -> OutputStream {
        OutputStream {
            rx: self.tx.subscribe(),
        }
    }
}

impl Default for OutputBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

pub struct OutputStream {
    rx: broadcast::Receiver<OutputChunk>,
}

impl OutputStream {
    pub async fn recv(&mut self) -> OutputRecv {
        match self.rx.recv().await {
            Ok(chunk) => OutputRecv::Chunk(chunk),
            Err(broadcast::error::RecvError::Lagged(skipped)) => OutputRecv::Lagged { skipped },
            Err(broadcast::error::RecvError::Closed) => OutputRecv::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn receives_chunks_in_order() {
        let broadcaster = OutputBroadcaster::new();
        let mut stream = broadcaster.subscribe();

        broadcaster.send(OutputChunk {
            seq_at_end: StreamSeq(3),
            bytes: Bytes::from_static(b"abc"),
        });

        match stream.recv().await {
            OutputRecv::Chunk(chunk) => {
                assert_eq!(chunk.seq_at_end, StreamSeq(3));
                assert_eq!(chunk.bytes, Bytes::from_static(b"abc"));
            }
            other => panic!("unexpected recv: {other:?}"),
        }
    }

    #[tokio::test]
    async fn lagged_stream_returns_lagged() {
        let broadcaster = OutputBroadcaster::new();
        let mut stream = broadcaster.subscribe();

        for seq in 0..=(SUBSCRIBER_CAP as u64) {
            broadcaster.send(OutputChunk {
                seq_at_end: StreamSeq(seq),
                bytes: Bytes::from_static(b"x"),
            });
        }

        match stream.recv().await {
            OutputRecv::Lagged { skipped } => assert_eq!(skipped, 1),
            other => panic!("expected lagged recv, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn closed_stream_returns_closed() {
        let broadcaster = OutputBroadcaster::new();
        let mut stream = broadcaster.subscribe();
        drop(broadcaster);

        assert!(matches!(stream.recv().await, OutputRecv::Closed));
    }
}
