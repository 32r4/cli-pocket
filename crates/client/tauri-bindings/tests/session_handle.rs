use cli_pocket_tauri_bindings::SessionHandle;

#[tokio::test(flavor = "current_thread")]
async fn build_handle_compiles() {
    let h = SessionHandle::new_disconnected();
    assert!(!h.is_connected());
}
