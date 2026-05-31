use std::sync::Arc;

use cli_pocket_crypto::KeyPair;
use cli_pocket_proto::ServerInfo;
use cli_pocket_transport::Transport;

use crate::client_db::ClientDb;
use crate::connection::{run_connection_with_handshake, ConnectionDeps, HandshakeKind};
use crate::session::SessionManager;

pub struct AcceptedTransport<T> {
    pub label: String,
    pub kind: AcceptedTransportKind,
    pub transport: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptedTransportKind {
    Direct { auto_pair: bool },
    Relay { auto_pair: bool },
}

#[derive(Clone)]
pub struct AcceptDeps {
    pub identity: Arc<KeyPair>,
    pub relay_psk: Option<Arc<[u8; 32]>>,
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
    pub server_info: ServerInfo,
}

impl AcceptDeps {
    pub fn connection_deps(&self) -> ConnectionDeps {
        ConnectionDeps {
            session_mgr: Arc::clone(&self.session_mgr),
            client_db: Arc::clone(&self.client_db),
            server_info: self.server_info.clone(),
        }
    }
}

pub async fn run_accepted_transport<T>(
    accepted: AcceptedTransport<T>,
    deps: AcceptDeps,
) -> crate::DaemonResult<()>
where
    T: Transport,
{
    let AcceptedTransport {
        label,
        kind,
        transport,
    } = accepted;
    let handshake = match kind {
        AcceptedTransportKind::Direct { auto_pair } => HandshakeKind::Direct { auto_pair },
        AcceptedTransportKind::Relay { auto_pair } => HandshakeKind::Relay {
            psk: deps.relay_psk.as_deref(),
            auto_pair,
        },
    };
    let result =
        run_connection_with_handshake(transport, &deps.identity, handshake, deps.connection_deps())
            .await;

    match result {
        Ok(()) => {
            tracing::info!(target = "cli_pocket_daemon::accept", %label, "connection closed cleanly");
            Ok(())
        }
        Err(error) => {
            let is_expected = match &error {
                crate::DaemonError::Transport(transport_err) => {
                    transport_err.is_expected_disconnect()
                }
                crate::DaemonError::Internal(msg) => {
                    msg.contains("transport closed during frame recv")
                }
                _ => false,
            };

            if is_expected {
                tracing::debug!(target = "cli_pocket_daemon::accept", %label, %error, "connection closed by client");
            } else {
                tracing::error!(target = "cli_pocket_daemon::accept", %label, %error, "connection ended with error");
            }
            Err(error)
        }
    }
}
