use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, ClientIdentity, KeyValueStore, Rng};
use std::collections::HashMap;
use std::sync::Mutex;

const KEYPAIR_KEY: &str = "cli-pocket/identity/v1/keypair";
const CLIENT_ID_KEY: &str = "cli-pocket/identity/v1/client-id";

#[derive(Default)]
struct MemKv {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemKv {
    fn put_sync(&self, key: &str, value: Vec<u8>) {
        self.inner.lock().unwrap().insert(key.to_owned(), value);
    }
}

#[async_trait(?Send)]
impl KeyValueStore for MemKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(self.inner.lock().unwrap().get(key).cloned())
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<(), ClientError> {
        self.inner
            .lock()
            .unwrap()
            .insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), ClientError> {
        self.inner.lock().unwrap().remove(key);
        Ok(())
    }
}

struct FixedRng {
    byte: u8,
}

impl Rng for FixedRng {
    fn fill(&self, dest: &mut [u8]) {
        dest.fill(self.byte);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn first_run_generates_and_persists() {
    let kv = MemKv::default();
    let rng = FixedRng { byte: 7 };

    let id1 = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();
    let id2 = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();

    assert_eq!(id1.client_id, id2.client_id);
    assert_eq!(id1.keypair.public, id2.keypair.public);
    assert_eq!(id1.keypair.secret.expose(), id2.keypair.secret.expose());
}

#[tokio::test(flavor = "current_thread")]
async fn first_run_uses_injected_rng_for_private_key() {
    let kv = MemKv::default();
    let rng = FixedRng { byte: 11 };

    let id = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();

    assert_eq!(id.keypair.secret.expose(), &[11; 32]);
}

#[tokio::test(flavor = "current_thread")]
async fn export_then_import_into_fresh_kv() {
    let kv1 = MemKv::default();
    let rng = FixedRng { byte: 13 };
    let id1 = ClientIdentity::load_or_create(&kv1, &rng).await.unwrap();
    let exported = id1.export_serialized().unwrap();

    let kv2 = MemKv::default();
    ClientIdentity::import_serialized(&kv2, &exported)
        .await
        .unwrap();
    let id2 = ClientIdentity::load_or_create(&kv2, &rng).await.unwrap();

    assert_eq!(id1.client_id, id2.client_id);
    assert_eq!(id1.keypair.public, id2.keypair.public);
    assert_eq!(id1.keypair.secret.expose(), id2.keypair.secret.expose());
}

#[tokio::test(flavor = "current_thread")]
async fn export_contains_only_client_identity_fields() {
    let kv = MemKv::default();
    let rng = FixedRng { byte: 17 };
    let id = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();
    let exported = id.export_serialized().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&exported).unwrap();
    let object = value.as_object().unwrap();

    assert!(object.contains_key("version"));
    assert!(object.contains_key("client_id"));
    assert!(object.contains_key("keypair"));
    assert!(!object.contains_key("identity"));
    assert!(!exported.windows(b"host_id".len()).any(|w| w == b"host_id"));
    assert!(!exported
        .windows(b"created_at".len())
        .any(|w| w == b"created_at"));
}

#[tokio::test(flavor = "current_thread")]
async fn import_rejects_mismatched_key_material() {
    let kv = MemKv::default();
    let rng = FixedRng { byte: 19 };
    let id = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&id.export_serialized().unwrap()).unwrap();
    value["keypair"]["public"][0] = serde_json::Value::from(255_u8);
    let exported = serde_json::to_vec(&value).unwrap();

    let err = ClientIdentity::import_serialized(&MemKv::default(), &exported)
        .await
        .unwrap_err();

    assert!(
        matches!(err, ClientError::Identity(message) if message.contains("public key does not match private key"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn load_rejects_mismatched_stored_key_material() {
    let kv = MemKv::default();
    let rng = FixedRng { byte: 23 };
    let id = ClientIdentity::load_or_create(&kv, &rng).await.unwrap();
    let mut keypair: serde_json::Value =
        serde_json::from_slice(&kv.get(KEYPAIR_KEY).await.unwrap().unwrap()).unwrap();
    keypair["public"][0] = serde_json::Value::from(255_u8);
    kv.put_sync(KEYPAIR_KEY, serde_json::to_vec(&keypair).unwrap());
    kv.put_sync(CLIENT_ID_KEY, serde_json::to_vec(&id.client_id).unwrap());

    let err = ClientIdentity::load_or_create(&kv, &rng).await.unwrap_err();

    assert!(
        matches!(err, ClientError::Identity(message) if message.contains("public key does not match private key"))
    );
}
