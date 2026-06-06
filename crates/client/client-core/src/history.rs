use bytes::Bytes;
use cli_pocket_proto::{StreamSeq, TerminalId};

#[derive(Debug, Clone)]
pub struct TerminalHistoryPage {
    pub terminal_id: TerminalId,
    pub start_seq: StreamSeq,
    pub end_seq: StreamSeq,
    pub bytes: Bytes,
    pub has_more: bool,
}

impl TerminalHistoryPage {
    #[must_use]
    pub fn new(
        terminal_id: TerminalId,
        start_seq: StreamSeq,
        end_seq: StreamSeq,
        bytes: Bytes,
        has_more: bool,
    ) -> Self {
        Self {
            terminal_id,
            start_seq,
            end_seq,
            bytes,
            has_more,
        }
    }
}
