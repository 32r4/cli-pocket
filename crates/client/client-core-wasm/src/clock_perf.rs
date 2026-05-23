//! `performance.now()` + `setTimeout` -> [`Clock`] adapter.
//!
//! The browser exposes a high-resolution monotonic-ish clock via
//! `Window.performance.now()`, returning a `f64` of milliseconds since the
//! page navigation. We truncate to `u64`, which is enough for reconnect
//! backoff and frame-level timeouts.
//!
//! `sleep_ms` is bridged through [`gloo_timers`], which wraps
//! `setTimeout`/`clearTimeout` behind a Future.

use async_trait::async_trait;
use cli_pocket_client_core::Clock;
use gloo_timers::future::TimeoutFuture;
use web_sys::window;

/// [`Clock`] backed by `Window.performance.now()` and `setTimeout`.
///
/// Stateless — safe to construct on demand.
#[allow(dead_code)] // Wired into the public JS API by Task F13.
pub struct PerfClock;

#[async_trait(?Send)]
impl Clock for PerfClock {
    fn now_ms(&self) -> u64 {
        // `window()` returns `None` outside a browser window (e.g. worker
        // bootstrap before the global is installed). Returning 0 in that case
        // matches the "monotonic-ish" contract well enough — callers compare
        // deltas, not absolute timestamps.
        let now = window()
            .and_then(|w| w.performance())
            .map_or(0.0_f64, |p| p.now());
        // `performance.now()` is non-negative milliseconds; saturating cast
        // pins overflow to `u64::MAX` rather than wrapping.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let ms = if now.is_finite() && now >= 0.0 {
            now as u64
        } else {
            0
        };
        ms
    }

    async fn sleep_ms(&self, ms: u64) {
        // `gloo_timers` accepts `u32` milliseconds; clamp to that range.
        // 49.7 days is well past any reconnect backoff this code performs.
        let ms = u32::try_from(ms).unwrap_or(u32::MAX);
        TimeoutFuture::new(ms).await;
    }
}
