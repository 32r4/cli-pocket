//! PTY + scrollback boundary. See ADR 0002.

pub mod output;
pub mod parser;
pub mod ring;

mod platform;

pub use output::{Lagged, OutputBroadcaster, OutputChunk, OutputRecv, OutputStream};
pub use parser::AnchorTracker;
pub use ring::{RingError, ScrollbackRing};
