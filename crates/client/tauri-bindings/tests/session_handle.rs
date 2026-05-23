use cli_pocket_tauri_bindings::SessionHandle;
use tokio::sync::mpsc;

#[tokio::test(flavor = "current_thread")]
async fn build_handle_compiles() {
    let (event_tx, _event_rx) = mpsc::channel(1);
    let h = SessionHandle::spawn(event_tx);
    assert!(!h.is_connected().await);
}
