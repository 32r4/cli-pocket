use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cli_pocket_proto::ServerId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(skip)]
    pub security: SecurityConfig,
    pub relay: RelayConfig,
    #[serde(default)]
    pub app: AppConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenConfig {
    pub addr: IpAddr,
    pub port: u16,
}

impl Default for ListenConfig {
    fn default() -> Self {
        daemon_build_template().listen.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub identity_path: PathBuf,
    pub clients_path: PathBuf,
    pub revoked_path: PathBuf,
}

impl SecurityConfig {
    pub fn for_config_path(config_path: &Path) -> Self {
        if config_path == Path::new("crates/server/daemon-bin/daemon.dev.toml") {
            let base = PathBuf::from(".cache/cli-pocket-daemon-dev");
            return Self {
                identity_path: base.join("identity.json"),
                clients_path: base.join("clients.json"),
                revoked_path: base.join("revoked.json"),
            };
        }

        let base = config_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(default_state_dir, Path::to_path_buf);

        Self {
            identity_path: base.join("identity.json"),
            clients_path: base.join("clients.json"),
            revoked_path: base.join("revoked.json"),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        let base = default_state_dir();

        Self {
            identity_path: base.join("identity.json"),
            clients_path: base.join("clients.json"),
            revoked_path: base.join("revoked.json"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub base_url: String,
    pub psk_hex: String,
    #[serde(default)]
    pub server_auth_token: Option<String>,
    #[serde(default)]
    pub retry: RelayRetryConfig,
}

impl Default for RelayConfig {
    fn default() -> Self {
        daemon_build_template().relay.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayRetryConfig {
    #[serde(default = "default_relay_retry_initial_ms")]
    pub initial_ms: u64,
    #[serde(default = "default_relay_retry_max_ms")]
    pub max_ms: u64,
    #[serde(default = "default_relay_retry_mul_x10")]
    pub mul_x10: u32,
}

impl Default for RelayRetryConfig {
    fn default() -> Self {
        Self {
            initial_ms: default_relay_retry_initial_ms(),
            max_ms: default_relay_retry_max_ms(),
            mul_x10: default_relay_retry_mul_x10(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub base_url: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        daemon_build_template().app.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_terminals: usize,
    pub scrollback_bytes: usize,
    pub scrollback_anchor_interval: usize,
    pub broadcast_capacity: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        daemon_build_template().limits.clone()
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let mut cfg = daemon_build_template().clone();
        cfg.security = SecurityConfig::default();
        cfg
    }
}

impl DaemonConfig {
    pub fn load_from(path: &Path) -> crate::DaemonResult<Self> {
        let text = std::fs::read_to_string(path)?;

        let mut cfg = toml::from_str::<Self>(&text)
            .map_err(|error| crate::DaemonError::Config(error.to_string()))?;
        cfg.security = SecurityConfig::for_config_path(path);
        Ok(cfg)
    }

    pub fn save_to(&self, path: &Path) -> crate::DaemonResult<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|error| crate::DaemonError::Config(error.to_string()))?;

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, text)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingOffer {
    pub label: Option<String>,
    pub server_id: ServerId,
    pub server_public_hex: String,
    pub relay_url: String,
    pub relay_psk_hex: String,
}

#[derive(Debug, Clone, Serialize)]
struct PairingOfferPayload<'a> {
    v: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
    #[serde(rename = "serverId")]
    server_id: String,
    #[serde(rename = "serverPublicHex")]
    server_public_hex: &'a str,
    relay: PairingRelayPayload<'a>,
}

#[derive(Debug, Clone, Serialize)]
struct PairingRelayPayload<'a> {
    url: &'a str,
    #[serde(rename = "pskHex")]
    psk_hex: &'a str,
}

pub fn build_pairing_offer_url(
    app_base_url: &str,
    offer: &PairingOffer,
) -> crate::DaemonResult<String> {
    let base = app_base_url.trim();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(crate::DaemonError::Config(
            "app.base_url must start with http:// or https://".into(),
        ));
    }

    let payload = PairingOfferPayload {
        v: 1,
        label: offer
            .label
            .as_deref()
            .filter(|label| !label.trim().is_empty()),
        server_id: offer.server_id.0.to_string(),
        server_public_hex: &offer.server_public_hex,
        relay: PairingRelayPayload {
            url: &offer.relay_url,
            psk_hex: &offer.relay_psk_hex,
        },
    };
    let json = serde_json::to_vec(&payload)
        .map_err(|error| crate::DaemonError::Config(error.to_string()))?;
    let encoded = URL_SAFE_NO_PAD.encode(json);
    let trimmed = base.trim_end_matches('/');

    Ok(format!("{trimmed}/#pair={encoded}"))
}

pub fn relay_server_ws_url(base_url: &str) -> crate::DaemonResult<String> {
    relay_ws_url(base_url, "/ws/server")
}

pub fn relay_server_ws_url_for_server(
    base_url: &str,
    server_id: ServerId,
) -> crate::DaemonResult<String> {
    let url = relay_server_ws_url(base_url)?;
    Ok(format!("{url}?server={}", server_id.0))
}

pub fn relay_client_ws_url(base_url: &str) -> crate::DaemonResult<String> {
    relay_ws_url(base_url, "/ws/client")
}

pub fn relay_client_ws_url_for_server(
    base_url: &str,
    server_id: ServerId,
) -> crate::DaemonResult<String> {
    let url = relay_client_ws_url(base_url)?;
    Ok(format!("{url}?server={}", server_id.0))
}

fn relay_ws_url(base_url: &str, suffix: &str) -> crate::DaemonResult<String> {
    let trimmed = base_url.trim();
    if !(trimmed.starts_with("ws://") || trimmed.starts_with("wss://")) {
        return Err(crate::DaemonError::Config(
            "relay.base_url must start with ws:// or wss://".into(),
        ));
    }

    Ok(format!("{}{suffix}", trimmed.trim_end_matches('/')))
}

fn default_relay_retry_initial_ms() -> u64 {
    500
}

fn default_relay_retry_max_ms() -> u64 {
    30_000
}

fn default_relay_retry_mul_x10() -> u32 {
    20
}

pub fn default_state_dir() -> PathBuf {
    home_dir().map_or_else(
        || PathBuf::from(".cli-pocket"),
        |home| home.join(".cli-pocket"),
    )
}

fn daemon_build_template() -> &'static DaemonConfig {
    static TEMPLATE: OnceLock<DaemonConfig> = OnceLock::new();

    TEMPLATE.get_or_init(|| {
        match toml::from_str(include_str!("../../daemon-bin/daemon.build.toml")) {
            Ok(cfg) => cfg,
            Err(err) => panic!("invalid daemon.build.toml template: {err}"),
        }
    })
}

pub fn default_config_path() -> PathBuf {
    default_state_dir().join("daemon.toml")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(
            || match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
                (Some(drive), Some(path)) => Some(PathBuf::from(drive).join(path)),
                _ => None,
            },
        )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        build_pairing_offer_url, default_config_path, default_state_dir, relay_client_ws_url,
        relay_client_ws_url_for_server, relay_server_ws_url, relay_server_ws_url_for_server,
        AppConfig, PairingOffer,
    };
    use cli_pocket_proto::ServerId;

    #[test]
    fn app_config_defaults_to_official_base_url() {
        assert_eq!(
            AppConfig::default().base_url,
            "https://cli-pocket.32r4.asia"
        );
    }

    #[test]
    fn pairing_offer_url_uses_fragment_and_trims_trailing_slash() {
        let server_id = ServerId(uuid::Uuid::now_v7());
        let url = build_pairing_offer_url(
            "https://cli-pocket.32r4.asia/",
            &PairingOffer {
                label: Some("Primary Server".to_owned()),
                server_id,
                server_public_hex: "11".repeat(32),
                relay_url: "wss://relay.example/ws/client?server=abc".to_owned(),
                relay_psk_hex: "22".repeat(32),
            },
        )
        .expect("build pairing offer url");

        assert!(url.starts_with("https://cli-pocket.32r4.asia/#pair="));
        assert!(url.contains("#pair="));
        assert!(!url.contains("/#pair=#pair="));
    }

    #[test]
    fn relay_ws_urls_append_routes_to_base_url() {
        assert_eq!(
            relay_server_ws_url("wss://relay.example/").expect("server ws url"),
            "wss://relay.example/ws/server"
        );
        assert_eq!(
            relay_server_ws_url_for_server(
                "wss://relay.example",
                ServerId(uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap())
            )
            .expect("server ws url for server"),
            "wss://relay.example/ws/server?server=00112233-4455-6677-8899-aabbccddeeff"
        );
        assert_eq!(
            relay_client_ws_url("wss://relay.example").expect("client ws url"),
            "wss://relay.example/ws/client"
        );
        assert_eq!(
            relay_client_ws_url_for_server(
                "wss://relay.example",
                ServerId(uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap())
            )
            .expect("client ws url for server"),
            "wss://relay.example/ws/client?server=00112233-4455-6677-8899-aabbccddeeff"
        );
    }

    #[test]
    fn default_state_dir_ends_with_dot_cli_pocket() {
        assert!(default_state_dir().ends_with(Path::new(".cli-pocket")));
    }

    #[test]
    fn default_config_path_is_inside_dot_cli_pocket() {
        assert!(default_config_path().ends_with(Path::new(".cli-pocket/daemon.toml")));
    }
}
