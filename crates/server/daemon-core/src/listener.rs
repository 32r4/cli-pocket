//! WebSocket listener task: accepts TCP connections, upgrades to WS,
//! and spawns per-connection handlers.

use std::net::SocketAddr;
use std::sync::Arc;

use cli_pocket_crypto::KeyPair;
use cli_pocket_proto::ServerInfo;
use cli_pocket_transport::TokioWsTransport;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::MaybeTlsStream;
use tracing::{error, info};

use crate::client_db::ClientDb;
use crate::connection::{run_connection_with_handshake, ConnectionDeps};
use crate::pairing::PairingCodes;
use crate::server::run_pairing_transport;
use crate::session::SessionManager;

/// Shared dependencies handed to each accepted connection.
#[derive(Clone)]
pub struct ListenerDeps {
    pub identity: Arc<KeyPair>,
    pub psk: Option<Arc<[u8; 32]>>,
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
    pub pairing_codes: PairingCodes,
    pub server_info: ServerInfo,
}

/// Bind to the given address and accept inbound WebSocket connections.
///
/// Each accepted connection is upgraded to WebSocket, wrapped in a
/// `TokioWsTransport`, and handed to `run_connection_with_handshake`
/// in a spawned task.
///
/// This function only returns on listener error (e.g. port already in use).
pub async fn listen(addr: SocketAddr, deps: ListenerDeps) -> crate::DaemonResult<()> {
    let listener = TcpListener::bind(addr).await?;
    serve(listener, deps).await
}

/// Accept inbound WebSocket connections on an already-bound listener.
///
/// This lets callers separate the bind step from the accept loop so startup
/// can fail synchronously if the listen address is unavailable.
pub async fn serve(listener: TcpListener, deps: ListenerDeps) -> crate::DaemonResult<()> {
    let addr = listener.local_addr()?;
    info!(%addr, "daemon listening");

    loop {
        let (sock, peer) = listener.accept().await?;
        let deps = deps.clone();

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

            if path == "/pair" {
                match run_pairing_transport(
                    transport,
                    &deps.identity,
                    &deps.client_db,
                    &deps.pairing_codes,
                )
                .await
                {
                    Ok(record) => {
                        info!(
                            %peer,
                            client_id = %record.client_id.0,
                            "client paired"
                        );
                    }
                    Err(e) => error!(%peer, error = %e, "pairing ended with error"),
                }
                return;
            }

            if path != "/session" {
                error!(%peer, %path, "unsupported websocket path");
                return;
            }

            let psk_ref = deps.psk.as_ref().map(|arc| arc.as_ref());
            if let Err(e) = run_connection_with_handshake(
                transport,
                &deps.identity,
                psk_ref,
                ConnectionDeps {
                    session_mgr: deps.session_mgr,
                    client_db: deps.client_db,
                    server_info: deps.server_info,
                },
            )
            .await
            {
                error!(%peer, error = %e, "connection ended with error");
            } else {
                info!(%peer, "connection closed cleanly");
            }
        });
    }
}

struct PathCapture {
    request_path: Arc<parking_lot::Mutex<Option<String>>>,
}

impl Callback for PathCapture {
    #[allow(clippy::result_large_err, clippy::unnecessary_wraps)]
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        *self.request_path.lock() = Some(request.uri().path().to_string());
        Ok(response)
    }
}
