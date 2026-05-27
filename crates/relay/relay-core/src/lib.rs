//! Relay core: bridges daemons (hosts) and clients with bounded capacity.

pub mod caps;
pub mod config;
pub mod forward;
pub mod guillotine;
pub mod http;
pub mod metrics;
pub mod pairs;
pub mod registry;
pub mod server;

use cli_pocket_proto::codec::{encode_relay_ctrl, encode_relay_data};
use cli_pocket_proto::{RelayCtrl, RelayData};
pub use config::RelayConfig;
pub use server::RelayServer;

#[must_use]
pub fn version_banner() -> &'static str {
    "cli-pocket-relay (scaffold)"
}

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("transport: {0}")]
    Transport(#[from] cli_pocket_transport::TransportError),
    #[error("over-capacity: {0}")]
    OverCapacity(&'static str),
    #[error("protocol: {0}")]
    Protocol(&'static str),
    #[error("internal: {0}")]
    Internal(String),
}

pub type RelayResult<T> = std::result::Result<T, RelayError>;

#[must_use]
pub fn encode_ctrl_frame(ctrl: &RelayCtrl) -> Vec<u8> {
    encode_relay_ctrl(ctrl).expect("relay ctrl encoding should be infallible for valid enums")
}

#[must_use]
pub fn encode_data_frame(data: &RelayData) -> Vec<u8> {
    encode_relay_data(data).expect("relay data encoding should be infallible for valid enums")
}

pub fn codec_err_to_protocol(err: &cli_pocket_proto::CodecError) -> RelayError {
    RelayError::Internal(format!("relay codec error: {err}"))
}
