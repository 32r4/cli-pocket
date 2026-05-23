use cli_pocket_client_core::Transport;
use cli_pocket_tauri_bindings::TokioWsTransport;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::result_large_err)]
async fn echo_roundtrip_with_subprotocol() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_hdr_async(
            sock,
            |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
             mut resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(
                    req.headers()
                        .get("Sec-WebSocket-Protocol")
                        .and_then(|value| value.to_str().ok()),
                    Some("cli-pocket-host/v1")
                );
                resp.headers_mut().insert(
                    "Sec-WebSocket-Protocol",
                    "cli-pocket-host/v1".parse().unwrap(),
                );
                Ok(resp)
            },
        )
        .await
        .unwrap();

        while let Some(msg) = ws.next().await {
            if let Ok(Message::Binary(bytes)) = msg {
                ws.send(Message::Binary(bytes)).await.unwrap();
            }
        }
    });

    let url = format!("ws://{addr}");
    let mut transport = TokioWsTransport::connect(&url, Some("cli-pocket-host/v1"))
        .await
        .unwrap();
    transport.send(b"hello".to_vec()).await.unwrap();

    let echoed = transport.recv().await.unwrap();
    assert_eq!(echoed.as_deref(), Some(&b"hello"[..]));

    transport.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn recv_returns_error_for_text_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        ws.send(Message::Text("not binary".into())).await.unwrap();
        futures_util::future::pending::<()>().await;
    });

    let url = format!("ws://{addr}");
    let mut transport = TokioWsTransport::connect(&url, None).await.unwrap();

    let err = timeout(Duration::from_secs(1), transport.recv())
        .await
        .expect("recv timed out waiting for text-frame error")
        .unwrap_err();

    assert_eq!(
        err.to_string(),
        "transport: unexpected text frame on binary transport"
    );
}
