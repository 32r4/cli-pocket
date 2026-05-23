mod commands;
mod deep_link;
mod event_pump;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let (app_state, event_rx) = AppState::new().expect("mobile app state should initialize");

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
        ])
        .setup(move |app| {
            event_pump::start(app.handle().clone(), event_rx);
            deep_link::install(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
