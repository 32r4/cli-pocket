use cli_pocket_crypto::{KeyPair, Secret};
use cli_pocket_proto::ClientId;
use serde::{Deserialize, Serialize};
use snow::resolvers::{CryptoResolver, DefaultResolver};
use uuid::Builder as UuidBuilder;

const KEYPAIR_KEY: &str = "cli-pocket/identity/v1/keypair";
const CLIENT_ID_KEY: &str = "cli-pocket/identity/v1/client-id";
const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct ClientIdentity {
    pub client_id: ClientId,
    pub keypair: KeyPair,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredKeyPair {
    version: u32,
    public: [u8; 32],
    private: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedIdentity {
    version: u32,
    client_id: ClientId,
    keypair: StoredKeyPair,
}

impl ClientIdentity {
    pub async fn load_or_create<K: crate::KeyValueStore, R: crate::Rng>(
        kv: &K,
        rng: &R,
    ) -> crate::ClientResult<Self> {
        if let Some(bytes) = kv.get(KEYPAIR_KEY).await? {
            let keypair = decode_stored_keypair(&bytes)?.into_keypair()?;
            let client_id_bytes = kv
                .get(CLIENT_ID_KEY)
                .await?
                .ok_or_else(|| crate::ClientError::Identity("missing client-id".to_owned()))?;
            let client_id = decode_client_id(&client_id_bytes)?;

            return Ok(Self { client_id, keypair });
        }

        let mut private = [0_u8; 32];
        rng.fill(&mut private);
        let keypair = keypair_from_private(private)?;
        let client_id = generate_client_id(rng);

        persist(kv, &keypair, client_id).await?;

        Ok(Self { client_id, keypair })
    }

    pub fn export_serialized(&self) -> crate::ClientResult<Vec<u8>> {
        serde_json::to_vec_pretty(&ExportedIdentity {
            version: EXPORT_VERSION,
            client_id: self.client_id,
            keypair: StoredKeyPair::from_keypair(&self.keypair),
        })
        .map_err(|err| crate::ClientError::Identity(err.to_string()))
    }

    pub async fn import_serialized<K: crate::KeyValueStore>(
        kv: &K,
        bytes: &[u8],
    ) -> crate::ClientResult<()> {
        let exported: ExportedIdentity = serde_json::from_slice(bytes)
            .map_err(|err| crate::ClientError::Identity(err.to_string()))?;

        if exported.version != EXPORT_VERSION {
            return Err(crate::ClientError::Identity(format!(
                "unknown identity export version {}",
                exported.version
            )));
        }

        let keypair = exported.keypair.into_keypair()?;
        persist(kv, &keypair, exported.client_id).await
    }
}

async fn persist<K: crate::KeyValueStore>(
    kv: &K,
    keypair: &KeyPair,
    client_id: ClientId,
) -> crate::ClientResult<()> {
    let keypair_bytes = serde_json::to_vec(&StoredKeyPair::from_keypair(keypair))
        .map_err(|err| crate::ClientError::Identity(err.to_string()))?;
    let client_id_bytes = serde_json::to_vec(&client_id)
        .map_err(|err| crate::ClientError::Identity(err.to_string()))?;

    kv.put(KEYPAIR_KEY, &keypair_bytes).await?;
    kv.put(CLIENT_ID_KEY, &client_id_bytes).await?;
    Ok(())
}

impl StoredKeyPair {
    fn from_keypair(keypair: &KeyPair) -> Self {
        Self {
            version: EXPORT_VERSION,
            public: keypair.public,
            private: *keypair.secret.expose(),
        }
    }

    fn into_keypair(self) -> crate::ClientResult<KeyPair> {
        if self.version != EXPORT_VERSION {
            return Err(crate::ClientError::Identity(format!(
                "unknown keypair version {}",
                self.version
            )));
        }

        let keypair = keypair_from_private(self.private)?;
        if keypair.public != self.public {
            return Err(crate::ClientError::Identity(
                "public key does not match private key".to_owned(),
            ));
        }

        Ok(keypair)
    }
}

fn decode_stored_keypair(bytes: &[u8]) -> crate::ClientResult<StoredKeyPair> {
    serde_json::from_slice(bytes).map_err(|err| crate::ClientError::Identity(err.to_string()))
}

fn decode_client_id(bytes: &[u8]) -> crate::ClientResult<ClientId> {
    serde_json::from_slice(bytes).map_err(|err| crate::ClientError::Identity(err.to_string()))
}

fn keypair_from_private(private: [u8; 32]) -> crate::ClientResult<KeyPair> {
    let resolver = DefaultResolver;
    let mut dh = resolver
        .resolve_dh(&snow::params::DHChoice::Curve25519)
        .ok_or_else(|| crate::ClientError::Crypto("Curve25519 resolver unavailable".to_owned()))?;
    dh.set(&private);

    let public: [u8; 32] = dh
        .pubkey()
        .try_into()
        .map_err(|_| crate::ClientError::Crypto("Curve25519 public key length".to_owned()))?;

    Ok(KeyPair {
        public,
        secret: Secret::new(private),
    })
}

fn generate_client_id<R: crate::Rng>(rng: &R) -> ClientId {
    let mut bytes = [0_u8; 16];
    rng.fill(&mut bytes);
    ClientId(UuidBuilder::from_random_bytes(bytes).into_uuid())
}
