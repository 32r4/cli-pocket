//! Platform-agnostic client core for cli-pocket.
//!
//! Build native: `cargo check -p cli-pocket-client-core`
//! Build wasm:   `cargo check -p cli-pocket-client-core --target wasm32-unknown-unknown`

pub mod error;
pub mod events;
pub mod identity;
pub mod reconnect;
pub mod relay;
pub mod session;
pub mod snapshot;
pub mod terminal;
pub mod traits;

pub use error::{ClientError, ClientResult};
pub use events::ClientEvent;
pub use identity::ClientIdentity;
pub use session::{ClientSession, SessionBuilder, SessionConfig, SessionEndpoint};
pub use snapshot::TerminalSnapshot;
pub use terminal::TerminalHandle;
pub use traits::{Clock, KeyValueStore, Rng, Transport};
