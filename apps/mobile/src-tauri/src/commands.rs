use cli_pocket_tauri_app::commands as shared_commands;
use cli_pocket_tauri_app::ClientRuntimeState;
use tauri::State;

const MOBILE_WS_SUBPROTOCOL: Option<&str> = Some("cli-pocket-server/v1");

#[tauri::command]
pub async fn cli_pocket_connect(
    state: State<'_, ClientRuntimeState>,
    config: serde_json::Value,
) -> Result<(), String> {
    shared_commands::connect(state.session(), state.kv(), config, MOBILE_WS_SUBPROTOCOL).await
}

#[tauri::command]
pub async fn cli_pocket_create_terminal(
    state: State<'_, ClientRuntimeState>,
    params: serde_json::Value,
) -> Result<(), String> {
    shared_commands::create_terminal(state.session(), params).await
}

#[tauri::command]
pub async fn cli_pocket_send_input(
    state: State<'_, ClientRuntimeState>,
    terminal_id: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    shared_commands::send_input(state.session(), terminal_id, bytes).await
}

#[tauri::command]
pub async fn cli_pocket_resize(
    state: State<'_, ClientRuntimeState>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    shared_commands::resize(state.session(), terminal_id, cols, rows).await
}

#[tauri::command]
pub async fn cli_pocket_kill(
    state: State<'_, ClientRuntimeState>,
    terminal_id: String,
    signal: Option<String>,
) -> Result<(), String> {
    shared_commands::kill(state.session(), terminal_id, signal).await
}

#[tauri::command]
pub async fn cli_pocket_export_identity(
    state: State<'_, ClientRuntimeState>,
) -> Result<Vec<u8>, String> {
    shared_commands::export_identity(state.kv()).await
}

#[tauri::command]
pub async fn cli_pocket_import_identity(
    state: State<'_, ClientRuntimeState>,
    blob: Vec<u8>,
) -> Result<(), String> {
    shared_commands::import_identity(state.kv(), blob).await
}

#[tauri::command]
pub async fn cli_pocket_close(state: State<'_, ClientRuntimeState>) -> Result<(), String> {
    shared_commands::close(state.session()).await
}

#[tauri::command]
pub async fn cli_pocket_load_daemon_registry(
    state: State<'_, ClientRuntimeState>,
) -> Result<Option<serde_json::Value>, String> {
    shared_commands::load_daemon_registry(state.kv()).await
}

#[tauri::command]
pub async fn cli_pocket_save_daemon_registry(
    app_state: State<'_, ClientRuntimeState>,
    state: serde_json::Value,
) -> Result<(), String> {
    shared_commands::save_daemon_registry(app_state.kv(), state).await
}
