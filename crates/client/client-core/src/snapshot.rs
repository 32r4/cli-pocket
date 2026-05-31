use bytes::Bytes;
use cli_pocket_proto::{Snapshot, TerminalInfo};

#[derive(Debug, Clone)]
pub struct TerminalSnapshot {
    pub info: TerminalInfo,
    pub bytes: Bytes,
}

impl TerminalSnapshot {
    #[must_use]
    pub fn from_parts(info: TerminalInfo, snapshot: Snapshot) -> Self {
        Self {
            info,
            bytes: Bytes::from(snapshot.bytes.into_vec()),
        }
    }
}
