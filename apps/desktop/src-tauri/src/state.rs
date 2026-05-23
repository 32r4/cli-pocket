use cli_pocket_tauri_bindings::SessionHandle;

pub struct AppState {
    pub session: SessionHandle,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            session: SessionHandle::new_disconnected(),
        }
    }
}
