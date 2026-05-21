use crate::terminal::HostId;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OfferId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Endpoint {
    Direct { host: String, port: u16 },
    Loopback { port: u16 },
    Relay { relay_url: String, host_id: HostId },
}

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
    HostUnregister,
    ClientPairRequest {
        host_id: HostId,
        attempt_token: u32,
    },
    ClientCodeLookup {
        hint: ByteBuf,
    },
    ClientPairCancel,
    PairInbound {
        pair_id: PairId,
        attempt_token: u32,
    },
    PairRejected {
        reason: String,
    },
    PairOpen {
        pair_id: PairId,
    },
    PairClose {
        pair_id: PairId,
        reason: PairCloseReason,
    },
    OfferAvailable {
        offer_id: OfferId,
        host_pubkey: ByteBuf,
        endpoints: Vec<Endpoint>,
    },
    OfferConsumed,
    OfferStale,
    OfferPublish {
        offer_id: OfferId,
        spake2_m_share: ByteBuf,
        host_pubkey: ByteBuf,
        endpoints: Vec<Endpoint>,
        ttl_secs: u32,
    },
    OfferRetract {
        offer_id: OfferId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayData {
    Forward { pair_id: PairId, bytes: ByteBuf },
}

pub const RELAY_DISC_CTRL: u8 = 0x01;
pub const RELAY_DISC_DATA: u8 = 0x02;
