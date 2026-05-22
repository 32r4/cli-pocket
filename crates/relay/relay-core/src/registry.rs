use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use cli_pocket_proto::HostId;
use parking_lot::Mutex;
use tokio::sync::mpsc;

/// Message sent from the relay router to a host's websocket writer task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostMsg {
    Ctrl(Bytes),
    Data(Bytes),
    Close,
}

#[derive(Debug)]
pub struct HostSlot {
    pub host_id: HostId,
    pub tx: mpsc::Sender<HostMsg>,
}

impl HostSlot {
    #[must_use]
    pub fn new(host_id: HostId, tx: mpsc::Sender<HostMsg>) -> Self {
        Self { host_id, tx }
    }
}

#[derive(Default, Clone)]
pub struct HostRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    hosts: Mutex<HashMap<HostId, StoredHostSlot>>,
    next_generation: AtomicU64,
}

struct StoredHostSlot {
    slot: HostSlot,
    generation: u64,
}

pub struct HostRegistration {
    host_id: HostId,
    generation: u64,
    inner: Arc<RegistryInner>,
}

impl HostRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, slot: HostSlot) -> crate::RelayResult<HostRegistration> {
        let host_id = slot.host_id;
        let mut hosts = self.inner.hosts.lock();
        if hosts.contains_key(&host_id) {
            return Err(crate::RelayError::Protocol("duplicate host registration"));
        }
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        hosts.insert(host_id, StoredHostSlot { slot, generation });
        Ok(HostRegistration {
            host_id,
            generation,
            inner: Arc::clone(&self.inner),
        })
    }

    #[must_use]
    pub fn get(&self, host_id: &HostId) -> Option<mpsc::Sender<HostMsg>> {
        self.inner
            .hosts
            .lock()
            .get(host_id)
            .map(|stored| stored.slot.tx.clone())
    }

    #[must_use]
    pub fn list_ids(&self) -> Vec<HostId> {
        self.inner.hosts.lock().keys().copied().collect()
    }

    #[must_use]
    pub fn unregister(&self, registration: &HostRegistration) -> bool {
        registration.unregister()
    }

    #[must_use]
    pub fn clone_handle(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl HostRegistration {
    fn unregister(&self) -> bool {
        let mut hosts = self.inner.hosts.lock();
        let should_remove = hosts
            .get(&self.host_id)
            .is_some_and(|stored| stored.generation == self.generation);
        if should_remove {
            hosts.remove(&self.host_id);
        }
        should_remove
    }
}

impl Drop for HostRegistration {
    fn drop(&mut self) {
        self.unregister();
    }
}
