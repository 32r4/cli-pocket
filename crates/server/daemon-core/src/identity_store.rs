use std::path::Path;

use cli_pocket_crypto::{Identity, KeyPair};
use cli_pocket_proto::HostId;

#[derive(Debug, Clone)]
pub struct DaemonIdentity {
    pub host_id: HostId,
    pub keypair: KeyPair,
}

pub fn load_or_create(path: &Path) -> crate::DaemonResult<DaemonIdentity> {
    let identity = if path.exists() {
        Identity::load_strict(path)
            .map_err(|error| crate::DaemonError::Identity(error.to_string()))?
    } else {
        let identity = Identity::generate()
            .map_err(|error| crate::DaemonError::Identity(error.to_string()))?;
        identity
            .save(path)
            .map_err(|error| crate::DaemonError::Identity(error.to_string()))?;
        identity
    };

    Ok(DaemonIdentity {
        host_id: HostId(identity.host_id),
        keypair: identity.keypair(),
    })
}
