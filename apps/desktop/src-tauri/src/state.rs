use cli_pocket_daemon_core::config::{default_config_path, workspace_root};
use cli_pocket_daemon_core::service::{
    build_config_template, dev_config_template, load_or_create_config_with_template,
};
use cli_pocket_daemon_core::{Daemon, DaemonConfig};
use cli_pocket_tauri_app::ManagedAppState;
use cli_pocket_tauri_bindings::SessionEvent;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

struct EmbeddedDaemonState {
    config: DaemonConfig,
    daemon: Option<Daemon>,
}

#[derive(Clone)]
pub struct EmbeddedDaemonRuntime {
    enabled: bool,
    inner: Arc<Mutex<EmbeddedDaemonState>>,
}

pub type AppState = ManagedAppState<EmbeddedDaemonRuntime>;

pub fn new_app_state() -> Result<(AppState, mpsc::Receiver<SessionEvent>), String> {
    let daemon_config = load_or_create_config_with_template(
        desktop_daemon_config_path(),
        desktop_daemon_template(),
    )
    .map_err(|error| error.to_string())?;
    let client_data_dir = desktop_store_dir(&daemon_config).to_path_buf();
    let daemon = EmbeddedDaemonRuntime::from_config(daemon_config);

    ManagedAppState::new_at(&client_data_dir, daemon)
}

pub fn embedded_daemon_enabled() -> bool {
    !cfg!(mobile)
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
    fn from_config(config: DaemonConfig) -> Self {
        Self {
            enabled: embedded_daemon_enabled(),
            inner: Arc::new(Mutex::new(EmbeddedDaemonState {
                config,
                daemon: None,
            })),
        }
    }

    fn ensure_enabled(&self) -> Result<(), String> {
        if self.enabled {
            return Ok(());
        }

        Err("embedded daemon is unavailable on mobile".to_owned())
    }

    pub async fn start(&self) -> Result<(), String> {
        self.ensure_enabled()?;

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
        self.ensure_enabled()?;

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
        self.ensure_enabled()?;

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
        self.ensure_enabled()?;
        self.start().await?;

        let state = self.inner.lock().await;
        Ok(format!(
            "ws://127.0.0.1:{}/session",
            state.config.listen.port
        ))
    }

    pub async fn server_label(&self) -> Result<Option<String>, String> {
        self.ensure_enabled()?;
        self.start().await?;

        let state = self.inner.lock().await;
        Ok(state
            .daemon
            .as_ref()
            .and_then(|daemon| daemon.server_info.server_label.clone()))
    }
}
