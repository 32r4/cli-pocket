use crate::error::{ByeReason, ProtocolError};
use crate::hello::{Hello, HelloOk};
use crate::terminal::ServerConfig;
use crate::terminal::{
    RequestId, StreamId, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo,
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
    pub body: RequestBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestBody {
    ListTerminals,
    CreateTerminal {
        params: TerminalCreateParams,
    },
    OpenTerminal {
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
    pub result: Result<ResponseBody, ResponseError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseBody {
    ListTerminals { terminals: Vec<TerminalInfo> },
    CreateTerminal { info: TerminalInfo },
    OpenTerminal { ack: OpenTerminalAck },
    ReadHistory { page: HistoryPage },
    KillTerminal,
    GetServerConfig { config: ServerConfig },
    SetServerConfig { config: ServerConfig },
    SendInput,
    ResizeTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenTerminalAck {
    pub stream_id: StreamId,
    pub info: TerminalInfo,
    pub start_seq: StreamSeq,
    pub end_seq: StreamSeq,
    pub render_bytes: ByteBuf,
    pub has_more_history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub terminal_id: TerminalId,
    pub start_seq: StreamSeq,
    pub end_seq: StreamSeq,
    pub bytes: ByteBuf,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: ProtocolError,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDataFrame {
    pub stream_id: StreamId,
    pub seq: StreamSeq,
    pub offset: Option<u32>,
    pub bytes: ByteBuf,
    pub last: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFrame {
    pub body: EventBody,
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
    Error {
        error: ProtocolError,
        message: String,
    },
}
