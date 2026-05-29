use tauri::{AppHandle, Emitter};
use tauri_plugin_deep_link::DeepLinkExt;

pub fn install(app: &AppHandle) {
    let emit_app = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            let _ = emit_app.emit("cli_pocket:deep_link", url.to_string());
        }
    });
}
