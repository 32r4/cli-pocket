//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod identity;
pub mod noise;
pub mod redact;

pub use identity::{Identity, IdentityError, KeyBytes32, KeyPair};
pub use noise::{NoiseError, NoiseInitiator, NoiseResponder, NoiseSession};
pub use redact::Secret;
