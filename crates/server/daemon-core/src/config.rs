use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

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
