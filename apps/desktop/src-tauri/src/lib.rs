mod commands;
mod state;

use cli_pocket_tauri_app::{install_app_hooks, install_tracing};
use rustls::crypto::aws_lc_rs;
use state::{embedded_daemon_enabled, new_app_state, AppState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_rustls_crypto_provider();
    install_tracing();

    let (app_state, event_rx) = new_app_state().expect("desktop app state should initialize");

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::cli_pocket_connect,
            commands::cli_pocket_create_terminal,
            commands::cli_pocket_activate_terminal,
            commands::cli_pocket_list_terminals,
            commands::cli_pocket_read_history,
            commands::cli_pocket_get_server_config,
            commands::cli_pocket_set_server_config,
            commands::cli_pocket_send_input,
            commands::cli_pocket_resize,
            commands::cli_pocket_kill,
            commands::cli_pocket_export_identity,
            commands::cli_pocket_import_identity,
            commands::cli_pocket_close,
            commands::cli_pocket_local_daemon_endpoint,
            commands::cli_pocket_daemon_pair_url,
            commands::cli_pocket_daemon_restart,
            commands::cli_pocket_load_daemon_registry,
            commands::cli_pocket_save_daemon_registry,
        ])
        .setup(move |app| {
            if embedded_daemon_enabled() {
                let daemon = app.state::<AppState>().daemon().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = daemon.start().await {
                        tracing::error!("embedded daemon failed to start: {error}");
                    }
                });
            }
            install_app_hooks(&app.handle().clone(), event_rx);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}

fn install_rustls_crypto_provider() {
    let _ = aws_lc_rs::default_provider().install_default();
}
