use crate::state::AppState;
use std::sync::Mutex;
use tauri::State;

#[tauri::command]
pub async fn connect(
    state: State<'_, Mutex<AppState>>,
    _config: serde_json::Value,
) -> Result<(), String> {
    // TODO: Parse config and build SessionBuilder, then call state.session.connect(builder)
    let _ = state;
    Err("unimplemented: need to parse config and build SessionBuilder".into())
}

#[tauri::command]
pub async fn create_terminal(
    state: State<'_, Mutex<AppState>>,
    params: serde_json::Value,
) -> Result<(), String> {
    let params: cli_pocket_proto::TerminalCreateParams =
        serde_json::from_value(params).map_err(|e| e.to_string())?;

    let session = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.session.clone()
    };

    session.create_terminal(params).await
}

#[tauri::command]
pub async fn send_input(
    _state: State<'_, Mutex<AppState>>,
    _terminal_id: String,
    _bytes_b64: String,
) -> Result<(), String> {
    // TODO: Implement via TerminalHandle from session
    Err("unimplemented: send_input requires terminal handle access".into())
}

#[tauri::command]
pub async fn resize(
    _state: State<'_, Mutex<AppState>>,
    _terminal_id: String,
    _cols: u16,
    _rows: u16,
) -> Result<(), String> {
    // TODO: Implement via TerminalHandle from session
    Err("unimplemented: resize requires terminal handle access".into())
}

#[tauri::command]
pub async fn kill(
    _state: State<'_, Mutex<AppState>>,
    _terminal_id: String,
    _signal: Option<String>,
) -> Result<(), String> {
    // TODO: Implement via TerminalHandle from session
    Err("unimplemented: kill requires terminal handle access".into())
}

#[tauri::command]
pub async fn export_identity(_state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    // TODO: Need access to identity from somewhere (kv store or session)
    Err("unimplemented: identity export requires kv store access".into())
}

#[tauri::command]
pub async fn import_identity(
    _state: State<'_, Mutex<AppState>>,
    _blob: String,
) -> Result<(), String> {
    // TODO: Need access to kv store
    Err("unimplemented: identity import requires kv store access".into())
}

#[tauri::command]
pub async fn close(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let session = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.session.clone()
    };

    session.shutdown().await;
    Ok(())
}
