pub mod commands;
pub mod deep_link;
pub mod event_pump;

use cli_pocket_client_core::ClientEvent;
use cli_pocket_tauri_bindings::SessionHandle;
use tokio::sync::mpsc;

pub fn spawn_session_runtime() -> (SessionHandle, mpsc::Receiver<ClientEvent>) {
    let (event_tx, event_rx) = mpsc::channel::<ClientEvent>(64);
    let session = SessionHandle::spawn(event_tx);
    (session, event_rx)
}
