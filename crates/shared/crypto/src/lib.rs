//! Noise XK + SPAKE2 + identity. See § Section 5.

pub mod identity;
pub mod noise;
pub mod redact;
pub mod spake2;

pub use identity::{Identity, IdentityError, KeyBytes32, KeyPair};
pub use noise::{
    NoiseAnonymousInitiator, NoiseAnonymousResponder, NoiseError, NoiseInitiator, NoiseResponder,
    NoiseSession,
};
pub use redact::Secret;
pub use spake2::{Spake2Error, Spake2Outcome, Spake2Side};
