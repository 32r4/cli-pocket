use rand_core::OsRng;
use spake2::{Ed25519Group, Identity, Password, Spake2 as InnerSpake2};

#[derive(Debug, thiserror::Error)]
pub enum Spake2Error {
    #[error("spake2")]
    Spake2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spake2Outcome {
    pub shared: Vec<u8>,
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
        Ok(Spake2Outcome {
            shared: self
                .inner
                .finish(peer_msg)
                .map_err(|_| Spake2Error::Spake2)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Spake2Side;

    #[test]
    fn matching_codes_agree() {
        let host = Spake2Side::start_host("493152", b"host", b"client-hint");
        let client = Spake2Side::start_client("493152", b"host", b"client-hint");
        let host_msg = host.outbound().to_vec();
        let client_msg = client.outbound().to_vec();
        let host_out = host.finish(&client_msg).expect("host finish");
        let client_out = client.finish(&host_msg).expect("client finish");
        assert_eq!(host_out.shared, client_out.shared);
        assert!(host_out.shared.len() >= 32);
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
}
