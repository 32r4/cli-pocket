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

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::connect,
            commands::create_terminal,
            commands::send_input,
            commands::resize,
            commands::kill,
            commands::export_identity,
            commands::import_identity,
            commands::close,
        ])
        .setup(|app| {
            event_pump::start(app.handle().clone());
            deep_link::install(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
