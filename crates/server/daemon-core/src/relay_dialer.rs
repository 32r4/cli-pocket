//! Relay dialer skeleton.
//!
//! The relay dialer connects to the configured relay URL, registers as a host,
//! and forwards inbound ciphertext frames to the local listener as if they
//! were direct client connections. This task ships a minimal skeleton; the
//! full relay multiplexing graduates in Plan F integration once Plan E exposes
//! its host-side test harness.
//!
//! The on-the-wire protocol is locked in by `cli_pocket_proto::relay`'s
//! `RelayCtrl` / `RelayData` enums (see `crates/shared/proto/src/relay.rs`).

use cli_pocket_crypto::KeyPair;
use cli_pocket_proto::HostId;
// Imported for documentation of the protocol surface this task targets at
// integration time; the skeleton itself does not dispatch on them yet.
#[allow(unused_imports)]
use cli_pocket_proto::relay::{RelayCtrl, RelayData};
use tracing::{info, warn};

use crate::config::RelayConfig;

/// Connect to the configured relay and register as a host.
///
/// Currently a logging-only skeleton: emits a connect log, parks the task on
/// `pending`, and would return `Ok(())` on shutdown. The real loop will:
///
/// 1. Dial `config.url` with subprotocol `cli-pocket-relay-host/v1`.
/// 2. Send `RelayCtrl::HostRegister { host_id, host_pubkey, signature }`,
///    optionally presenting `config.host_token` as a bearer.
/// 3. For each `RelayCtrl::PairInbound { pair_id, .. }`:
///    - capacity-check, accept or `PairRejected`,
///    - on subsequent `RelayData::Forward { pair_id, bytes }` deliver to a
///      virtual transport driving `run_connection`.
/// 4. Send periodic `RelayCtrl::HostHeartbeat` while idle.
///
/// Full implementation deferred to Plan F integration.
pub async fn run(
    config: RelayConfig,
    host_id: HostId,
    identity: KeyPair,
) -> crate::DaemonResult<()> {
    info!(
        url = %config.url,
        host_id = ?host_id,
        host_token = config.host_token.is_some(),
        psk_len = config.psk_hex.len(),
        "relay dialer starting (skeleton)"
    );
    // The identity and PSK are not consumed yet; reference them so the
    // signature stays stable when the real loop lands.
    let _ = (&identity, &config.psk_hex);
    warn!("relay multiplexing not yet implemented; daemon serves direct LAN clients only");

    futures_util::future::pending::<()>().await;
    Ok(())
}
