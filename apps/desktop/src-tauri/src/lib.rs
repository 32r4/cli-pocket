mod commands;
mod deep_link;
mod event_pump;
mod state;

use state::AppState;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Create app state with the session handle
    let app_state = Mutex::new(AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .manage(app_state)
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
            // Take the event receiver from state and start the event pump
            {
                let state = app.state::<Mutex<AppState>>();
                let mut guard = state.lock().unwrap();
                if let Some(event_rx) = guard.take_event_rx() {
                    event_pump::start(app.handle().clone(), event_rx);
                }
            }
            deep_link::install(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
