//! Daemon core: session manager, Noise responder, terminal routing.

pub mod client_db;
pub mod config;
pub mod connection;
pub mod handshake;
pub mod identity_store;
pub mod listener;
pub mod relay_dialer;
pub mod resume;
pub mod server;
pub mod session;

pub use config::DaemonConfig;
pub use server::Daemon;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("identity: {0}")]
    Identity(String),
    #[error("client-db: {0}")]
    ClientDb(String),
    #[error("crypto: {0}")]
    Crypto(#[from] cli_pocket_crypto::NoiseError),
    #[error("transport: {0}")]
    Transport(#[from] cli_pocket_transport::TransportError),
    #[error("proto: {0}")]
    Proto(#[from] cli_pocket_proto::CodecError),
    #[error("pty: {0}")]
    Pty(#[from] cli_pocket_pty::TerminalError),
    #[error("revoked: client {0}")]
    Revoked(String),
    #[error("not paired: client {0}")]
    NotPaired(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type DaemonResult<T> = std::result::Result<T, DaemonError>;

#[must_use]
pub fn version_banner() -> String {
    format!(
        "cli-pocket-daemon (scaffold proto v{})",
        cli_pocket_proto::SCAFFOLD_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_mentions_proto_version() {
        assert!(version_banner().contains("proto v0"));
    }
}
