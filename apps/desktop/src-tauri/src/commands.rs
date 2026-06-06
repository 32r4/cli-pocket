use crate::state::AppState;
use cli_pocket_tauri_app::commands as shared_commands;
use tauri::State;

#[tauri::command]
pub async fn cli_pocket_connect(
    state: State<'_, AppState>,
    config: serde_json::Value,
    event_channel: String,
) -> Result<(), String> {
    shared_commands::connect(state.session(), state.kv(), config, event_channel, None).await
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
pub async fn cli_pocket_get_server_config(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    shared_commands::get_server_config(state.session()).await
}

#[tauri::command]
pub async fn cli_pocket_set_server_config(
    state: State<'_, AppState>,
    config: serde_json::Value,
) -> Result<serde_json::Value, String> {
    shared_commands::set_server_config(state.session(), config).await
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
pub async fn cli_pocket_local_daemon_endpoint(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.daemon().local_endpoint_url().await
}

#[tauri::command]
pub async fn cli_pocket_daemon_pair_url(state: State<'_, AppState>) -> Result<String, String> {
    state.daemon().pair_url().await
}

#[tauri::command]
pub async fn cli_pocket_daemon_restart(state: State<'_, AppState>) -> Result<(), String> {
    state.daemon().restart().await
}

#[tauri::command]
pub async fn cli_pocket_load_daemon_registry(
    state: State<'_, AppState>,
) -> Result<Option<serde_json::Value>, String> {
    let endpoint_url = state.daemon().local_endpoint_url().await?;
    let label = state
        .daemon()
        .server_label()
        .await?
        .unwrap_or_else(|| "Local".to_owned());
    let mut registry = shared_commands::load_daemon_registry(state.kv())
        .await?
        .unwrap_or_else(|| {
            serde_json::json!({
                "version": 1,
                "daemons": [],
                "selectedDaemonId": null,
            })
        });
    let Some(daemons) = registry
        .get_mut("daemons")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(Some(registry));
    };

    if let Some(daemon) = daemons.iter_mut().find(|daemon| {
        daemon
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id == "local-daemon")
    }) {
        daemon["label"] = serde_json::Value::String(label);
        daemon["endpointUrl"] = serde_json::Value::String(endpoint_url);
        return Ok(Some(registry));
    }

    daemons.push(serde_json::json!({
        "id": "local-daemon",
        "label": label,
        "kind": "direct",
        "endpointUrl": endpoint_url,
        "resumeTokenHex": null,
        "lastConnectedAt": null,
    }));

    Ok(Some(registry))
}

#[tauri::command]
pub async fn cli_pocket_save_daemon_registry(
    app_state: State<'_, AppState>,
    state: serde_json::Value,
) -> Result<(), String> {
    shared_commands::save_daemon_registry(app_state.kv(), state).await
}
