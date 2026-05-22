use cli_pocket_proto::{StreamId, TerminalInfo};

#[derive(Debug, Clone)]
pub struct TerminalHandle {
    pub info: TerminalInfo,
    pub stream: StreamId,
}
