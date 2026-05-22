//! PTY + scrollback boundary. See ADR 0002.

pub mod output;
pub mod parser;
pub mod ring;
pub mod terminal;

mod platform;

pub use output::{Lagged, OutputBroadcaster, OutputChunk, OutputRecv, OutputStream};
pub use parser::AnchorTracker;
pub use ring::{RingError, ScrollbackRing};
pub use terminal::{Terminal, TerminalError};
