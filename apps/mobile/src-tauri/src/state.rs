use std::path::Path;

use cli_pocket_client_core::ClientEvent;
use cli_pocket_tauri_bindings::{FileKvStore, SessionHandle};
use tokio::sync::mpsc;

pub struct AppState {
    pub session: SessionHandle,
    pub kv: FileKvStore,
}

impl AppState {
    pub fn new_at(data_dir: &Path) -> Result<(Self, mpsc::Receiver<ClientEvent>), String> {
        let kv = FileKvStore::open_at(data_dir).map_err(|error| error.to_string())?;
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>(64);
        let session = SessionHandle::spawn(event_tx);

        Ok((Self { session, kv }, event_rx))
    }
}
