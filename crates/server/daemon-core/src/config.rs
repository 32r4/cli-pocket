use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cli_pocket_proto::HostId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub relay: Option<RelayConfig>,
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
        Self {
            addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 7842,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub identity_path: PathBuf,
    pub clients_path: PathBuf,
    pub revoked_path: PathBuf,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        let base = default_data_dir();

        Self {
            identity_path: base.join("identity.json"),
            clients_path: base.join("clients.json"),
            revoked_path: base.join("revoked.json"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub url: String,
    pub psk_hex: String,
    #[serde(default)]
    pub host_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub base_url: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: "https://cli-pocket.32r4.asia".to_owned(),
        }
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
        Self {
            max_terminals: 64,
            scrollback_bytes: 2 * 1024 * 1024,
            scrollback_anchor_interval: 64 * 1024,
            broadcast_capacity: 1024,
        }
    }
}

impl DaemonConfig {
    pub fn load_from(path: &Path) -> crate::DaemonResult<Self> {
        let text = std::fs::read_to_string(path)?;

        toml::from_str(&text).map_err(|error| crate::DaemonError::Config(error.to_string()))
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
    pub host_id: HostId,
    pub server_public_hex: String,
    pub relay_url: String,
    pub relay_psk_hex: String,
}

#[derive(Debug, Clone, Serialize)]
struct PairingOfferPayload<'a> {
    v: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
    #[serde(rename = "hostId")]
    host_id: String,
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
        host_id: offer.host_id.0.to_string(),
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

fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join("cli-pocket");
        }
    } else if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("cli-pocket");
    } else if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/cli-pocket");
    }

    PathBuf::from(".cli-pocket")
}

#[cfg(test)]
mod tests {
    use super::{build_pairing_offer_url, AppConfig, PairingOffer};
    use cli_pocket_proto::HostId;

    #[test]
    fn app_config_defaults_to_official_base_url() {
        assert_eq!(
            AppConfig::default().base_url,
            "https://cli-pocket.32r4.asia"
        );
    }

    #[test]
    fn pairing_offer_url_uses_fragment_and_trims_trailing_slash() {
        let host_id = HostId(uuid::Uuid::now_v7());
        let url = build_pairing_offer_url(
            "https://cli-pocket.32r4.asia/",
            &PairingOffer {
                label: Some("Primary Host".to_owned()),
                host_id,
                server_public_hex: "11".repeat(32),
                relay_url: "wss://relay.example/ws/client?host=abc".to_owned(),
                relay_psk_hex: "22".repeat(32),
            },
        )
        .expect("build pairing offer url");

        assert!(url.starts_with("https://cli-pocket.32r4.asia/#pair="));
        assert!(url.contains("#pair="));
        assert!(!url.contains("/#pair=#pair="));
    }
}
