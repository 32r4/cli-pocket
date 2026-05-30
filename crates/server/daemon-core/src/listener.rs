//! WebSocket listener task: accepts TCP connections, upgrades to WS,
//! and yields accepted `/session` transports.

use std::net::SocketAddr;
use std::sync::Arc;

use cli_pocket_transport::TokioWsTransport;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tokio_tungstenite::MaybeTlsStream;
use tracing::{error, info};

use crate::accept::{AcceptedTransport, AcceptedTransportKind};

const DIRECT_SESSION_SUBPROTOCOL: &str = "cli-pocket-server/v1";

/// Bind to the given address and accept inbound WebSocket `/session` transports.
///
/// This function only returns on listener error (e.g. port already in use) or
/// when the receiver side of `accepted_tx` has been dropped.
pub async fn listen(
    addr: SocketAddr,
    accepted_tx: mpsc::Sender<AcceptedTransport<TokioWsTransport>>,
) -> crate::DaemonResult<()> {
    let listener = TcpListener::bind(addr).await?;
    serve(listener, accepted_tx).await
}

/// Accept inbound WebSocket connections on an already-bound listener.
///
/// This lets callers separate the bind step from the accept loop so startup
/// can fail synchronously if the listen address is unavailable.
pub async fn serve(
    listener: TcpListener,
    accepted_tx: mpsc::Sender<AcceptedTransport<TokioWsTransport>>,
) -> crate::DaemonResult<()> {
    let addr = listener.local_addr()?;
    info!(%addr, "daemon listening");

    loop {
        let (sock, peer) = listener.accept().await?;
        let accepted_tx = accepted_tx.clone();

        tokio::spawn(async move {
            info!(%peer, "connection opened");

            let request_path = Arc::new(parking_lot::Mutex::new(None));
            let callback_path = Arc::clone(&request_path);
            let ws = match accept_hdr_async(
                MaybeTlsStream::Plain(sock),
                PathCapture {
                    request_path: callback_path,
                },
            )
            .await
            {
                Ok(ws) => ws,
                Err(e) => {
                    error!(%peer, error = %e, "ws upgrade failed");
                    return;
                }
            };

            let transport = TokioWsTransport::new(ws);
            let path = request_path
                .lock()
                .clone()
                .unwrap_or_else(|| "/".to_string());

            if path != "/session" {
                error!(%peer, %path, "unsupported websocket path");
                return;
            }

            if let Err(error) = accepted_tx
                .send(AcceptedTransport {
                    label: peer.to_string(),
                    kind: AcceptedTransportKind::Direct {
                        auto_pair: peer.ip().is_loopback(),
                    },
                    transport,
                })
                .await
            {
                error!(%peer, %error, "listener accept channel closed");
            }
        });
    }
}

struct PathCapture {
    request_path: Arc<parking_lot::Mutex<Option<String>>>,
}

impl Callback for PathCapture {
    #[allow(clippy::result_large_err, clippy::unnecessary_wraps)]
    fn on_request(
        self,
        request: &Request,
        mut response: Response,
    ) -> Result<Response, ErrorResponse> {
        *self.request_path.lock() = Some(request.uri().path().to_string());

        if request
            .headers()
            .get(SEC_WEBSOCKET_PROTOCOL)
            .and_then(|value| value.to_str().ok())
            .is_some_and(requested_subprotocol_supported)
        {
            response.headers_mut().insert(
                SEC_WEBSOCKET_PROTOCOL,
                DIRECT_SESSION_SUBPROTOCOL
                    .parse()
                    .expect("valid websocket subprotocol header"),
            );
        }

        Ok(response)
    }
}

fn requested_subprotocol_supported(value: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .any(|item| item == DIRECT_SESSION_SUBPROTOCOL)
}
