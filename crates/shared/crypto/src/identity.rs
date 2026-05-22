use crate::redact::Secret;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde_json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("snow: {0}")]
    Snow(#[from] snow::Error),
    #[error("noise params: {0}")]
    NoiseParams(String),
    #[error("identity file has wrong permissions (expected mode 0600): {0}")]
    BadPermissions(String),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid key length: expected 32 bytes, got {0}")]
    WrongKeyLength(usize),
}

/// X25519 keypair used as the Noise static key.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyPair {
    pub public: [u8; 32],
    pub secret: Secret<[u8; 32]>,
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &B64.encode(self.public))
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl KeyPair {
    pub fn generate() -> Result<Self, IdentityError> {
        let builder = snow::Builder::new(noise_params()?);
        let kp = builder.generate_keypair()?;
        let mut public = [0_u8; 32];
        let mut secret = [0_u8; 32];
        public.copy_from_slice(&kp.public);
        secret.copy_from_slice(&kp.private);
        Ok(Self {
            public,
            secret: Secret::new(secret),
        })
    }
}

fn noise_params() -> Result<snow::params::NoiseParams, IdentityError> {
    "Noise_XK_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .map_err(|err: snow::Error| IdentityError::NoiseParams(err.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub version: u32,
    #[serde(rename = "host_id")]
    pub host_id: Uuid,
    pub created_at: String,
    #[serde(rename = "static_public_key", with = "key32_b64")]
    pub static_public: [u8; 32],
    #[serde(rename = "static_secret_key")]
    pub static_secret: Secret<KeyBytes32>,
}

/// Newtype carrying 32 raw bytes, base64-serialized.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyBytes32(pub [u8; 32]);

impl std::fmt::Debug for KeyBytes32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("KeyBytes32")
            .field(&B64.encode(self.0))
            .finish()
    }
}

impl Serialize for KeyBytes32 {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&B64.encode(self.0))
    }
}

impl<'de> Deserialize<'de> for KeyBytes32 {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        let bytes = B64.decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "expected 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0_u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

mod key32_b64 {
    use super::*;

    pub fn serialize<S: Serializer>(key: &[u8; 32], ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&B64.encode(key))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<[u8; 32], D::Error> {
        let kb = KeyBytes32::deserialize(de)?;
        Ok(kb.0)
    }
}

impl Identity {
    #[must_use]
    pub fn from_keypair(kp: &KeyPair) -> Self {
        Self {
            version: 1,
            host_id: Uuid::now_v7(),
            created_at: now_rfc3339(),
            static_public: kp.public,
            static_secret: Secret::new(KeyBytes32(*kp.secret.expose())),
        }
    }

    pub fn generate() -> Result<Self, IdentityError> {
        Ok(Self::from_keypair(&KeyPair::generate()?))
    }

    #[must_use]
    pub fn keypair(&self) -> KeyPair {
        KeyPair {
            public: self.static_public,
            secret: Secret::new(self.static_secret.expose().0),
        }
    }

    pub fn load_strict(path: &Path) -> Result<Self, IdentityError> {
        check_mode(path)?;
        let bytes = fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn load(path: &Path) -> Result<Self, IdentityError> {
        Self::load_strict(path)
    }

    pub fn save(&self, path: &Path) -> Result<(), IdentityError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        save_identity_json(path, &json)
    }
}

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (year, month, day, hour, minute, second) = epoch_to_ymd_hms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn epoch_to_ymd_hms(mut s: u64) -> (i32, u32, u32, u32, u32, u32) {
    let second = (s % 60) as u32;
    s /= 60;
    let minute = (s % 60) as u32;
    s /= 60;
    let hour = (s % 24) as u32;
    let mut days = s / 24;
    let mut year: i32 = 1970;

    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }

    let month_lengths = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: u32 = 1;
    for month_len in month_lengths {
        if days < month_len {
            break;
        }
        days -= month_len;
        month += 1;
    }
    let day = u32::try_from(days + 1).unwrap_or(1);
    (year, month, day, hour, minute, second)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(unix)]
fn check_mode(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;

    let meta = fs::metadata(path)?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(IdentityError::BadPermissions(format!(
            "{}: got 0o{:o}, expected 0o600. Fix with: chmod 600 {}",
            path.display(),
            mode,
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_mode(path: &Path) -> Result<(), IdentityError> {
    let _ = fs::metadata(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode_600(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;

    let mut perm = fs::metadata(path)?.permissions();
    perm.set_mode(0o600);
    fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode_600(path: &Path) -> Result<(), IdentityError> {
    let _ = fs::metadata(path)?;
    Ok(())
}

#[cfg(unix)]
fn save_identity_json(path: &Path, json: &[u8]) -> Result<(), IdentityError> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    let tmp_path = path.with_extension(format!("{}.tmp", Uuid::now_v7().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp_path)?;
    file.write_all(json)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(not(unix))]
fn save_identity_json(path: &Path, json: &[u8]) -> Result<(), IdentityError> {
    fs::write(path, json)?;
    set_mode_600(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn keypair_generates_32_byte_public() {
        let kp = KeyPair::generate().expect("generate keypair");
        assert_eq!(kp.public.len(), 32);
        assert_eq!(kp.secret.expose().len(), 32);
    }

    #[test]
    fn identity_roundtrips_through_file() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("host_identity.json");
        let kp = KeyPair::generate().expect("generate keypair");
        let id = Identity::from_keypair(&kp);

        id.save(&path).expect("save identity");
        let back = Identity::load_strict(&path).expect("load identity");

        assert_eq!(back.static_public, id.static_public);
        assert_eq!(back.static_secret.expose().0, id.static_secret.expose().0);
        assert_eq!(back.host_id, id.host_id);
    }

    #[cfg(unix)]
    #[test]
    fn load_rejects_world_readable_file() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("host_identity.json");
        let id = Identity::from_keypair(&KeyPair::generate().expect("generate keypair"));
        id.save(&path).expect("save identity");

        let mut perm = fs::metadata(&path).expect("metadata").permissions();
        perm.set_mode(0o644);
        fs::set_permissions(&path, perm).expect("set permissions");

        let err = Identity::load_strict(&path).expect_err("world-readable file rejected");
        assert!(matches!(err, IdentityError::BadPermissions(_)));
    }

    #[test]
    fn epoch_to_ymd_works_for_known_value() {
        let actual = epoch_to_ymd_hms(1_779_321_600);
        assert_eq!(actual, (2026, 5, 21, 0, 0, 0));
    }

    #[test]
    fn generate_produces_identity_with_host_id_field() {
        let id = Identity::generate().expect("generate identity");
        let json = serde_json::to_value(&id).expect("serialize identity");
        assert!(json.get("host_id").is_some());
        assert!(json.get("id").is_none());
    }
}
