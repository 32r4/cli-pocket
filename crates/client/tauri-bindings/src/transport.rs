use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, ClientResult, Transport};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct TokioWsTransport {
    ws: WsStream,
}

impl TokioWsTransport {
    pub async fn connect(url: &str, subprotocol: Option<&str>) -> ClientResult<Self> {
        let mut request = url
            .into_client_request()
            .map_err(|err| ClientError::Transport(err.to_string()))?;

        if let Some(subprotocol) = subprotocol {
            let header_value = subprotocol
                .parse()
                .map_err(|err| ClientError::Transport(format!("invalid subprotocol: {err}")))?;
            request
                .headers_mut()
                .insert("Sec-WebSocket-Protocol", header_value);
        }

        let (ws, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|err| ClientError::Transport(err.to_string()))?;

        Ok(Self { ws })
    }
}

#[async_trait(?Send)]
impl Transport for TokioWsTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> ClientResult<()> {
        self.ws
            .send(Message::Binary(bytes))
            .await
            .map_err(|err| ClientError::Transport(err.to_string()))
    }

    async fn recv(&mut self) -> ClientResult<Option<Vec<u8>>> {
        loop {
            match self.ws.next().await {
                Some(Ok(Message::Binary(bytes))) => return Ok(Some(bytes)),
                Some(Ok(Message::Ping(bytes))) => {
                    self.ws
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|err| ClientError::Transport(err.to_string()))?;
                }
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Ok(Message::Text(_))) => {
                    return Err(ClientError::Transport(
                        "unexpected text frame on binary transport".into(),
                    ));
                }
                Some(Ok(Message::Pong(_) | Message::Frame(_))) => {}
                Some(Err(err)) => return Err(ClientError::Transport(err.to_string())),
            }
        }
    }

    async fn close(&mut self) -> ClientResult<()> {
        self.ws
            .close(None)
            .await
            .map_err(|err| ClientError::Transport(err.to_string()))
    }
}
