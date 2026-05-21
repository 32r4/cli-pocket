use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ProtocolError {
    #[error("unknown terminal")]
    UnknownTerminal,
    #[error("unauthorized")]
    Unauthorized,
    #[error("backpressure exceeded")]
    BackpressureExceeded,
    #[error("protocol mismatch")]
    ProtocolMismatch,
    #[error("resource exhausted")]
    ResourceExhausted,
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[error("resume stale")]
    ResumeStale,
    #[error("rate limited")]
    RateLimited,
    /// Catchall for protocol-level errors that are intentionally surfaced as
    /// `Other` in this version of the wire format.
    #[error("other: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByeReason {
    Normal,
    Revoked,
    ServerShutdown,
    ProtocolError(ProtocolError),
}
