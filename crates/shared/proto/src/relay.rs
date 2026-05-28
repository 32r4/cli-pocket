use crate::terminal::HostId;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairCloseReason {
    Normal,
    HostGone,
    ClientGone,
    Stuck,
    RelayShutdown,
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayCtrl {
    HostRegister {
        host_id: HostId,
        host_pubkey: ByteBuf,
        signature: ByteBuf,
    },
    HostRegisterOk,
    HostRegisterErr {
        reason: String,
    },
    HostHeartbeat,
    ClientConnect {
        host_id: HostId,
    },
    PairInbound {
        pair_id: PairId,
    },
    PairOpen {
        pair_id: PairId,
    },
    PairClose {
        pair_id: PairId,
        reason: PairCloseReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayData {
    Forward { pair_id: PairId, bytes: ByteBuf },
}

pub const RELAY_DISC_CTRL: u8 = 0x01;
pub const RELAY_DISC_DATA: u8 = 0x02;
