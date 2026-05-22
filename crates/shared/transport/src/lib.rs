//! Binary-framed WebSocket transport abstraction. See Section 3 / Section 6.

pub mod transport;

pub use transport::{Transport, TransportError};
