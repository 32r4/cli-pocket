//! PTY + scrollback boundary. See ADR 0002.

pub mod parser;

mod platform;

pub use parser::AnchorTracker;
