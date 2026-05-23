//! Daemon facade: boot, start, shutdown — wires all dependencies.

use std::net::SocketAddr;
use std::sync::Arc;

use cli_pocket_proto::ServerInfo;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::client_db::ClientDb;
use crate::config::DaemonConfig;
use crate::identity_store::{load_or_create, DaemonIdentity};
use crate::listener::{listen, ListenerDeps};
use crate::session::SessionManager;

/// Top-level daemon struct owning all subsystems.
///
/// Lifecycle:
/// 1. `Daemon::boot(config)` — load identity, open client DB, create session manager.
/// 2. `daemon.start()` — spawn the WS listener (and relay dialer if configured).
/// 3. `daemon.shutdown()` — abort all spawned tasks.
pub struct Daemon {
    pub identity: DaemonIdentity,
    pub session_mgr: Arc<SessionManager>,
    pub client_db: Arc<ClientDb>,
    pub server_info: ServerInfo,
    pub config: DaemonConfig,
    listener_handle: Option<JoinHandle<()>>,
    relay_handle: Option<JoinHandle<()>>,
}

impl Daemon {
    /// Create all subsystems from configuration.
    ///
    /// This does **not** start any network listeners. Call [`Self::start`]
    /// to begin accepting connections.
    pub async fn boot(config: DaemonConfig) -> crate::DaemonResult<Self> {
        let identity = load_or_create(&config.security.identity_path)?;

        let client_db = Arc::new(
            ClientDb::open(
                &config.security.clients_path,
                &config.security.revoked_path,
            )
            .await?,
        );

        let session_mgr =
            Arc::new(SessionManager::new(config.limits.max_terminals));

        let server_info = ServerInfo {
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            host_label: None,
        };

        Ok(Self {
            identity,
            session_mgr,
            client_db,
            server_info,
            config,
            listener_handle: None,
            relay_handle: None,
        })
    }

    /// Start the WS listener on the configured address.
    ///
    /// If a relay is configured in `config.relay`, also spawn the relay
    /// dialer task.
    pub async fn start(&mut self) -> crate::DaemonResult<()> {
        let addr = SocketAddr::new(self.config.listen.addr, self.config.listen.port);

        let identity = Arc::new(self.identity.keypair.clone());
        let psk = self.config.relay.as_ref().and_then(|r| {
            let bytes = hex::decode(&r.psk_hex).ok()?;
            let arr: [u8; 32] = bytes.try_into().ok()?;
            Some(Arc::new(arr))
        });
        let session_mgr = Arc::clone(&self.session_mgr);
        let client_db = Arc::clone(&self.client_db);
        let server_info = self.server_info.clone();

        let listener_deps = ListenerDeps {
            identity,
            psk,
            session_mgr,
            client_db,
            server_info,
        };

        let handle = tokio::spawn(async move {
            if let Err(e) = listen(addr, listener_deps).await {
                error!(error = %e, "listener exited with error");
            }
        });
        self.listener_handle = Some(handle);

        if self.config.relay.is_some() {
            let host_id = self.identity.host_id;
            let relay_config = self.config.relay.clone().unwrap();
            let handle = tokio::spawn(async move {
                if let Err(e) = crate::relay_dialer::run(relay_config, host_id).await {
                    error!(error = %e, "relay dialer exited with error");
                }
            });
            self.relay_handle = Some(handle);
        }

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
    }

    /// Utility: return the daemon's public key as a hex string.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.identity.keypair.public)
    }
}
