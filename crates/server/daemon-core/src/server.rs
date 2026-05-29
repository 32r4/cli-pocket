//! Daemon facade: boot, start, shutdown — wires all dependencies.

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use cli_pocket_proto::ServerInfo;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::accept::{run_accepted_transport, AcceptDeps, AcceptedTransport};
use crate::client_db::ClientDb;
use crate::config::{
    build_pairing_offer_url, relay_client_ws_url_for_server, DaemonConfig, PairingOffer,
};
use crate::identity_store::{load_or_create, DaemonIdentity};
use crate::listener::serve;
use crate::session::SessionManager;

/// Top-level daemon struct owning all subsystems.
///
/// Lifecycle:
/// 1. `Daemon::boot(config)` — load identity, open client DB, create session manager.
/// 2. `daemon.start()` — spawn the WS listener and relay dialer.
/// 3. `daemon.shutdown()` — abort all spawned tasks.
pub struct Daemon {
    pub identity: DaemonIdentity,
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
    pub server_info: ServerInfo,
    pub config: DaemonConfig,
    listener_handle: Option<JoinHandle<()>>,
    relay_handle: Option<JoinHandle<()>>,
    listener_accept_handle: Option<JoinHandle<()>>,
    relay_accept_handle: Option<JoinHandle<()>>,
}

impl Daemon {
    /// Create all subsystems from configuration.
    ///
    /// This does **not** start any network listeners. Call [`Self::start`]
    /// to begin accepting connections.
    pub async fn boot(config: DaemonConfig) -> crate::DaemonResult<Self> {
        let identity = load_or_create(&config.security.identity_path)?;

        let client_db = Arc::new(
            ClientDb::open(&config.security.clients_path, &config.security.revoked_path).await?,
        );

        let session_mgr = Arc::new(SessionManager::new(config.limits.max_terminals));

        let server_info = ServerInfo {
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            server_label: detect_server_label(),
        };

        Ok(Self {
            identity,
            session_mgr,
            client_db,
            server_info,
            config,
            listener_handle: None,
            relay_handle: None,
            listener_accept_handle: None,
            relay_accept_handle: None,
        })
    }

    /// Start the WS listener on the configured address and spawn the relay
    /// dialer task.
    pub async fn start(&mut self) -> crate::DaemonResult<()> {
        let addr = SocketAddr::new(self.config.listen.addr, self.config.listen.port);
        let listener = TcpListener::bind(addr).await?;

        let identity = Arc::new(self.identity.keypair.clone());
        let relay_psk = {
            let bytes = hex::decode(&self.config.relay.psk_hex).ok();
            bytes.and_then(|bytes| {
                let arr: [u8; 32] = bytes.try_into().ok()?;
                Some(Arc::new(arr))
            })
        };
        let accept_deps = AcceptDeps {
            identity,
            relay_psk: relay_psk.clone(),
            session_mgr: Arc::clone(&self.session_mgr),
            client_db: Arc::clone(&self.client_db),
            server_info: self.server_info.clone(),
        };
        let (listener_tx, mut listener_rx) =
            mpsc::channel::<AcceptedTransport<cli_pocket_transport::TokioWsTransport>>(32);
        let listener_deps = accept_deps.clone();
        let handle = tokio::spawn(async move {
            while let Some(accepted) = listener_rx.recv().await {
                let deps = listener_deps.clone();
                tokio::spawn(async move {
                    let _ = run_accepted_transport(accepted, deps).await;
                });
            }
        });
        self.listener_accept_handle = Some(handle);

        let handle = tokio::spawn(async move {
            if let Err(e) = serve(listener, listener_tx).await {
                error!(error = %e, "listener exited with error");
            }
        });
        self.listener_handle = Some(handle);

        let relay_config = self.config.relay.clone();
        let server_id = self.identity.server_id;
        let identity_keypair = self.identity.keypair.clone();
        let (relay_tx, mut relay_rx) =
            mpsc::channel::<AcceptedTransport<crate::relay_dialer::PairTransport>>(32);
        let relay_deps = accept_deps.clone();
        let handle = tokio::spawn(async move {
            while let Some(accepted) = relay_rx.recv().await {
                let deps = relay_deps.clone();
                tokio::spawn(async move {
                    let _ = run_accepted_transport(accepted, deps).await;
                });
            }
        });
        self.relay_accept_handle = Some(handle);

        let handle = tokio::spawn(async move {
            if let Err(e) = crate::relay_dialer::run_forever(
                relay_config,
                server_id,
                identity_keypair,
                relay_tx,
            )
            .await
            {
                error!(error = %e, "relay dialer exited with error");
            }
        });
        self.relay_handle = Some(handle);

        info!("daemon started");
        Ok(())
    }

    /// Abort all spawned tasks (listener, relay dialer).
    pub async fn shutdown(self) {
        if let Some(h) = self.listener_handle {
            h.abort();
        }
        if let Some(h) = self.relay_handle {
            h.abort();
        }
        if let Some(h) = self.listener_accept_handle {
            h.abort();
        }
        if let Some(h) = self.relay_accept_handle {
            h.abort();
        }
    }

    /// Utility: return the daemon's public key as a hex string.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.identity.keypair.public)
    }

    pub fn pair_url(&self) -> crate::DaemonResult<String> {
        let relay_url =
            relay_client_ws_url_for_server(&self.config.relay.base_url, self.identity.server_id)?;
        let relay_psk_hex = self.config.relay.psk_hex.trim();
        if relay_psk_hex.is_empty() {
            return Err(crate::DaemonError::Config(
                "pair-url requires relay.psk_hex".to_owned(),
            ));
        }

        let relay_psk = hex::decode(relay_psk_hex)
            .map_err(|error| crate::DaemonError::Config(format!("relay.psk_hex: {error}")))?;
        if relay_psk.len() != 32 {
            return Err(crate::DaemonError::Config(
                "pair-url requires relay.psk_hex to decode to 32 bytes".to_owned(),
            ));
        }

        build_pairing_offer_url(
            &self.config.app.base_url,
            &PairingOffer {
                label: None,
                server_id: self.identity.server_id,
                server_public_hex: self.public_key_hex(),
                relay_url,
                relay_psk_hex: relay_psk_hex.to_owned(),
            },
        )
    }
}

fn detect_server_label() -> Option<String> {
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        let value = env::var(key).ok()?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    None
}
