use async_trait::async_trait;

/// Bidirectional byte transport for a client session.
///
/// Mirrors the native transport contract closely enough that native adapters
/// can wrap `cli-pocket-transport`, while wasm adapters can bridge to
/// `web-sys` without pulling in tokio.
#[async_trait(?Send)]
pub trait Transport {
    async fn send(&mut self, bytes: Vec<u8>) -> crate::ClientResult<()>;
    async fn recv(&mut self) -> crate::ClientResult<Option<Vec<u8>>>;
    async fn close(&mut self) -> crate::ClientResult<()>;
}

/// Monotonic-ish clock abstraction for reconnect and timeout logic.
#[async_trait(?Send)]
pub trait Clock {
    fn now_ms(&self) -> u64;
    async fn sleep_ms(&self, ms: u64);
}

/// Cryptographically secure random byte source.
pub trait Rng {
    fn fill(&self, dest: &mut [u8]);
}

/// Persistent string-keyed byte store for identity material.
#[async_trait(?Send)]
pub trait KeyValueStore {
    async fn get(&self, key: &str) -> crate::ClientResult<Option<Vec<u8>>>;
    async fn put(&self, key: &str, value: &[u8]) -> crate::ClientResult<()>;
    async fn delete(&self, key: &str) -> crate::ClientResult<()>;
}
