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
        ])
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| std::io::Error::other(format!("resolve app data dir: {error}")))?;
            let (app_state, event_rx) = AppState::new_at(&app_data_dir).map_err(|error| {
                std::io::Error::other(format!("mobile app state should initialize: {error}"))
            })?;

            app.manage(app_state);
            event_pump::start(app.handle().clone(), event_rx);
            deep_link::install(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
