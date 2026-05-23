use cli_pocket_crypto::{KeyPair, NoiseResponder, NoiseSession};
use cli_pocket_transport::Transport;

use crate::client_db::{ClientDb, ClientRecord};

/// Result of a successful Noise XK responder handshake.
pub struct AcceptedHandshake {
    /// Established Noise session for encrypted frame transport.
    pub session: NoiseSession,
    /// The client record looked up via the initiator's static public key.
    pub client: ClientRecord,
}

/// Run the Noise XK responder handshake over the given transport.
///
/// The XK pattern requires three messages:
/// 1. Client -> Server: `e` (ephemeral)
/// 2. Server -> Client: `e, ee, s, es`
/// 3. Client -> Server: `s, se`
///
/// After the handshake completes, the responder learns the client's static
/// public key, which is looked up in `ClientDb` and checked for revocation.
pub async fn responder_handshake<T: Transport + ?Sized>(
    transport: &mut T,
    identity: &KeyPair,
    psk: Option<&[u8; 32]>,
    db: &ClientDb,
) -> crate::DaemonResult<AcceptedHandshake> {
    // Step 1: Create the NoiseResponder with our identity keypair and optional PSK.
    let mut responder = NoiseResponder::new(identity, psk).map_err(crate::DaemonError::Crypto)?;

    // Step 2: Read msg1 from client (`e`).
    let msg1 = transport
        .recv()
        .await
        .map_err(crate::DaemonError::Transport)?
        .ok_or_else(|| {
            crate::DaemonError::Internal("transport closed during handshake msg1".into())
        })?;
    responder
        .read_handshake(&msg1)
        .map_err(crate::DaemonError::Crypto)?;

    // Step 3: Write msg2 to client (`e, ee, s, es`).
    let msg2 = responder
        .write_handshake()
        .map_err(crate::DaemonError::Crypto)?;
    transport
        .send(msg2)
        .await
        .map_err(crate::DaemonError::Transport)?;

    // Step 4: Read msg3 from client (`s, se`).
    let msg3 = transport
        .recv()
        .await
        .map_err(crate::DaemonError::Transport)?
        .ok_or_else(|| {
            crate::DaemonError::Internal("transport closed during handshake msg3".into())
        })?;
    responder
        .read_handshake(&msg3)
        .map_err(crate::DaemonError::Crypto)?;

    // Step 5: Extract the client's static public key before consuming the responder.
    let client_pk = responder.remote_static_public().ok_or_else(|| {
        crate::DaemonError::Internal("XK handshake finished without remote static key".into())
    })?;

    // Step 6: Transition to transport mode.
    let session = responder.finish().map_err(crate::DaemonError::Crypto)?;

    // Step 7: Look up the client in the database by their static public key.
    let record = db
        .lookup_by_public(&client_pk)
        .await?
        .ok_or_else(|| crate::DaemonError::NotPaired(hex::encode(client_pk)))?;

    // Step 8: Check if the client has been revoked.
    if db.is_revoked(&record.client_id).await {
        return Err(crate::DaemonError::Revoked(format!(
            "{:?}",
            record.client_id
        )));
    }

    Ok(AcceptedHandshake {
        session,
        client: record,
    })
}
