use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection closed")]
    Closed,
    #[error("io: {0}")]
    Io(String),
    #[error("websocket: {0}")]
    WebSocket(String),
}

/// Bidirectional binary-framed transport. Every send is one logical message;
/// every recv yields one logical message (or `None` on close).
#[async_trait]
pub trait Transport: Send + 'static {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError>;
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
    async fn close(&mut self) -> Result<(), TransportError>;
}
