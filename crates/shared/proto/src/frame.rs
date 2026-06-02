use crate::error::{ByeReason, ProtocolError};
use crate::hello::{Hello, HelloOk};
use crate::snapshot::Snapshot;
use crate::terminal::ServerConfig;
use crate::terminal::{
    ExitInfo, StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo,
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub body: FrameBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameBody {
    // ---- Connection control ----
    Hello(Hello),
    HelloOk(HelloOk),
    Ping {
        nonce: u32,
    },
    Pong {
        nonce: u32,
    },
    Bye {
        reason: ByeReason,
    },

    // ---- Terminal lifecycle (request/response, request_id paired) ----
    TerminalCreate {
        request_id: u32,
        params: TerminalCreateParams,
    },
    TerminalCreateOk {
        request_id: u32,
        terminal: TerminalId,
        stream: StreamId,
    },
    TerminalCreateErr {
        request_id: u32,
        error: ProtocolError,
    },

    TerminalAttach {
        request_id: u32,
        terminal: TerminalId,
        since: Option<StreamSeq>,
    },
    TerminalAttachOk {
        request_id: u32,
        snapshot: Snapshot,
        head_seq: StreamSeq,
        stream: StreamId,
        initial_window: u32,
    },
    TerminalAttachErr {
        request_id: u32,
        error: ProtocolError,
    },

    TerminalDetach {
        request_id: u32,
        stream: StreamId,
    },
    TerminalDetachOk {
        request_id: u32,
    },
    TerminalDetachErr {
        request_id: u32,
        error: ProtocolError,
    },

    TerminalKill {
        request_id: u32,
        terminal: TerminalId,
    },
    TerminalKillOk {
        request_id: u32,
    },
    TerminalKillErr {
        request_id: u32,
        error: ProtocolError,
    },

    TerminalList {
        request_id: u32,
    },
    TerminalListOk {
        request_id: u32,
        terminals: Vec<TerminalInfo>,
    },

    ServerConfigGet {
        request_id: u32,
    },
    ServerConfigGetOk {
        request_id: u32,
        config: ServerConfig,
    },
    ServerConfigGetErr {
        request_id: u32,
        error: ProtocolError,
    },

    ServerConfigSet {
        request_id: u32,
        config: ServerConfig,
    },
    ServerConfigSetOk {
        request_id: u32,
        config: ServerConfig,
    },
    ServerConfigSetErr {
        request_id: u32,
        error: ProtocolError,
    },

    TerminalExit {
        terminal: TerminalId,
        exit: ExitInfo,
    },

    // ---- Data plane (per terminal stream) ----
    Output {
        stream: StreamId,
        seq: StreamSeq,
        bytes: ByteBuf,
    },
    Input {
        stream: StreamId,
        bytes: ByteBuf,
    },
    Resize {
        stream: StreamId,
        cols: u16,
        rows: u16,
    },

    // ---- Flow control ----
    Window {
        stream: StreamId,
        credit: u32,
    },
}

impl Frame {
    #[must_use]
    pub fn body(body: FrameBody) -> Self {
        Self { body }
    }
}
