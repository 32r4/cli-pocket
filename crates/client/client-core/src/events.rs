use bytes::Bytes;
use cli_pocket_proto::{ExitInfo, SessionId, StreamSeq, TerminalId, TerminalInfo};

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connecting,
    Connected {
        session_id: SessionId,
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
    TerminalExited {
        terminal_id: TerminalId,
        info: ExitInfo,
    },
    Error(String),
}
