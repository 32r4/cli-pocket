//! Wire protocol contracts. See docs/superpowers/specs/2026-05-21-cross-platform-remote-terminal-design.md § 2.

pub mod error;
pub mod frame;
pub mod hello;
pub mod relay;
pub mod snapshot;
pub mod terminal;

pub use error::{ByeReason, ProtocolError};
pub use frame::{Frame, FrameBody};
pub use hello::{
    Capabilities, ClientKind, Hello, HelloErr, HelloOk, ResumeAttachment, ResumeToken, ServerInfo,
};
pub use relay::{
    Endpoint, OfferId, PairCloseReason, PairId, RelayCtrl, RelayData, RELAY_DISC_CTRL,
    RELAY_DISC_DATA,
};
pub use snapshot::{
    AnchorState, CharsetState, Color, DeltaSlice, MouseMode, SgrAttrs, Snapshot, TerminalModes,
};
pub use terminal::{
    ClientId, ExitInfo, HostId, KillSignal, SessionId, StreamId, StreamSeq, TerminalCreateParams,
    TerminalId, TerminalInfo,
};

/// Wire protocol version negotiated in `Hello`.
pub const PROTOCOL_VERSION: u32 = 1;

/// Scaffold compatibility value used by crates not yet implemented in Plan B.
pub const SCAFFOLD_VERSION: u32 = 0;
