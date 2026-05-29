use cli_pocket_client_core::ClientEvent;
use cli_pocket_daemon_core::service::load_or_create_config;
use cli_pocket_daemon_core::{Daemon, DaemonConfig};
use cli_pocket_tauri_bindings::{FileKvStore, SessionHandle};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

struct EmbeddedDaemonState {
    config: DaemonConfig,
    daemon: Option<Daemon>,
}

#[derive(Clone)]
pub struct EmbeddedDaemonRuntime {
    inner: Arc<Mutex<EmbeddedDaemonState>>,
}

pub struct AppState {
    pub session: SessionHandle,
    pub kv: FileKvStore,
    pub daemon: EmbeddedDaemonRuntime,
}

impl AppState {
    pub fn new() -> Result<(Self, mpsc::Receiver<ClientEvent>), String> {
        let kv = FileKvStore::open_default().map_err(|error| error.to_string())?;
        let daemon_config = load_or_create_config(None).map_err(|error| error.to_string())?;
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>(64);
        let session = SessionHandle::spawn(event_tx);

        Ok((
            Self {
                session,
                kv,
                daemon: EmbeddedDaemonRuntime {
                    inner: Arc::new(Mutex::new(EmbeddedDaemonState {
                        config: daemon_config,
                        daemon: None,
                    })),
                },
            },
            event_rx,
        ))
    }
}

impl EmbeddedDaemonRuntime {
    pub async fn start(&self) -> Result<(), String> {
        let mut state = self.inner.lock().await;
        if state.daemon.is_some() {
            return Ok(());
        }

        let mut daemon = Daemon::boot(state.config.clone())
            .await
            .map_err(|error| error.to_string())?;
        daemon.start().await.map_err(|error| error.to_string())?;
        state.daemon = Some(daemon);
        Ok(())
    }

    pub async fn restart(&self) -> Result<(), String> {
        let mut state = self.inner.lock().await;
        if let Some(daemon) = state.daemon.take() {
            daemon.shutdown().await;
        }

        let mut daemon = Daemon::boot(state.config.clone())
            .await
            .map_err(|error| error.to_string())?;
        daemon.start().await.map_err(|error| error.to_string())?;
        state.daemon = Some(daemon);
        Ok(())
    }

    pub async fn pair_url(&self) -> Result<String, String> {
        let mut state = self.inner.lock().await;
        if state.daemon.is_none() {
            let mut daemon = Daemon::boot(state.config.clone())
                .await
                .map_err(|error| error.to_string())?;
            daemon.start().await.map_err(|error| error.to_string())?;
            state.daemon = Some(daemon);
        }

        state
            .daemon
            .as_ref()
            .ok_or_else(|| "daemon not running".to_owned())?
            .pair_url()
            .map_err(|error| error.to_string())
    }

    pub async fn local_endpoint_url(&self) -> Result<String, String> {
        self.start().await?;

        let state = self.inner.lock().await;
        Ok(format!(
            "ws://127.0.0.1:{}/session",
            state.config.listen.port
        ))
    }
}
