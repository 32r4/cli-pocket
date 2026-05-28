use cli_pocket_proto::codec::{decode_relay, encode_relay_ctrl, RelayWire};
use cli_pocket_proto::{HostId, PairId, RelayCtrl};

use crate::{ClientError, ClientResult, Transport};

pub async fn open_client_pair<T: Transport>(
    transport: &mut T,
    host_id: HostId,
) -> ClientResult<PairId> {
    transport
        .send(encode_relay_ctrl(&RelayCtrl::ClientConnect { host_id }).map_err(ClientError::from)?)
        .await?;

    match recv_ctrl(transport).await? {
        RelayCtrl::PairOpen { pair_id } => Ok(pair_id),
        RelayCtrl::PairClose { reason, .. } => Err(ClientError::Transport(format!(
            "relay closed pair before open: {reason:?}"
        ))),
        other => Err(ClientError::Transport(format!(
            "unexpected relay control frame before pair open: {other:?}"
        ))),
    }
}

pub async fn maybe_handle_pair_close<T: Transport>(
    transport: &mut T,
) -> ClientResult<Option<RelayCtrl>> {
    let Some(bytes) = transport.recv().await? else {
        return Ok(None);
    };
    match decode_relay(&bytes) {
        Ok(RelayWire::Ctrl(ctrl)) => Ok(Some(ctrl)),
        Ok(RelayWire::Data(_)) => Err(ClientError::Transport(
            "unexpected relay host data on client leg".to_owned(),
        )),
        Err(_) => Err(ClientError::Transport(
            "unexpected relay control frame after pair open".to_owned(),
        )),
    }
}

async fn recv_ctrl<T: Transport>(transport: &mut T) -> ClientResult<RelayCtrl> {
    let Some(bytes) = transport.recv().await? else {
        return Err(ClientError::Closed);
    };
    match decode_relay(&bytes).map_err(ClientError::from)? {
        RelayWire::Ctrl(ctrl) => Ok(ctrl),
        RelayWire::Data(_) => Err(ClientError::Transport(
            "unexpected relay data frame before pair open".to_owned(),
        )),
    }
}
