use crate::identity::KeyPair;

const NOISE_PARAMS: &str = "Noise_XK_25519_ChaChaPoly_BLAKE2s";
const NOISE_PSK2_PARAMS: &str = "Noise_XKpsk2_25519_ChaChaPoly_BLAKE2s";
const NOISE_MAX_MSG_LEN: usize = 65_535;
const NOISE_TAG_LEN: usize = 16;
const NOISE_HANDSHAKE_MSG_BUF: usize = 1_024;

#[derive(Debug, thiserror::Error)]
pub enum NoiseError {
    #[error("snow: {0}")]
    Snow(#[from] snow::Error),
    #[error("noise params: {0}")]
    NoiseParams(String),
    #[error("payload exceeds Noise message size limit")]
    PayloadTooLarge,
    #[error("handshake not finished")]
    HandshakeNotFinished,
}

/// Client side of Noise XK. The initiator knows the responder's static
/// public key out of band from pairing.
pub struct NoiseInitiator {
    state: Option<snow::HandshakeState>,
}

impl NoiseInitiator {
    pub fn new(
        local: &KeyPair,
        remote_static_public: &[u8; 32],
        psk: Option<&[u8; 32]>,
    ) -> Result<Self, NoiseError> {
        let mut builder = snow::Builder::new(noise_params(psk.is_some())?)
            .local_private_key(local.secret.expose())
            .remote_public_key(remote_static_public);
        if let Some(psk) = psk {
            builder = builder.psk(2, psk);
        }
        Ok(Self {
            state: Some(builder.build_initiator()?),
        })
    }

