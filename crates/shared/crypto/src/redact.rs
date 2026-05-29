use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Wraps secret bytes (private keys, PSKs, SPAKE2 shares mid-flight).
/// `Debug`/`Display` redact; `Serialize` writes the raw bytes since we do
/// need to persist them in `server_identity.json`. The redaction protection is
/// against accidental `tracing` / `eprintln!` leaks, not against serializers.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }

    pub fn expose(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted>")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted>")
    }
}

impl<T: Serialize> Serialize for Secret<T> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(Self(T::deserialize(de)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts() {
        let secret = Secret::new(vec![1_u8, 2, 3]);
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert_eq!(format!("{secret}"), "<redacted>");
    }

    #[test]
    fn serialize_preserves_payload() {
        let secret = Secret::new(vec![1_u8, 2, 3]);
        let json = serde_json::to_string(&secret).expect("serialize secret");
        assert_eq!(json, "[1,2,3]");

        let roundtrip: Secret<Vec<u8>> = serde_json::from_str(&json).expect("deserialize secret");
        assert_eq!(roundtrip.into_inner(), vec![1, 2, 3]);
    }
}
