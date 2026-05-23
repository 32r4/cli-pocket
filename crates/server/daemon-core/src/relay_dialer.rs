//! Relay dialer scaffold; implemented in Task D10.

use cli_pocket_proto::HostId;
use crate::config::RelayConfig;
use tracing::info;

/// Connect to the configured relay and register as a host.
///
/// Full implementation deferred to Task D10 / Plan F integration.
#[allow(unreachable_code)]
pub async fn run(_config: RelayConfig, _host_id: HostId) -> crate::DaemonResult<()> {
    info!("relay dialer started (stub)");
    std::future::pending::<()>().await;
    Ok(())
}
