use cli_pocket_daemon_core::client_db::{ClientDb, ClientRecord};
use cli_pocket_proto::ClientId;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn pair_persists_and_lookup_works() {
    let dir = create_temp_dir("pair-persist");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();

    let cid = client_id("018f1200-0000-7000-8000-000000000001");
    let pk = [7_u8; 32];
    db.add(ClientRecord {
        client_id: cid,
        public_key: pk,
        label: "phone".into(),
        paired_at: 0,
    })
    .await
    .unwrap();

    let by_public = db.lookup_by_public(&pk).await.unwrap().unwrap();
    assert_eq!(by_public.client_id, cid);

    let by_id = db.lookup_by_id(&cid).await.unwrap().unwrap();
    assert_eq!(by_id.public_key, pk);

    drop(db);
    let db2 = ClientDb::open(&clients, &revoked).await.unwrap();
    assert!(db2.lookup_by_public(&pk).await.unwrap().is_some());

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn revocation_propagates_through_watch() {
    let dir = create_temp_dir("revoke-watch");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();

    let cid = client_id("018f1200-0000-7000-8000-000000000002");
    db.add(ClientRecord {
        client_id: cid,
        public_key: [9_u8; 32],
        label: "laptop".into(),
        paired_at: 0,
    })
    .await
    .unwrap();
    assert!(!db.is_revoked(&cid).await);

    let mut rx = db.watch_revocations();
    db.revoke(cid).await.unwrap();
    tokio::time::timeout(Duration::from_millis(200), rx.changed())
        .await
        .expect("watch should fire")
        .unwrap();
    assert!(db.is_revoked(&cid).await);

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn externally_written_revocations_hot_reload() {
    let dir = create_temp_dir("revoke-hot-reload");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();

    let cid = client_id("018f1200-0000-7000-8000-000000000003");
    let mut rx = db.watch_revocations();
    std::fs::write(
        &revoked,
        format!(
            "{{\n  \"revoked\": [\n    \"{}\"\n  ]\n}}\n",
            serde_json::to_value(cid).unwrap().as_str().unwrap()
        ),
    )
    .unwrap();

    tokio::time::timeout(Duration::from_millis(500), rx.changed())
        .await
        .expect("external revocation write should reload")
        .unwrap();
    assert!(db.is_revoked(&cid).await);

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_client_id_is_rejected_without_changing_indexes() {
    let dir = create_temp_dir("duplicate-client-id");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();

    let cid = client_id("018f1200-0000-7000-8000-000000000004");
    let first_pk = [4_u8; 32];
    let second_pk = [5_u8; 32];
    db.add(ClientRecord {
        client_id: cid,
        public_key: first_pk,
        label: "phone".into(),
        paired_at: 0,
    })
    .await
    .unwrap();

    let err = db
        .add(ClientRecord {
            client_id: cid,
            public_key: second_pk,
            label: "tablet".into(),
            paired_at: 1,
        })
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("duplicate client_id"));

    assert_eq!(
        db.lookup_by_public(&first_pk)
            .await
            .unwrap()
            .unwrap()
            .client_id,
        cid
    );
    assert!(db.lookup_by_public(&second_pk).await.unwrap().is_none());
    assert_eq!(
        db.lookup_by_id(&cid).await.unwrap().unwrap().public_key,
        first_pk
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_public_key_is_rejected_without_changing_indexes() {
    let dir = create_temp_dir("duplicate-public-key");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();

    let first_cid = client_id("018f1200-0000-7000-8000-000000000005");
    let second_cid = client_id("018f1200-0000-7000-8000-000000000006");
    let pk = [6_u8; 32];
    db.add(ClientRecord {
        client_id: first_cid,
        public_key: pk,
        label: "phone".into(),
        paired_at: 0,
    })
    .await
    .unwrap();

    let err = db
        .add(ClientRecord {
            client_id: second_cid,
            public_key: pk,
            label: "tablet".into(),
            paired_at: 1,
        })
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("duplicate public_key"));

    assert_eq!(
        db.lookup_by_public(&pk).await.unwrap().unwrap().client_id,
        first_cid
    );
    assert!(db.lookup_by_id(&second_cid).await.unwrap().is_none());

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn open_rejects_duplicate_records_from_disk() {
    let dir = create_temp_dir("duplicate-disk-records");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let cid = client_id("018f1200-0000-7000-8000-000000000007");
    std::fs::write(
        &clients,
        format!(
            "{{\"clients\":[\
             {{\"client_id\":{},\"public_key\":[7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7],\"label\":\"phone\",\"paired_at\":0}},\
             {{\"client_id\":{},\"public_key\":[8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8],\"label\":\"tablet\",\"paired_at\":1}}\
             ]}}",
            serde_json::to_string(&cid).unwrap(),
            serde_json::to_string(&cid).unwrap()
        ),
    )
    .unwrap();
    std::fs::write(&revoked, "{\"revoked\":[]}").unwrap();

    let err = ClientDb::open(&clients, &revoked).await.unwrap_err();
    assert!(format!("{err}").contains("duplicate client_id"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn open_rejects_duplicate_public_keys_from_disk() {
    let dir = create_temp_dir("duplicate-disk-public-keys");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let first_cid = client_id("018f1200-0000-7000-8000-00000000000a");
    let second_cid = client_id("018f1200-0000-7000-8000-00000000000b");
    std::fs::write(
        &clients,
        format!(
            "{{\"clients\":[\
             {{\"client_id\":{},\"public_key\":[10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10],\"label\":\"phone\",\"paired_at\":0}},\
             {{\"client_id\":{},\"public_key\":[10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10],\"label\":\"tablet\",\"paired_at\":1}}\
             ]}}",
            serde_json::to_string(&first_cid).unwrap(),
            serde_json::to_string(&second_cid).unwrap()
        ),
    )
    .unwrap();
    std::fs::write(&revoked, "{\"revoked\":[]}").unwrap();

    let err = ClientDb::open(&clients, &revoked).await.unwrap_err();
    assert!(format!("{err}").contains("duplicate public_key"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn add_persistence_failure_does_not_change_memory() {
    let dir = create_temp_dir("add-persist-failure");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();
    std::fs::remove_file(&clients).unwrap();
    std::fs::create_dir(&clients).unwrap();

    let cid = client_id("018f1200-0000-7000-8000-000000000008");
    let pk = [8_u8; 32];
    let err = db
        .add(ClientRecord {
            client_id: cid,
            public_key: pk,
            label: "phone".into(),
            paired_at: 0,
        })
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("I/O"));

    assert!(db.lookup_by_id(&cid).await.unwrap().is_none());
    assert!(db.lookup_by_public(&pk).await.unwrap().is_none());

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn revoke_persistence_failure_does_not_change_memory_or_notify() {
    let dir = create_temp_dir("revoke-persist-failure");
    let clients = dir.join("clients.json");
    let revoked = dir.join("revoked.json");
    let db = ClientDb::open(&clients, &revoked).await.unwrap();
    std::fs::remove_file(&revoked).unwrap();
    std::fs::create_dir(&revoked).unwrap();

    let cid = client_id("018f1200-0000-7000-8000-000000000009");
    let mut rx = db.watch_revocations();

    let err = db.revoke(cid).await.unwrap_err();
    assert!(format!("{err}").contains("I/O"));
    assert!(!db.is_revoked(&cid).await);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), rx.changed())
            .await
            .is_err()
    );

    std::fs::remove_dir_all(dir).unwrap();
}

fn client_id(value: &str) -> ClientId {
    serde_json::from_str(&format!("\"{value}\"")).unwrap()
}

fn create_temp_dir(name: &str) -> PathBuf {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "cli-pocket-client-db-{name}-{}-{}-{attempt}",
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
