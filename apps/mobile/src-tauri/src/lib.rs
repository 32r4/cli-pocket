mod commands;

use cli_pocket_tauri_app::{
    install_app_hooks, install_tracing, resolve_app_data_dir, ManagedAppState,
};
use rustls::crypto::aws_lc_rs;
use tauri::Manager;

pub type AppState = ManagedAppState<()>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_rustls_crypto_provider();
    install_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![
            commands::cli_pocket_connect,
            commands::cli_pocket_create_terminal,
            commands::cli_pocket_open_terminal,
            commands::cli_pocket_list_terminals,
            commands::cli_pocket_send_input,
            commands::cli_pocket_resize,
            commands::cli_pocket_kill,
            commands::cli_pocket_export_identity,
            commands::cli_pocket_import_identity,
            commands::cli_pocket_close,
            commands::cli_pocket_load_daemon_registry,
            commands::cli_pocket_save_daemon_registry,
        ])
        .setup(|app| {
            let app_data_dir = resolve_app_data_dir(app).map_err(std::io::Error::other)?;
            let (app_state, event_rx) = AppState::new_at(&app_data_dir, ()).map_err(|error| {
                std::io::Error::other(format!("mobile app state should initialize: {error}"))
            })?;

            app.manage(app_state);
            install_app_hooks(&app.handle().clone(), event_rx);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}

fn install_rustls_crypto_provider() {
    let _ = aws_lc_rs::default_provider().install_default();
}
