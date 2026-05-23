//! `crypto.getRandomValues` -> [`Rng`] adapter.
//!
//! On wasm32, `getrandom` with the `js` feature routes through
//! `Crypto.getRandomValues` (in browsers / web workers) or
//! `crypto.randomFillSync` (in Node). That single dependency already does
//! the right thing in every environment we ship to, so the adapter is a
//! thin shim.
//!
//! Failure to fetch entropy is treated as fatal: panicking here matches the
//! native side (and Plan F's spec text), where the underlying OS RNG is
//! assumed to be available.

use cli_pocket_client_core::Rng;

/// [`Rng`] backed by the browser's `crypto.getRandomValues`.
///
/// Stateless — safe to construct on demand.
#[allow(dead_code)] // Wired into the public JS API by Task F13.
pub struct CryptoRng;

impl Rng for CryptoRng {
    fn fill(&self, dest: &mut [u8]) {
        getrandom::getrandom(dest).expect("crypto.getRandomValues");
    }
}
