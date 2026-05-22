use rand_core::OsRng;
use spake2::{Ed25519Group, Identity, Password, Spake2 as InnerSpake2};

#[derive(Debug, thiserror::Error)]
pub enum Spake2Error {
    #[error("spake2")]
    Spake2,
    #[error("spake2 shared secret too short for Noise PSK: {len} bytes")]
    SharedSecretTooShort { len: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spake2Outcome {
    pub shared: Vec<u8>,
    pub psk: [u8; 32],
}

impl Spake2Outcome {
    fn new(shared: Vec<u8>) -> Result<Self, Spake2Error> {
        let psk_bytes = shared
            .get(..32)
            .ok_or(Spake2Error::SharedSecretTooShort { len: shared.len() })?;
        let mut psk = [0_u8; 32];
        psk.copy_from_slice(psk_bytes);
        Ok(Self { shared, psk })
    }
}

pub struct Spake2Side {
    inner: InnerSpake2<Ed25519Group>,
    outbound: Vec<u8>,
}

impl Spake2Side {
    pub fn start_host(
        code: impl AsRef<[u8]>,
        host_id_bytes: impl AsRef<[u8]>,
        client_hint: impl AsRef<[u8]>,
    ) -> Self {
        let password = Password::new(code.as_ref());
        let id_a = Identity::new(host_id_bytes.as_ref());
        let id_b = Identity::new(client_hint.as_ref());
        let (inner, outbound) =
            InnerSpake2::<Ed25519Group>::start_a_with_rng(&password, &id_a, &id_b, OsRng);
        Self { inner, outbound }
    }

    pub fn start_client(
        code: impl AsRef<[u8]>,
        host_id_bytes: impl AsRef<[u8]>,
        client_hint: impl AsRef<[u8]>,
    ) -> Self {
        let password = Password::new(code.as_ref());
        let id_a = Identity::new(host_id_bytes.as_ref());
        let id_b = Identity::new(client_hint.as_ref());
        let (inner, outbound) =
            InnerSpake2::<Ed25519Group>::start_b_with_rng(&password, &id_a, &id_b, OsRng);
        Self { inner, outbound }
    }

    #[must_use]
    pub fn outbound(&self) -> &[u8] {
        &self.outbound
    }

    pub fn finish(self, peer_msg: &[u8]) -> Result<Spake2Outcome, Spake2Error> {
        let shared = self
            .inner
            .finish(peer_msg)
            .map_err(|_| Spake2Error::Spake2)?;
        Spake2Outcome::new(shared)
    }
}

#[cfg(test)]
mod tests {
    use super::{Spake2Error, Spake2Outcome, Spake2Side};
    use crate::identity::KeyPair;
    use crate::{NoiseInitiator, NoiseResponder};

    #[test]
    fn matching_codes_produce_noise_psks() {
        let host = Spake2Side::start_host("493152", b"host", b"client-hint");
        let client = Spake2Side::start_client("493152", b"host", b"client-hint");
        let host_msg = host.outbound().to_vec();
        let client_msg = client.outbound().to_vec();
        let host_out = host.finish(&client_msg).expect("host finish");
        let client_out = client.finish(&host_msg).expect("client finish");
        assert_eq!(host_out.shared, client_out.shared);
        assert_eq!(host_out.psk, client_out.psk);

        let server = KeyPair::generate().expect("generate server keypair");
        let client_keypair = KeyPair::generate().expect("generate client keypair");
        NoiseInitiator::new(&client_keypair, &server.public, Some(&client_out.psk))
            .expect("SPAKE2 PSK should be accepted by Noise initiator");
        NoiseResponder::new(&server, Some(&host_out.psk))
            .expect("SPAKE2 PSK should be accepted by Noise responder");
    }

    #[test]
    fn mismatched_codes_yield_disagreeing_keys() {
        let host = Spake2Side::start_host("493152", b"host", b"client-hint");
        let client = Spake2Side::start_client("000000", b"host", b"client-hint");
        let host_msg = host.outbound().to_vec();
        let client_msg = client.outbound().to_vec();
        let host_out = host.finish(&client_msg).expect("host finish");
        let client_out = client.finish(&host_msg).expect("client finish");
        assert_ne!(host_out.shared, client_out.shared);
    }

    #[test]
    fn rejects_shared_secret_too_short_for_noise_psk() {
        let err = Spake2Outcome::new(vec![7_u8; 31]).expect_err("short secret rejected");
        assert!(matches!(err, Spake2Error::SharedSecretTooShort { len: 31 }));
    }
}
