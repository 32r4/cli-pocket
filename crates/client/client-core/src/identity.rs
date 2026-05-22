use cli_pocket_crypto::KeyPair;
use cli_pocket_proto::ClientId;

#[derive(Debug, Clone)]
pub struct ClientIdentity {
    pub client_id: ClientId,
    pub keypair: KeyPair,
}
