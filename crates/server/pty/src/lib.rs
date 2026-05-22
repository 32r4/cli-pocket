//! PTY + scrollback boundary. See ADR 0002.

pub mod parser;
pub mod ring;

mod platform;

pub use parser::AnchorTracker;
pub use ring::{RingError, ScrollbackRing};
