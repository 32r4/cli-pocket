use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelayConfig {
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(default)]
    pub caps: CapsConfig,
    #[serde(default)]
    pub guillotine: GuillotineConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenConfig {
    pub addr: IpAddr,
    pub port: u16,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 8080,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsConfig {
    pub max_hosts: usize,
    pub max_pairs: usize,
    pub max_bytes_per_sec: u64,
    pub max_queued_bytes: usize,
}

impl Default for CapsConfig {
    fn default() -> Self {
        Self {
            max_hosts: 256,
            max_pairs: 2048,
            max_bytes_per_sec: 4 * 1024 * 1024,
            max_queued_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuillotineConfig {
    pub idle_seconds: u64,
}

impl Default for GuillotineConfig {
    fn default() -> Self {
        Self { idle_seconds: 120 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// If set, hosts must present this bearer token.
    pub host_token: Option<String>,
    /// If set, clients must present this bearer token.
    pub client_token: Option<String>,
}

impl RelayConfig {
    /// Load relay configuration from a TOML file.
    pub fn load_from(path: &Path) -> crate::RelayResult<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|err| crate::RelayError::Config(err.to_string()))
    }
}
