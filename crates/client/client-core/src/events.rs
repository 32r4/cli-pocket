use bytes::Bytes;
use cli_pocket_proto::{SessionId, StreamSeq, TerminalId, TerminalInfo};

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connecting,
    Connected {
        session_id: SessionId,
        server_label: Option<String>,
    },
    Disconnected {
        will_retry: bool,
        reason: String,
    },
    TerminalCreated(TerminalInfo),
    TerminalOutput {
        terminal_id: TerminalId,
        stream_seq: StreamSeq,
        bytes: Bytes,
    },
    Error(String),
}
