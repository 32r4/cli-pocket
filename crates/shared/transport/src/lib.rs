//! Binary-framed WebSocket transport abstraction. See Section 3 / Section 6.

pub mod memory;
pub mod transport;

pub use memory::{InMemoryTransport, InMemoryTransportPair};
pub use transport::{Transport, TransportError};
