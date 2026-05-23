use crate::transport::{Transport, TransportError};
use async_trait::async_trait;
use futures_util::{sink::SinkExt, stream::StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct TokioWsTransport {
    ws: WsStream,
}

impl TokioWsTransport {
    pub fn new(ws: WsStream) -> Self {
        Self { ws }
    }

    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| TransportError::WebSocket(e.to_string()))?;
        Ok(Self::new(ws))
    }
}

#[async_trait]
impl Transport for TokioWsTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError> {
        self.ws
            .send(Message::Binary(bytes))
            .await
            .map_err(|e| TransportError::WebSocket(e.to_string()))
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        loop {
            match self.ws.next().await {
                None | Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Err(e)) => return Err(TransportError::WebSocket(e.to_string())),
                Some(Ok(Message::Binary(bytes))) => return Ok(Some(bytes)),
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                Some(Ok(Message::Text(_))) => {
                    return Err(TransportError::WebSocket(
                        "unexpected text frame on binary transport".into(),
                    ));
                }
            }
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.ws
            .close(None)
            .await
            .map_err(|e| TransportError::WebSocket(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::TokioWsTransport;
    use crate::Transport;
    use futures_util::sink::SinkExt;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    async fn connect_pair() -> (
        TokioWsTransport,
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap()
        });

        let client = TokioWsTransport::connect(&format!("ws://{addr}"))
            .await
            .unwrap();
        let server_ws = server.await.unwrap();
        (client, server_ws)
    }

    #[tokio::test]
    async fn recv_binary_frame() {
        let (mut client, mut server_ws) = connect_pair().await;

        server_ws
            .send(Message::Binary(vec![1, 2, 3]))
            .await
            .unwrap();

        let got = client.recv().await.unwrap();
        assert_eq!(got, Some(vec![1, 2, 3]));
    }

    #[tokio::test]
    async fn recv_rejects_text_frames() {
        let (mut client, mut server_ws) = connect_pair().await;

        server_ws.send(Message::Text("hello".into())).await.unwrap();

        let err = client.recv().await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "websocket: unexpected text frame on binary transport"
        );
    }

    #[tokio::test]
    async fn recv_returns_none_after_close() {
        let (mut client, mut server_ws) = connect_pair().await;

        server_ws.close(None).await.unwrap();
        drop(server_ws);

        let got = client.recv().await.unwrap();
        assert!(got.is_none());
    }
}
