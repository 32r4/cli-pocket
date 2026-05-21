//! Wire protocol contracts. See docs/superpowers/specs/2026-05-21-cross-platform-remote-terminal-design.md § 2.

pub mod terminal;

pub use terminal::{
    ClientId, ExitInfo, HostId, KillSignal, SessionId, StreamId, StreamSeq, TerminalCreateParams,
    TerminalId, TerminalInfo,
};

/// Wire protocol version negotiated in `Hello`.
pub const PROTOCOL_VERSION: u32 = 1;

/// Scaffold compatibility value used by crates not yet implemented in Plan B.
pub const SCAFFOLD_VERSION: u32 = 0;
