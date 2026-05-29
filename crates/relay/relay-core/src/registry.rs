use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use cli_pocket_proto::ServerId;
use parking_lot::Mutex;
use tokio::sync::mpsc;

/// Message sent from the relay router to a server's websocket writer task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMsg {
    Ctrl(Bytes),
    Data(Bytes),
    Close,
}

#[derive(Debug)]
pub struct ServerSlot {
    pub server_id: ServerId,
    pub tx: mpsc::Sender<ServerMsg>,
}

impl ServerSlot {
    #[must_use]
    pub fn new(server_id: ServerId, tx: mpsc::Sender<ServerMsg>) -> Self {
        Self { server_id, tx }
    }
}

#[derive(Default, Clone)]
pub struct ServerRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    servers: Mutex<HashMap<ServerId, StoredServerSlot>>,
    next_generation: AtomicU64,
}

struct StoredServerSlot {
    slot: ServerSlot,
    generation: u64,
}

pub struct ServerRegistration {
    server_id: ServerId,
    generation: u64,
    inner: Arc<RegistryInner>,
}

impl ServerRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, slot: ServerSlot) -> crate::RelayResult<ServerRegistration> {
        let server_id = slot.server_id;
        let mut servers = self.inner.servers.lock();
        if servers.contains_key(&server_id) {
            return Err(crate::RelayError::Protocol("duplicate server registration"));
        }
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        servers.insert(server_id, StoredServerSlot { slot, generation });
        Ok(ServerRegistration {
            server_id,
            generation,
            inner: Arc::clone(&self.inner),
        })
    }

    #[must_use]
    pub fn get(&self, server_id: &ServerId) -> Option<mpsc::Sender<ServerMsg>> {
        self.inner
            .servers
            .lock()
            .get(server_id)
            .map(|stored| stored.slot.tx.clone())
    }

    #[must_use]
    pub fn list_ids(&self) -> Vec<ServerId> {
        self.inner.servers.lock().keys().copied().collect()
    }

    #[must_use]
    pub fn unregister(&self, registration: &ServerRegistration) -> bool {
        registration.unregister()
    }

    #[must_use]
    pub fn clone_handle(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ServerRegistration {
    fn unregister(&self) -> bool {
        let mut servers = self.inner.servers.lock();
        let should_remove = servers
            .get(&self.server_id)
            .is_some_and(|stored| stored.generation == self.generation);
        if should_remove {
            servers.remove(&self.server_id);
        }
        should_remove
    }
}

impl Drop for ServerRegistration {
    fn drop(&mut self) {
        self.unregister();
    }
}
