use cli_pocket_proto::{ClientId, TerminalId};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

const VERSION: u8 = 1;
const TAG_LEN: usize = 16;
const BODY_LEN: usize = 1 + 16 + 16 + 8 + 8; // version + client_id + terminal_id + issued_ms + expiry_ms
const TOKEN_LEN: usize = BODY_LEN + TAG_LEN;

type HmacSha256 = Hmac<Sha256>;

/// Secret key used to mint and verify resume tokens.
pub struct ResumeTokenSecret([u8; 32]);

/// Opaque token bytes (hex-encodable for the wire).
#[derive(Debug, Clone)]
pub struct ResumeTokenBytes {
    pub bytes: Vec<u8>,
}

/// Successfully verified resume token payload.
#[derive(Debug, Clone)]
pub struct VerifiedResume {
    pub client_id: ClientId,
    pub terminal_id: TerminalId,
    pub issued_ms: u64,
    pub expiry_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ResumeError {
    #[error("malformed token")]
    Malformed,
    #[error("bad version")]
    BadVersion,
    #[error("token expired")]
    Expired,
    #[error("bad tag")]
    BadTag,
}

impl ResumeTokenSecret {
    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    /// Mint a new resume token for the given client and terminal.
    pub fn mint(
        &self,
        client: &ClientId,
        terminal: &TerminalId,
        now_ms: u64,
        ttl_ms: u64,
    ) -> ResumeTokenBytes {
        let mut body = Vec::with_capacity(BODY_LEN);
        body.push(VERSION);
        body.extend_from_slice(client.0.as_bytes());
        body.extend_from_slice(terminal.0.as_bytes());
        body.extend_from_slice(&now_ms.to_be_bytes());
        body.extend_from_slice(&(now_ms + ttl_ms).to_be_bytes());

        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(&body);
        let tag = mac.finalize().into_bytes();
        body.extend_from_slice(&tag[..TAG_LEN]);

        ResumeTokenBytes { bytes: body }
    }

    /// Verify a resume token and return the embedded claims.
    pub fn verify(
        &self,
        token: &ResumeTokenBytes,
        now_ms: u64,
    ) -> Result<VerifiedResume, ResumeError> {
        if token.bytes.len() != TOKEN_LEN {
            return Err(ResumeError::Malformed);
        }

        let (body, tag) = token.bytes.split_at(BODY_LEN);

        if body[0] != VERSION {
            return Err(ResumeError::BadVersion);
        }

        // Recompute and compare the HMAC tag in constant time.
        let mut mac = HmacSha256::new_from_slice(&self.0).unwrap();
        mac.update(body);
        let expected = mac.finalize().into_bytes();
        if !bool::from(expected[..TAG_LEN].ct_eq(tag)) {
            return Err(ResumeError::BadTag);
        }

        let cid_bytes: [u8; 16] = body[1..17].try_into().unwrap();
        let tid_bytes: [u8; 16] = body[17..33].try_into().unwrap();
        let issued_ms = u64::from_be_bytes(body[33..41].try_into().unwrap());
        let expiry_ms = u64::from_be_bytes(body[41..49].try_into().unwrap());

        if now_ms >= expiry_ms {
            return Err(ResumeError::Expired);
        }

        Ok(VerifiedResume {
            client_id: ClientId(Uuid::from_bytes(cid_bytes)),
            terminal_id: TerminalId(Uuid::from_bytes(tid_bytes)),
            issued_ms,
            expiry_ms,
        })
    }
}
