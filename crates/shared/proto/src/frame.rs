use crate::error::{ByeReason, ProtocolError};
use crate::hello::{Hello, HelloOk};
use crate::terminal::ServerConfig;
use crate::terminal::{
    ExitInfo, RequestId, StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo,
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
    Ping { nonce: u32 },
    Pong { nonce: u32 },
    Bye { reason: ByeReason },

    // ---- Terminal lifecycle (request/response, request_id paired) ----
    Request(RequestFrame),
    Response(ResponseFrame),
    StreamData(StreamDataFrame),
    Event(EventFrame),
}

impl Frame {
    #[must_use]
    pub fn body(body: FrameBody) -> Self {
        Self { body }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestFrame {
    pub id: RequestId,
    pub op: RequestOp,
    pub body: RequestBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestOp {
    ListTerminals,
    CreateTerminal,
    AttachTerminal,
    ReadHistory,
    KillTerminal,
    GetServerConfig,
    SetServerConfig,
    SendInput,
    ResizeTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestBody {
    ListTerminals,
    CreateTerminal {
        params: TerminalCreateParams,
    },
    AttachTerminal {
        terminal_id: TerminalId,
    },
    ReadHistory {
        terminal_id: TerminalId,
        before: Option<StreamSeq>,
        max_bytes: u32,
    },
    KillTerminal {
        terminal_id: TerminalId,
    },
    GetServerConfig,
    SetServerConfig {
        config: ServerConfig,
    },
    SendInput {
        terminal_id: TerminalId,
        bytes: ByteBuf,
    },
    ResizeTerminal {
        terminal_id: TerminalId,
        cols: u16,
        rows: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseFrame {
    pub id: RequestId,
    pub ok: bool,
    pub body: Option<ResponseBody>,
    pub error: Option<ResponseError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseBody {
    ListTerminals {
        terminals: Vec<TerminalInfo>,
    },
    CreateTerminal {
        info: TerminalInfo,
    },
    AttachTerminal {
        stream_id: StreamId,
        terminal_info: TerminalInfo,
        baseline_start_seq: StreamSeq,
        baseline_end_seq: StreamSeq,
        render_prefix: String,
    },
    ReadHistory {
        stream_id: StreamId,
        terminal_id: TerminalId,
        start_seq: StreamSeq,
        end_seq: StreamSeq,
    },
    KillTerminal,
    GetServerConfig {
        config: ServerConfig,
    },
    SetServerConfig {
        config: ServerConfig,
    },
    SendInput,
    ResizeTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: ProtocolError,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDataFrame {
    pub stream_id: StreamId,
    pub kind: StreamKind,
    pub seq: StreamSeq,
    pub offset: Option<u32>,
    pub bytes: ByteBuf,
    pub last: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Baseline,
    Output,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFrame {
    pub kind: EventKind,
    pub body: EventBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Connected,
    Disconnected,
    TerminalCreated,
    TerminalExited,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventBody {
    Connected,
    Disconnected {
        reason: String,
    },
    TerminalCreated {
        info: TerminalInfo,
    },
    TerminalExited {
        terminal_id: TerminalId,
        exit: ExitInfo,
    },
    Error {
        error: ProtocolError,
        message: String,
    },
}
