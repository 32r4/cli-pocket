use cli_pocket_client_core::KeyValueStore;
use cli_pocket_tauri_bindings::FileKvStore;
use tempfile::tempdir;

#[tokio::test]
async fn roundtrip_and_delete() {
    let dir = tempdir().unwrap();
    let store = FileKvStore::open_at(dir.path()).unwrap();

    store.put("a", b"hello").await.unwrap();
    store.put("b", b"world").await.unwrap();
    assert_eq!(store.get("a").await.unwrap().as_deref(), Some(&b"hello"[..]));
    assert_eq!(store.get("missing").await.unwrap(), None);

    store.delete("a").await.unwrap();
    assert_eq!(store.get("a").await.unwrap(), None);

    // Re-open from disk to verify persistence.
    drop(store);
    let store2 = FileKvStore::open_at(dir.path()).unwrap();
    assert_eq!(store2.get("b").await.unwrap().as_deref(), Some(&b"world"[..]));
}