    pub fn write_handshake(&mut self) -> Result<Vec<u8>, NoiseError> {
        let state = self
            .state
            .as_mut()
            .ok_or(NoiseError::HandshakeNotFinished)?;
        let mut buf = vec![0_u8; NOISE_HANDSHAKE_MSG_BUF];
        let n = state.write_message(&[], &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    pub fn read_handshake(&mut self, msg: &[u8]) -> Result<(), NoiseError> {
        let state = self
            .state
            .as_mut()
            .ok_or(NoiseError::HandshakeNotFinished)?;
        let mut scratch = vec![0_u8; NOISE_HANDSHAKE_MSG_BUF];
        state.read_message(msg, &mut scratch)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<NoiseSession, NoiseError> {
        let state = self
            .state
            .take()
            .ok_or(NoiseError::HandshakeNotFinished)?
            .into_transport_mode()?;
        Ok(NoiseSession { transport: state })
    }

    pub fn is_handshake_finished(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(snow::HandshakeState::is_handshake_finished)
    }
}

/// Server side of Noise XK.
pub struct NoiseResponder {
    state: Option<snow::HandshakeState>,
}

impl NoiseResponder {
    pub fn new(local: &KeyPair, psk: Option<&[u8; 32]>) -> Result<Self, NoiseError> {
        let mut builder = snow::Builder::new(noise_params(psk.is_some())?)
            .local_private_key(local.secret.expose());
        if let Some(psk) = psk {
            builder = builder.psk(2, psk);
        }
        Ok(Self {
            state: Some(builder.build_responder()?),
        })
    }

    pub fn read_handshake(&mut self, msg: &[u8]) -> Result<(), NoiseError> {
        let state = self
            .state
            .as_mut()
            .ok_or(NoiseError::HandshakeNotFinished)?;
        let mut scratch = vec![0_u8; NOISE_HANDSHAKE_MSG_BUF];
        state.read_message(msg, &mut scratch)?;
        Ok(())
    }

    pub fn write_handshake(&mut self) -> Result<Vec<u8>, NoiseError> {
        let state = self
            .state
            .as_mut()
            .ok_or(NoiseError::HandshakeNotFinished)?;
        let mut buf = vec![0_u8; NOISE_HANDSHAKE_MSG_BUF];
        let n = state.write_message(&[], &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// After handshake completes, the responder learns the initiator's static
    /// public key, which the daemon checks against `clients.json`.
    #[must_use]
    pub fn remote_static_public(&self) -> Option<[u8; 32]> {
        let rs = self.state.as_ref()?.get_remote_static()?;
        let mut out = [0_u8; 32];
        if rs.len() != out.len() {
            return None;
        }
        out.copy_from_slice(rs);
        Some(out)
    }

    pub fn finish(mut self) -> Result<NoiseSession, NoiseError> {
        let state = self
            .state
            .take()
            .ok_or(NoiseError::HandshakeNotFinished)?
            .into_transport_mode()?;
        Ok(NoiseSession { transport: state })
    }

    pub fn is_handshake_finished(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(snow::HandshakeState::is_handshake_finished)
    }
}

/// Transport-mode Noise. Encrypts/decrypts whole frames; nonces are managed
/// internally by `snow`.
pub struct NoiseSession {
    transport: snow::TransportState,
}

impl NoiseSession {
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if plaintext.len() + NOISE_TAG_LEN > NOISE_MAX_MSG_LEN {
            return Err(NoiseError::PayloadTooLarge);
        }

        let mut out = vec![0_u8; plaintext.len() + NOISE_TAG_LEN];
        let n = self.transport.write_message(plaintext, &mut out)?;
        out.truncate(n);
        Ok(out)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let mut out = vec![0_u8; ciphertext.len()];
        let n = self.transport.read_message(ciphertext, &mut out)?;
        out.truncate(n);
        Ok(out)
    }
}

impl std::fmt::Debug for NoiseSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoiseSession")
            .field("state", &"<active>")
            .finish()
    }
}

fn noise_params(with_psk: bool) -> Result<snow::params::NoiseParams, NoiseError> {
    let params = if with_psk {
        NOISE_PSK2_PARAMS
    } else {
        NOISE_PARAMS
    };
    params
        .parse()
        .map_err(|err: snow::Error| NoiseError::NoiseParams(err.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::identity::KeyPair;
    use crate::{NoiseError, NoiseInitiator, NoiseResponder};

    #[test]
    fn noise_xk_full_handshake_and_bidirectional_transport() {
        let server = KeyPair::generate().expect("generate server keypair");
        let client = KeyPair::generate().expect("generate client keypair");

        let mut initiator =
            NoiseInitiator::new(&client, &server.public, None).expect("create initiator");
        let mut responder = NoiseResponder::new(&server, None).expect("create responder");

        let m1 = initiator.write_handshake().expect("write message 1");
        responder.read_handshake(&m1).expect("read message 1");
        let m2 = responder.write_handshake().expect("write message 2");
        initiator.read_handshake(&m2).expect("read message 2");
        let m3 = initiator.write_handshake().expect("write message 3");
        responder.read_handshake(&m3).expect("read message 3");

        assert!(initiator.is_handshake_finished());
        assert!(responder.is_handshake_finished());
        assert_eq!(
            responder.remote_static_public(),
            Some(client.public),
            "responder learns initiator static key"
        );

        let mut client_session = initiator.finish().expect("finish initiator");
        let mut server_session = responder.finish().expect("finish responder");

        let client_plaintext = b"hello daemon";
        let ciphertext = client_session
            .encrypt(client_plaintext)
            .expect("encrypt client payload");
        let plaintext = server_session
            .decrypt(&ciphertext)
            .expect("decrypt client payload");
        assert_eq!(plaintext, client_plaintext);

        let server_plaintext = b"hello client";
        let ciphertext = server_session
            .encrypt(server_plaintext)
            .expect("encrypt server payload");
        let plaintext = client_session
            .decrypt(&ciphertext)
            .expect("decrypt server payload");
        assert_eq!(plaintext, server_plaintext);
    }

    #[test]
    fn psk_mismatch_rejects_handshake() {
        let server = KeyPair::generate().expect("generate server keypair");
        let client = KeyPair::generate().expect("generate client keypair");
        let psk_a = [1_u8; 32];
        let psk_b = [2_u8; 32];

        let mut initiator =
            NoiseInitiator::new(&client, &server.public, Some(&psk_a)).expect("create initiator");
        let mut responder = NoiseResponder::new(&server, Some(&psk_b)).expect("create responder");

        let m1 = initiator.write_handshake().expect("write message 1");
        responder.read_handshake(&m1).expect("read message 1");
        let m2 = responder.write_handshake().expect("write message 2");
        let result = initiator.read_handshake(&m2);
        assert!(result.is_err(), "PSK mismatch must fail the handshake");
    }

    #[test]
    fn rejects_payload_over_message_limit() {
        let server = KeyPair::generate().expect("generate server keypair");
        let client = KeyPair::generate().expect("generate client keypair");
        let mut initiator =
            NoiseInitiator::new(&client, &server.public, None).expect("create initiator");
        let mut responder = NoiseResponder::new(&server, None).expect("create responder");

        let m1 = initiator.write_handshake().expect("write message 1");
        responder.read_handshake(&m1).expect("read message 1");
        let m2 = responder.write_handshake().expect("write message 2");
        initiator.read_handshake(&m2).expect("read message 2");
        let m3 = initiator.write_handshake().expect("write message 3");
        responder.read_handshake(&m3).expect("read message 3");

        let mut session = initiator.finish().expect("finish initiator");
        let oversized = vec![0_u8; 65_535];
        let err = session
            .encrypt(&oversized)
            .expect_err("oversized payload rejected");
        assert!(matches!(err, NoiseError::PayloadTooLarge));
    }
}
