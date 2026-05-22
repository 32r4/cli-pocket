use async_trait::async_trait;

#[async_trait(?Send)]
pub trait Transport {
    async fn send(&mut self, bytes: &[u8]) -> crate::ClientResult<()>;
    async fn recv(&mut self) -> crate::ClientResult<Vec<u8>>;
    async fn close(&mut self) -> crate::ClientResult<()>;
}

#[async_trait(?Send)]
pub trait Clock {
    fn now_ms(&self) -> u64;
    async fn sleep_ms(&self, ms: u64);
}

pub trait Rng {
    fn fill(&self, dest: &mut [u8]);
}

#[async_trait(?Send)]
pub trait KeyValueStore {
    async fn get(&self, key: &str) -> crate::ClientResult<Option<Vec<u8>>>;
    async fn put(&self, key: &str, value: &[u8]) -> crate::ClientResult<()>;
    async fn delete(&self, key: &str) -> crate::ClientResult<()>;
}
