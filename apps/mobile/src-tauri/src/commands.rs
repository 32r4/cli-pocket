use crate::AppState;
use cli_pocket_tauri_app::commands as shared_commands;
use tauri::State;

const MOBILE_WS_SUBPROTOCOL: Option<&str> = Some("cli-pocket-server/v1");

#[tauri::command]
pub async fn cli_pocket_connect(
    state: State<'_, AppState>,
    config: serde_json::Value,
    event_channel: String,
) -> Result<(), String> {
    shared_commands::connect(
        state.session(),
        state.kv(),
        config,
        event_channel,
        MOBILE_WS_SUBPROTOCOL,
    )
    .await
}

#[tauri::command]
pub async fn cli_pocket_create_terminal(
    state: State<'_, AppState>,
    params: serde_json::Value,
) -> Result<(), String> {
    shared_commands::create_terminal(state.session(), params).await
}

#[tauri::command]
pub async fn cli_pocket_open_terminal(
    state: State<'_, AppState>,
    terminal_id: String,
) -> Result<serde_json::Value, String> {
    shared_commands::open_terminal(state.session(), terminal_id).await
}

#[tauri::command]
pub async fn cli_pocket_list_terminals(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    shared_commands::list_terminals(state.session()).await
}

#[tauri::command]
pub async fn cli_pocket_read_history(
    state: State<'_, AppState>,
    terminal_id: String,
    before: Option<u64>,
    max_bytes: u32,
) -> Result<serde_json::Value, String> {
    shared_commands::read_history(state.session(), terminal_id, before, max_bytes).await
}

#[tauri::command]
pub async fn cli_pocket_send_input(
    state: State<'_, AppState>,
    terminal_id: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    shared_commands::send_input(state.session(), terminal_id, bytes).await
}

#[tauri::command]
pub async fn cli_pocket_resize(
    state: State<'_, AppState>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    shared_commands::resize(state.session(), terminal_id, cols, rows).await
}

#[tauri::command]
pub async fn cli_pocket_kill(
    state: State<'_, AppState>,
    terminal_id: String,
    signal: Option<String>,
) -> Result<(), String> {
    shared_commands::kill(state.session(), terminal_id, signal).await
}

#[tauri::command]
pub async fn cli_pocket_export_identity(state: State<'_, AppState>) -> Result<Vec<u8>, String> {
    shared_commands::export_identity(state.kv()).await
}

#[tauri::command]
pub async fn cli_pocket_import_identity(
    state: State<'_, AppState>,
    blob: Vec<u8>,
) -> Result<(), String> {
    shared_commands::import_identity(state.kv(), blob).await
}

#[tauri::command]
pub async fn cli_pocket_close(state: State<'_, AppState>) -> Result<(), String> {
    shared_commands::close(state.session()).await
}

#[tauri::command]
pub async fn cli_pocket_load_daemon_registry(
    state: State<'_, AppState>,
) -> Result<Option<serde_json::Value>, String> {
    shared_commands::load_daemon_registry(state.kv()).await
}

#[tauri::command]
pub async fn cli_pocket_save_daemon_registry(
    app_state: State<'_, AppState>,
    state: serde_json::Value,
) -> Result<(), String> {
    shared_commands::save_daemon_registry(app_state.kv(), state).await
}
