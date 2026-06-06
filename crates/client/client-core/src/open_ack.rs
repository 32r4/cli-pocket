use bytes::Bytes;
use cli_pocket_proto::{StreamId, StreamSeq, TerminalInfo};

#[derive(Debug, Clone)]
pub struct TerminalOpenAck {
    pub stream_id: StreamId,
    pub info: TerminalInfo,
    pub start_seq: StreamSeq,
    pub end_seq: StreamSeq,
    pub render_bytes: Bytes,
    pub has_more_history: bool,
}

impl TerminalOpenAck {
    #[must_use]
    pub fn new(
        stream_id: StreamId,
        info: TerminalInfo,
        start_seq: StreamSeq,
        end_seq: StreamSeq,
        render_bytes: Bytes,
        has_more_history: bool,
    ) -> Self {
        Self {
            stream_id,
            info,
            start_seq,
            end_seq,
            render_bytes,
            has_more_history,
        }
    }
}
