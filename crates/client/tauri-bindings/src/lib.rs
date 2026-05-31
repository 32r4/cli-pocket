//! Native bindings for `cli-pocket-client-core`: Transport, Clock, Rng, and
//! KeyValueStore implementations suitable for desktop and Tauri-mobile builds.

pub mod clock;
pub mod kv_store;
pub mod rng;
pub mod session_handle;
pub mod transport;

pub use clock::TokioClock;
pub use kv_store::FileKvStore;
pub use rng::OsRandom;
pub use session_handle::{LocalSpawner, SessionEvent, SessionHandle};
pub use transport::TokioWsTransport;

// Re-export ClientEvent for convenience in event_pump
pub use cli_pocket_client_core::ClientEvent;
