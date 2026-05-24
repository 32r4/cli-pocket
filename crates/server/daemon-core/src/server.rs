//! Daemon facade: boot, start, shutdown — wires all dependencies.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cli_pocket_proto::ServerInfo;
use cli_pocket_transport::{TokioWsTransport, Transport};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::client_db::ClientDb;
use crate::config::DaemonConfig;
use crate::identity_store::{load_or_create, DaemonIdentity};
use crate::listener::{serve, ListenerDeps};
use crate::pairing::PairingCodes;
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
    pub pairing_codes: PairingCodes,
    pub server_info: ServerInfo,
    pub config: DaemonConfig,
    listener_handle: Option<JoinHandle<()>>,
    relay_handle: Option<JoinHandle<()>>,
    pairing_code_handle: Option<JoinHandle<()>>,
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
        let pairing_codes = PairingCodes::new(Duration::from_secs(config.pairing.code_ttl_secs));

        let server_info = ServerInfo {
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            host_label: None,
        };

        Ok(Self {
            identity,
            session_mgr,
            client_db,
            pairing_codes,
            server_info,
            config,
            listener_handle: None,
            relay_handle: None,
            pairing_code_handle: None,
        })
    }

    /// Start the WS listener on the configured address.
    ///
    /// If a relay is configured in `config.relay`, also spawn the relay
    /// dialer task.
    pub async fn start(&mut self) -> crate::DaemonResult<()> {
        let addr = SocketAddr::new(self.config.listen.addr, self.config.listen.port);
        let listener = TcpListener::bind(addr).await?;

        let identity = Arc::new(self.identity.keypair.clone());
        let psk = self.config.relay.as_ref().and_then(|r| {
            let bytes = hex::decode(&r.psk_hex).ok()?;
            let arr: [u8; 32] = bytes.try_into().ok()?;
            Some(Arc::new(arr))
        });
        let session_mgr = Arc::clone(&self.session_mgr);
        let client_db = Arc::clone(&self.client_db);
        let pairing_codes = self.pairing_codes.clone();
        let server_info = self.server_info.clone();

        let listener_deps = ListenerDeps {
            identity,
            psk,
            session_mgr,
            client_db,
            pairing_codes,
            server_info,
        };

        let handle = tokio::spawn(async move {
            if let Err(e) = serve(listener, listener_deps).await {
                error!(error = %e, "listener exited with error");
            }
        });
        self.listener_handle = Some(handle);

        let pairing_codes = self.pairing_codes.clone();
        let tick_secs = self.config.pairing.code_ttl_secs.max(1);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));
            loop {
                interval.tick().await;
                if let Some(code) = pairing_codes.rotate_if_expired() {
                    info!(pairing_code = %code, "pairing code rotated");
                }
            }
        });
        self.pairing_code_handle = Some(handle);

        if let Some(relay_config) = self.config.relay.clone() {
            let host_id = self.identity.host_id;
            let identity_keypair = self.identity.keypair.clone();
            let handle = tokio::spawn(async move {
                if let Err(e) =
                    crate::relay_dialer::run(relay_config, host_id, identity_keypair).await
                {
                    error!(error = %e, "relay dialer exited with error");
                }
            });
            self.relay_handle = Some(handle);
        }

        info!(pairing_code = %self.pairing_codes.current_code(), "daemon started");
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
        if let Some(h) = self.pairing_code_handle {
            h.abort();
        }
    }

    /// Utility: return the daemon's public key as a hex string.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.identity.keypair.public)
    }
}

pub async fn run_pairing_transport(
    mut transport: TokioWsTransport,
    identity: &cli_pocket_crypto::KeyPair,
    client_db: &ClientDb,
    pairing_codes: &PairingCodes,
) -> crate::DaemonResult<crate::client_db::ClientRecord> {
    use cli_pocket_crypto::Spake2Side;
    use cli_pocket_proto::ClientId;
    use uuid::Uuid;

    let _rotated = pairing_codes.rotate_if_expired();
    let code = pairing_codes.current_code();
    let sp = Spake2Side::start_host(code.as_bytes(), PAIRING_HOST_ID, PAIRING_CLIENT_ID);
    let outbound = sp.outbound().to_vec();
    transport.send(outbound).await?;
    let peer_msg = transport.recv().await?.ok_or_else(|| {
        crate::DaemonError::Internal("pairing peer closed before SPAKE2 reply".into())
    })?;
    let outcome = sp
        .finish(&peer_msg)
        .map_err(|e| crate::DaemonError::Internal(format!("SPAKE2 finish failed: {e}")))?;

    if !pairing_codes.match_current(&code) {
        return Err(crate::DaemonError::Internal(
            "pairing code expired during handshake".into(),
        ));
    }

    let payload = transport.recv().await?.ok_or_else(|| {
        crate::DaemonError::Internal("pairing peer closed before client pk".into())
    })?;
    let client_pk_vec = decrypt_pairing_payload(&outcome.psk, &payload)?;
    let client_pk: [u8; 32] = client_pk_vec
        .as_slice()
        .try_into()
        .map_err(|_| crate::DaemonError::Internal("expected 32-byte client pk".into()))?;

    let _next_code = pairing_codes.consume_current(&code).ok_or_else(|| {
        crate::DaemonError::Internal("pairing code expired before client registration".into())
    })?;

    let record = crate::client_db::ClientRecord {
        client_id: ClientId(Uuid::now_v7()),
        public_key: client_pk,
        paired_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    let record = client_db.add_or_lookup_by_public(record).await?;

    let ack = encrypt_pairing_payload(&outcome.psk, &identity.public)?;
    transport.send(ack).await?;

    Ok(record)
}

/// Identity binding used by the daemon side of the SPAKE2 pairing exchange.
const PAIRING_HOST_ID: &[u8] = b"cli-pocket pairing host v1";
/// Identity binding used by the client side of the SPAKE2 pairing exchange.
const PAIRING_CLIENT_ID: &[u8] = b"cli-pocket pairing client v1";
/// HKDF-style info label embedded into the AEAD context. The SPAKE2 PSK is
/// already domain-separated, but we keep the label for forward compatibility.
const PAIRING_AEAD_NONCE: [u8; 12] = [0_u8; 12];

fn decrypt_pairing_payload(psk: &[u8; 32], ct: &[u8]) -> crate::DaemonResult<Vec<u8>> {
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
    let cipher = ChaCha20Poly1305::new(psk.into());
    let nonce = PAIRING_AEAD_NONCE.into();
    cipher
        .decrypt(&nonce, ct)
        .map_err(|e| crate::DaemonError::Internal(format!("pairing decrypt failed: {e}")))
}

fn encrypt_pairing_payload(psk: &[u8; 32], plain: &[u8]) -> crate::DaemonResult<Vec<u8>> {
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
    let cipher = ChaCha20Poly1305::new(psk.into());
    let nonce = PAIRING_AEAD_NONCE.into();
    cipher
        .encrypt(&nonce, plain)
        .map_err(|e| crate::DaemonError::Internal(format!("pairing encrypt failed: {e}")))
}
