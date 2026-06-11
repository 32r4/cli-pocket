use std::path::PathBuf;

use crate::config::{
    build_pairing_qr_code, default_config_path, DaemonConfig, PairingQrCode, SecurityConfig,
};
use crate::Daemon;

const BUILD_CONFIG_TEMPLATE: &str = include_str!("../../daemon-bin/daemon.build.toml");
const DEV_CONFIG_TEMPLATE: &str = include_str!("../../daemon-bin/daemon.dev.toml");

pub fn build_config_template() -> &'static str {
    BUILD_CONFIG_TEMPLATE
}

pub fn dev_config_template() -> &'static str {
    DEV_CONFIG_TEMPLATE
}

pub fn load_or_create_config(config_path: Option<PathBuf>) -> crate::DaemonResult<DaemonConfig> {
    let path = config_path.unwrap_or_else(default_config_path);
    load_or_create_config_with_template(path, BUILD_CONFIG_TEMPLATE)
}

pub fn load_or_create_config_with_template(
    config_path: PathBuf,
    template: &str,
) -> crate::DaemonResult<DaemonConfig> {
    let path = config_path;

    if path.exists() {
        return DaemonConfig::load_from(&path);
    }

    let mut cfg = toml::from_str::<DaemonConfig>(template)
        .map_err(|error| crate::DaemonError::Config(error.to_string()))?;
    cfg.security = SecurityConfig::for_config_path(&path);
    if cfg.relay.psk_hex.is_empty() {
        cfg.relay.psk_hex = generate_relay_psk_hex()?;
    }
    cfg.save_to(&path)?;
    Ok(cfg)
}

pub async fn pair_url(config: DaemonConfig) -> crate::DaemonResult<String> {
    let daemon = Daemon::boot(config).await?;
    daemon.pair_url()
}

pub async fn pair_qr_code(config: DaemonConfig) -> crate::DaemonResult<PairingQrCode> {
    let url = pair_url(config).await?;
    build_pairing_qr_code(url)
}

fn generate_relay_psk_hex() -> crate::DaemonResult<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| crate::DaemonError::Internal(format!("read OS random bytes: {error}")))?;
    Ok(hex::encode(bytes))
}
