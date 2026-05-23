#![cfg(not(target_arch = "wasm32"))]

//! Native-session scaffold. Keep this ignored until we wire a real mock daemon
//! replay against the current SessionBuilder / SessionHandle APIs.

use cli_pocket_tauri_bindings::SessionHandle;
use tokio::sync::mpsc;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a mock daemon replay against current native session APIs"]
async fn native_session_scaffold_compiles() {
    let (event_tx, _event_rx) = mpsc::channel(1);
    let handle = SessionHandle::spawn(event_tx);
    assert!(!handle.is_connected().await);
}
