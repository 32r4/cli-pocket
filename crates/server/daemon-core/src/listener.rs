//! WebSocket listener task: accepts TCP connections, upgrades to WS,
//! and spawns per-connection handlers.

use std::net::SocketAddr;
use std::sync::Arc;

use cli_pocket_crypto::KeyPair;
use cli_pocket_proto::ServerInfo;
use cli_pocket_transport::TokioWsTransport;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::MaybeTlsStream;
use tracing::{error, info};

use crate::client_db::ClientDb;
use crate::connection::{run_connection_with_handshake, ConnectionDeps};
use crate::session::SessionManager;

/// Shared dependencies handed to each accepted connection.
#[derive(Clone)]
pub struct ListenerDeps {
    pub identity: Arc<KeyPair>,
    pub psk: Option<Arc<[u8; 32]>>,
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
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
    info!(%addr, "daemon listening");

    loop {
        let (sock, peer) = listener.accept().await?;
        let deps = deps.clone();

        tokio::spawn(async move {
            info!(%peer, "connection opened");

            let ws = match accept_async(MaybeTlsStream::Plain(sock)).await {
                Ok(ws) => ws,
                Err(e) => {
                    error!(%peer, error = %e, "ws upgrade failed");
                    return;
                }
            };

            let transport = TokioWsTransport::new(ws);

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
