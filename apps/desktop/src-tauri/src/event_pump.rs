use cli_pocket_tauri_bindings::ClientEvent;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

/// Start the event pump task that forwards `ClientEvent`s to Tauri.
///
/// This should be called once during app setup with the event receiver
/// taken from `AppState`.
pub fn start(app: AppHandle, mut event_rx: mpsc::Receiver<ClientEvent>) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            // Convert event to a serializable payload
            let (event_name, payload) = match &event {
                ClientEvent::Connecting => ("session:connecting", serde_json::json!({})),
                ClientEvent::Connected { session_id } => (
                    "session:connected",
                    serde_json::json!({ "session_id": format!("{:?}", session_id) }),
                ),
                ClientEvent::Disconnected { will_retry, reason } => (
                    "session:disconnected",
                    serde_json::json!({ "will_retry": will_retry, "reason": reason }),
                ),
                ClientEvent::TerminalCreated(info) => (
                    "terminal:created",
                    serde_json::json!({
                        "terminal_id": format!("{:?}", info.terminal),
                        "cols": info.cols,
                        "rows": info.rows,
                        "label": info.label,
                    }),
                ),
                ClientEvent::TerminalOutput {
                    terminal_id,
                    stream_seq,
                    bytes,
                } => (
                    "terminal:output",
                    serde_json::json!({
                        "terminal_id": format!("{:?}", terminal_id),
                        "stream_seq": stream_seq.0,
                        "bytes_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
                    }),
                ),
                ClientEvent::TerminalExited { terminal_id, info } => (
                    "terminal:exited",
                    serde_json::json!({
                        "terminal_id": format!("{:?}", terminal_id),
                        "exit_info": format!("{:?}", info),
                    }),
                ),
                ClientEvent::Error(msg) => ("session:error", serde_json::json!({ "message": msg })),
            };

            // Emit the event to the frontend
            if let Err(err) = app.emit(event_name, &payload) {
                tracing::warn!("failed to emit event {}: {}", event_name, err);
            }
        }
    });
}
