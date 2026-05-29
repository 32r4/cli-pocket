use crate::terminal::ServerId;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairCloseReason {
    Normal,
    ServerGone,
    ClientGone,
    Stuck,
    RelayShutdown,
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayCtrl {
    ServerRegister {
        server_id: ServerId,
        server_pubkey: ByteBuf,
        signature: ByteBuf,
    },
    ServerRegisterOk,
    ServerRegisterErr {
        reason: String,
    },
    ServerHeartbeat,
    ClientConnect {
        server_id: ServerId,
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
