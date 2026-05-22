use crate::transport::{Transport, TransportError};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// One half of an in-memory bidirectional transport pair. Used by tests
/// across crates to drive client-daemon code without spinning up sockets.
pub struct InMemoryTransport {
    tx: Option<mpsc::Sender<Vec<u8>>>,
    rx: mpsc::Receiver<Vec<u8>>,
    closed: bool,
}

pub struct InMemoryTransportPair {
    pub a: InMemoryTransport,
    pub b: InMemoryTransport,
}

impl InMemoryTransportPair {
    #[must_use]
    pub fn new(buffer: usize) -> Self {
        let (atx, brx) = mpsc::channel(buffer);
        let (btx, arx) = mpsc::channel(buffer);
        Self {
            a: InMemoryTransport {
                tx: Some(atx),
                rx: arx,
                closed: false,
            },
            b: InMemoryTransport {
                tx: Some(btx),
                rx: brx,
                closed: false,
            },
        }
    }
}

#[async_trait]
impl Transport for InMemoryTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }

        let tx = self.tx.as_ref().ok_or(TransportError::Closed)?;
        tx.send(bytes).await.map_err(|_| TransportError::Closed)
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        if self.closed {
            return Ok(None);
        }

        Ok(self.rx.recv().await)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.closed = true;
        self.tx = None;
        self.rx.close();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{InMemoryTransportPair, Transport};
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn pair_exchanges_payload() {
        let mut pair = InMemoryTransportPair::new(4);

        pair.a.send(vec![1, 2, 3]).await.unwrap();

        let got = timeout(Duration::from_secs(1), pair.b.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, Some(vec![1, 2, 3]));
    }

    #[tokio::test]
    async fn close_propagates_to_recv() {
        let mut pair = InMemoryTransportPair::new(4);

        pair.a.close().await.unwrap();

        let got = timeout(Duration::from_secs(1), pair.b.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn one_megabyte_exchange_completes_in_under_a_second() {
        let mut pair = InMemoryTransportPair::new(16);
        let chunk = vec![0u8; 4096];
        let start = std::time::Instant::now();

        let producer = tokio::spawn(async move {
            for _ in 0..256 {
                pair.a.send(chunk.clone()).await.unwrap();
            }
        });

        let mut received = 0usize;
        while received < 1024 * 1024 {
            let bytes = timeout(Duration::from_secs(1), pair.b.recv())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            received += bytes.len();
        }

        producer.await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
