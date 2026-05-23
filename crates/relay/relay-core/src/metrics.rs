//! Prometheus metrics for the relay.
//!
//! [`init`] installs a process-wide Prometheus recorder and describes the
//! relay's counters and gauges. The returned [`PrometheusHandle`] renders
//! the current snapshot for the `/metrics` route in `http.rs`.
//!
//! Metric names follow the `cli_pocket_relay_*` prefix to keep them
//! distinct from other services scraped by the same Prometheus.

use std::sync::OnceLock;

use metrics::{describe_counter, describe_gauge};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus recorder and register relay metrics.
///
/// Safe to call repeatedly: the first call installs the global recorder and
/// caches the handle in a [`OnceLock`]; subsequent calls return a clone of
/// the cached handle. This makes the function usable from integration tests
/// that build several `RelayServer`s inside the same process.
#[must_use]
pub fn init() -> PrometheusHandle {
    HANDLE
        .get_or_init(|| {
            let handle = PrometheusBuilder::new()
                .install_recorder()
                .expect("install prometheus recorder");

            describe_counter!(
                "cli_pocket_relay_pairs_total",
                "Total pairs ever created since the relay started."
            );
            describe_counter!(
                "cli_pocket_relay_pair_close_total",
                "Total pairs closed, labelled by `reason` (normal, host_gone, client_gone, stuck, ...)."
            );
            describe_counter!(
                "cli_pocket_relay_bytes_total",
                "Total ciphertext bytes forwarded, labelled by `direction` (host_to_client, client_to_host)."
            );
            describe_gauge!(
                "cli_pocket_relay_hosts_current",
                "Currently registered hosts."
            );
            describe_gauge!("cli_pocket_relay_pairs_current", "Currently live pairs.");

            handle
        })
        .clone()
}
