use cli_pocket_proto::ByeReason;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("proto: {0}")]
    Proto(String),
    #[error("identity store: {0}")]
    Identity(String),
    #[error("rejected: {0:?}")]
    Rejected(ByeReason),
    #[error("not connected")]
    NotConnected,
    #[error("terminal not attached")]
    NoTerminal,
    #[error("backend closed")]
    Closed,
    #[error("connection timed out after {timeout_ms}ms")]
    ConnectTimeout { timeout_ms: u64 },
    #[error("internal: {0}")]
    Internal(String),
}

pub type ClientResult<T> = std::result::Result<T, ClientError>;

impl From<cli_pocket_crypto::IdentityError> for ClientError {
    fn from(value: cli_pocket_crypto::IdentityError) -> Self {
        Self::Crypto(value.to_string())
    }
}

impl From<cli_pocket_crypto::NoiseError> for ClientError {
    fn from(value: cli_pocket_crypto::NoiseError) -> Self {
        Self::Crypto(value.to_string())
    }
}

impl From<cli_pocket_crypto::Spake2Error> for ClientError {
    fn from(value: cli_pocket_crypto::Spake2Error) -> Self {
        Self::Crypto(value.to_string())
    }
}

impl From<cli_pocket_proto::CodecError> for ClientError {
    fn from(value: cli_pocket_proto::CodecError) -> Self {
        Self::Proto(value.to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<cli_pocket_transport::TransportError> for ClientError {
    fn from(value: cli_pocket_transport::TransportError) -> Self {
        Self::Transport(value.to_string())
    }
}
