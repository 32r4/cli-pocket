use cli_pocket_daemon_core::identity_store::load_or_create;
use std::path::PathBuf;

#[test]
fn first_run_generates_then_persists() {
    let dir = create_temp_dir();
    let path = dir.join("identity.json");

    let id1 = load_or_create(&path).unwrap();
    assert!(path.exists(), "file should have been written");

    let id2 = load_or_create(&path).unwrap();
    assert_eq!(
        id1.host_id, id2.host_id,
        "second load returns same identity"
    );
    assert_eq!(id1.keypair.public, id2.keypair.public);

    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_world_readable_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = create_temp_dir();
    let path = dir.join("identity.json");
    let _ = load_or_create(&path).unwrap();

    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    perm.set_mode(0o644);
    std::fs::set_permissions(&path, perm).unwrap();

    let err = load_or_create(&path).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("permissions") || msg.contains("0600"),
        "got: {msg}"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

fn create_temp_dir() -> PathBuf {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "cli-pocket-identity-store-{}-{}-{attempt}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if std::fs::create_dir(&path).is_ok() {
            return path;
        }
    }

    panic!("failed to create temp directory");
}
