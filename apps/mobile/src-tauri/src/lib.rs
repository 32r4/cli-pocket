mod commands;

use cli_pocket_tauri_app::{install_app_hooks, install_tracing, ClientRuntimeState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![
            commands::cli_pocket_connect,
            commands::cli_pocket_create_terminal,
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
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| std::io::Error::other(format!("resolve app data dir: {error}")))?;
            let (app_state, event_rx) =
                ClientRuntimeState::new_at(&app_data_dir).map_err(|error| {
                    std::io::Error::other(format!("mobile app state should initialize: {error}"))
                })?;

            app.manage(app_state);
            install_app_hooks(&app.handle().clone(), event_rx);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
