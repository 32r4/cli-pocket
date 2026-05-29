mod commands;
mod deep_link;
mod event_pump;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let (app_state, event_rx) = AppState::new().expect("desktop app state should initialize");

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::cli_pocket_connect,
            commands::cli_pocket_create_terminal,
            commands::cli_pocket_send_input,
            commands::cli_pocket_resize,
            commands::cli_pocket_kill,
            commands::cli_pocket_export_identity,
            commands::cli_pocket_import_identity,
            commands::cli_pocket_close,
            commands::cli_pocket_local_daemon_endpoint,
            commands::cli_pocket_daemon_pair_url,
            commands::cli_pocket_daemon_restart,
        ])
        .setup(move |app| {
            let daemon = app.state::<AppState>().daemon.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = daemon.start().await {
                    tracing::error!("embedded daemon failed to start: {error}");
                }
            });
            event_pump::start(app.handle().clone(), event_rx);
            deep_link::install(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
