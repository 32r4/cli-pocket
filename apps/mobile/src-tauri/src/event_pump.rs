use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use cli_pocket_client_core::ClientEvent;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

const EVENT_CHANNEL: &str = "cli_pocket:event";

pub fn start(app: AppHandle, mut event_rx: mpsc::Receiver<ClientEvent>) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let Err(error) = app.emit(EVENT_CHANNEL, serialize_event(&event)) {
                tracing::warn!("failed to emit {EVENT_CHANNEL}: {error}");
            }
        }
    });
}

fn serialize_event(event: &ClientEvent) -> serde_json::Value {
    match event {
        ClientEvent::Connecting => json!({ "kind": "Connecting" }),
        ClientEvent::Connected { session_id } => {
            json!({ "kind": "Connected", "session_id": session_id.0.to_string() })
        }
        ClientEvent::Disconnected { will_retry, reason } => json!({
            "kind": "Disconnected",
            "will_retry": will_retry,
            "reason": reason,
        }),
        ClientEvent::TerminalCreated(info) => json!({
            "kind": "TerminalCreated",
            "info": {
                "terminal": info.terminal.0.to_string(),
                "cols": info.cols,
                "rows": info.rows,
                "created_at_unix_ms": info.created_at_unix_ms,
                "label": info.label,
                "attached_clients": info.attached_clients,
            }
        }),
        ClientEvent::TerminalOutput {
            terminal_id,
            stream_seq,
            bytes,
        } => json!({
            "kind": "TerminalOutput",
            "terminal_id": terminal_id.0.to_string(),
            "stream_seq": stream_seq.0,
            "bytes_b64": BASE64.encode(bytes),
        }),
        ClientEvent::TerminalExited { terminal_id, info } => json!({
            "kind": "TerminalExited",
            "terminal_id": terminal_id.0.to_string(),
            "info": {
                "code": info.code,
                "signal": info.signal,
                "at_unix_ms": info.at_unix_ms,
            }
        }),
        ClientEvent::Error(message) => json!({
            "kind": "Error",
            "message": message,
        }),
    }
}
