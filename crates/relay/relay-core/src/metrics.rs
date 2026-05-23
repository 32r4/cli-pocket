//! Prometheus metrics for the relay.
//!
//! [`init`] installs a process-wide Prometheus recorder and describes the
//! relay's counters and gauges. The returned [`PrometheusHandle`] renders
//! the current snapshot for the `/metrics` route in `http.rs`.
//!
//! Metric names follow the `cli_pocket_relay_*` prefix to keep them
//! distinct from other services scraped by the same Prometheus.

use metrics::{describe_counter, describe_gauge};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Install the Prometheus recorder and register relay metrics.
///
/// Must be called exactly once at process startup. The returned handle is
/// cheap to clone and is shared with the `/metrics` HTTP handler.
///
/// # Panics
///
/// Panics if a recorder has already been installed in this process; that
/// would indicate a double-init bug in the caller.
#[must_use]
pub fn init() -> PrometheusHandle {
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
}
