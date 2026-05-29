use cli_pocket_client_core::ClientEvent;
use cli_pocket_daemon_core::config::{default_config_path, workspace_root};
use cli_pocket_daemon_core::service::{
    build_config_template, dev_config_template, load_or_create_config_with_template,
};
use cli_pocket_daemon_core::{Daemon, DaemonConfig};
use cli_pocket_tauri_app::spawn_session_runtime;
use cli_pocket_tauri_bindings::{FileKvStore, SessionHandle};
use std::path::PathBuf;
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
        let daemon_config = load_or_create_config_with_template(
            desktop_daemon_config_path(),
            desktop_daemon_template(),
        )
        .map_err(|error| error.to_string())?;
        let kv = FileKvStore::open_at(desktop_store_dir(&daemon_config))
            .map_err(|error| error.to_string())?;
        let (session, event_rx) = spawn_session_runtime();

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

fn desktop_daemon_template() -> &'static str {
    if cfg!(debug_assertions) {
        dev_config_template()
    } else {
        build_config_template()
    }
}

fn desktop_daemon_config_path() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_root().join("crates/server/daemon-bin/daemon.dev.toml")
    } else {
        default_config_path()
    }
}

fn desktop_store_dir(daemon_config: &DaemonConfig) -> &std::path::Path {
    daemon_config
        .security
        .identity_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
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
