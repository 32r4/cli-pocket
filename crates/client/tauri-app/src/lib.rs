pub mod commands;
pub mod deep_link;
pub mod event_pump;

use std::path::{Path, PathBuf};

use cli_pocket_client_core::ClientEvent;
use cli_pocket_tauri_bindings::{FileKvStore, SessionHandle};
use tauri::{App, AppHandle, Manager, Runtime};
use tokio::sync::mpsc;

pub fn spawn_session_runtime() -> (SessionHandle, mpsc::Receiver<ClientEvent>) {
    let (event_tx, event_rx) = mpsc::channel::<ClientEvent>(64);
    let session = SessionHandle::spawn(event_tx);
    (session, event_rx)
}

pub struct ClientRuntimeState {
    session: SessionHandle,
    kv: FileKvStore,
}

impl ClientRuntimeState {
    pub fn new_at(data_dir: &Path) -> Result<(Self, mpsc::Receiver<ClientEvent>), String> {
        let kv = FileKvStore::open_at(data_dir).map_err(|error| error.to_string())?;
        let (session, event_rx) = spawn_session_runtime();

        Ok((Self { session, kv }, event_rx))
    }

    pub fn session(&self) -> SessionHandle {
        self.session.clone()
    }

    pub fn kv(&self) -> FileKvStore {
        self.kv.clone()
    }
}

pub struct ManagedAppState<D> {
    client: ClientRuntimeState,
    daemon: D,
}

impl<D> ManagedAppState<D> {
    pub fn new_at(
        data_dir: &Path,
        daemon: D,
    ) -> Result<(Self, mpsc::Receiver<ClientEvent>), String> {
        let (client, event_rx) = ClientRuntimeState::new_at(data_dir)?;

        Ok((Self { client, daemon }, event_rx))
    }

    pub fn session(&self) -> SessionHandle {
        self.client.session()
    }

    pub fn kv(&self) -> FileKvStore {
        self.client.kv()
    }

    pub fn daemon(&self) -> &D {
        &self.daemon
    }
}

pub fn resolve_app_data_dir<R: Runtime>(app: &App<R>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("resolve app data dir: {error}"))
}

pub fn install_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

pub fn install_app_hooks(app: &AppHandle, event_rx: mpsc::Receiver<ClientEvent>) {
    event_pump::start(app.clone(), event_rx);
    deep_link::install(app);
}
