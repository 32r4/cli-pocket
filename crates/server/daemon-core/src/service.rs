use std::path::PathBuf;

use crate::config::{default_config_path, DaemonConfig, SecurityConfig};
use crate::Daemon;

const BUILD_CONFIG_TEMPLATE: &str = include_str!("../../daemon-bin/daemon.build.toml");

pub fn build_config_template() -> &'static str {
    BUILD_CONFIG_TEMPLATE
}

pub fn load_or_create_config(config_path: Option<PathBuf>) -> crate::DaemonResult<DaemonConfig> {
    let path = config_path.unwrap_or_else(default_config_path);

    if path.exists() {
        return DaemonConfig::load_from(&path);
    }

    let mut cfg = DaemonConfig {
        security: SecurityConfig::for_config_path(&path),
        ..DaemonConfig::default()
    };
    cfg.relay.psk_hex = generate_relay_psk_hex()?;
    cfg.save_to(&path)?;
    Ok(cfg)
}

pub async fn pair_url(config: DaemonConfig) -> crate::DaemonResult<String> {
    let daemon = Daemon::boot(config).await?;
    daemon.pair_url()
}

fn generate_relay_psk_hex() -> crate::DaemonResult<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| crate::DaemonError::Internal(format!("read OS random bytes: {error}")))?;
    Ok(hex::encode(bytes))
}
