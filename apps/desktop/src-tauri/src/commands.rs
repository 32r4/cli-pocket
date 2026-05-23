use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn connect(
    _state: State<'_, AppState>,
    _config: serde_json::Value,
) -> Result<(), String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn create_terminal(
    _state: State<'_, AppState>,
    _params: serde_json::Value,
) -> Result<(), String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn send_input(
    _state: State<'_, AppState>,
    _terminal_id: String,
    _bytes_b64: String,
) -> Result<(), String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn resize(
    _state: State<'_, AppState>,
    _terminal_id: String,
    _cols: u16,
    _rows: u16,
) -> Result<(), String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn kill(
    _state: State<'_, AppState>,
    _terminal_id: String,
    _signal: Option<String>,
) -> Result<(), String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn export_identity(_state: State<'_, AppState>) -> Result<String, String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn import_identity(
    _state: State<'_, AppState>,
    _blob: String,
) -> Result<(), String> {
    Err("unimplemented".into())
}

#[tauri::command]
pub async fn close(_state: State<'_, AppState>) -> Result<(), String> {
    Err("unimplemented".into())
}
