use cli_pocket_proto::ClientId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRecord {
    pub client_id: ClientId,
    pub public_key: [u8; 32],
    pub paired_at: u64,
}

#[derive(Debug)]
pub struct ClientDb {
    inner: Arc<RwLock<State>>,
    clients_path: PathBuf,
    revoked_path: PathBuf,
    revocations_tx: watch::Sender<RevocationSet>,
    reload_task: JoinHandle<()>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevocationSet {
    revoked: HashSet<ClientId>,
}

#[derive(Debug, Clone, Default)]
struct State {
    by_id: HashMap<ClientId, ClientRecord>,
    by_public: HashMap<[u8; 32], ClientId>,
    revoked: HashSet<ClientId>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ClientsFile {
    clients: Vec<ClientRecord>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RevokedFile {
    revoked: Vec<ClientId>,
}

impl ClientDb {
    #[allow(clippy::unused_async)]
    pub async fn open(clients_path: &Path, revoked_path: &Path) -> crate::DaemonResult<Self> {
        ensure_file(clients_path, &ClientsFile::default())?;
        ensure_file(revoked_path, &RevokedFile::default())?;

        let clients = read_clients_file(clients_path)?;
        let revoked = read_revoked_file(revoked_path)?;
        let state = State::from_files(clients, revoked)?;
        let revocations = RevocationSet {
            revoked: state.revoked.clone(),
        };
        let (revocations_tx, _) = watch::channel(revocations);
        let inner = Arc::new(RwLock::new(state));
        let clients_text = read_text(clients_path);
        let revoked_text = read_text(revoked_path);

        let reload_task = spawn_reload_task(
            Arc::clone(&inner),
            clients_path.to_path_buf(),
            revoked_path.to_path_buf(),
            revocations_tx.clone(),
            clients_text,
            revoked_text,
        );

        Ok(Self {
            inner,
            clients_path: clients_path.to_path_buf(),
            revoked_path: revoked_path.to_path_buf(),
            revocations_tx,
            reload_task,
        })
    }

    pub async fn add(&self, record: ClientRecord) -> crate::DaemonResult<()> {
        let mut state = self.inner.write().await;
        let next = state.with_added(record)?;
        write_clients_file(&self.clients_path, &next.clients_file())?;
        *state = next;
        Ok(())
    }

    pub async fn add_or_lookup_by_public(
        &self,
        record: ClientRecord,
    ) -> crate::DaemonResult<ClientRecord> {
        let mut state = self.inner.write().await;
        if let Some(existing) = state
            .by_public
            .get(&record.public_key)
            .and_then(|client_id| state.by_id.get(client_id))
            .cloned()
        {
            return Ok(existing);
        }

        let next = state.with_added(record.clone())?;
        write_clients_file(&self.clients_path, &next.clients_file())?;
        *state = next;
        Ok(record)
    }

    pub async fn list(&self) -> Vec<ClientRecord> {
        let state = self.inner.read().await;
        let mut clients: Vec<_> = state.by_id.values().cloned().collect();
        clients.sort_by_key(|record| (record.paired_at, record.client_id.0));
        clients
    }

    pub async fn lookup_by_public(
        &self,
        public_key: &[u8; 32],
    ) -> crate::DaemonResult<Option<ClientRecord>> {
        let state = self.inner.read().await;
        Ok(state
            .by_public
            .get(public_key)
            .and_then(|client_id| state.by_id.get(client_id))
            .cloned())
    }

    pub async fn lookup_by_id(
        &self,
        client_id: &ClientId,
    ) -> crate::DaemonResult<Option<ClientRecord>> {
        let state = self.inner.read().await;
        Ok(state.by_id.get(client_id).cloned())
    }

    pub async fn revoke(&self, client_id: ClientId) -> crate::DaemonResult<()> {
        let mut state = self.inner.write().await;
        if let Some(next) = state.with_revoked(client_id) {
            write_revoked_file(&self.revoked_path, &next.revoked_file())?;
            *state = next;
            let _ = self.revocations_tx.send(RevocationSet {
                revoked: state.revoked.clone(),
            });
        }
        Ok(())
    }

    pub async fn is_revoked(&self, client_id: &ClientId) -> bool {
        self.inner.read().await.revoked.contains(client_id)
    }

    #[must_use]
    pub fn watch_revocations(&self) -> watch::Receiver<RevocationSet> {
        self.revocations_tx.subscribe()
    }
}

impl Drop for ClientDb {
    fn drop(&mut self) {
        self.reload_task.abort();
    }
}

impl RevocationSet {
    #[must_use]
    pub fn contains(&self, client_id: &ClientId) -> bool {
        self.revoked.contains(client_id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.revoked.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.revoked.is_empty()
    }
}

impl State {
    fn from_files(clients: ClientsFile, revoked: RevokedFile) -> crate::DaemonResult<Self> {
        let mut state = Self {
            revoked: revoked.revoked.into_iter().collect(),
            ..Self::default()
        };

        for record in clients.clients {
            state.check_new_record(&record)?;
            state.by_public.insert(record.public_key, record.client_id);
            state.by_id.insert(record.client_id, record);
        }

        Ok(state)
    }

    fn with_added(&self, record: ClientRecord) -> crate::DaemonResult<Self> {
        self.check_new_record(&record)?;
        let mut next = self.clone();
        next.by_public.insert(record.public_key, record.client_id);
        next.by_id.insert(record.client_id, record);
        Ok(next)
    }

    fn with_revoked(&self, client_id: ClientId) -> Option<Self> {
        if self.revoked.contains(&client_id) {
            return None;
        }

        let mut next = self.clone();
        next.revoked.insert(client_id);
        Some(next)
    }

    fn check_new_record(&self, record: &ClientRecord) -> crate::DaemonResult<()> {
        if self.by_id.contains_key(&record.client_id) {
            return Err(crate::DaemonError::ClientDb(format!(
                "duplicate client_id {}",
                record.client_id.0
            )));
        }

        if self.by_public.contains_key(&record.public_key) {
            return Err(crate::DaemonError::ClientDb(
                "duplicate public_key".to_string(),
            ));
        }

        Ok(())
    }

    fn clients_file(&self) -> ClientsFile {
        let mut clients: Vec<_> = self.by_id.values().cloned().collect();
        clients.sort_by_key(|record| (record.paired_at, record.client_id.0));
        ClientsFile { clients }
    }

    fn revoked_file(&self) -> RevokedFile {
        let mut revoked: Vec<_> = self.revoked.iter().copied().collect();
        revoked.sort_by_key(|client_id| client_id.0);
        RevokedFile { revoked }
    }
}

fn spawn_reload_task(
    inner: Arc<RwLock<State>>,
    clients_path: PathBuf,
    revoked_path: PathBuf,
    revocations_tx: watch::Sender<RevocationSet>,
    mut last_clients_text: Option<String>,
    mut last_revoked_text: Option<String>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(50)).await;

            let clients_text = read_text(&clients_path);
            let revoked_text = read_text(&revoked_path);
            if clients_text == last_clients_text && revoked_text == last_revoked_text {
                continue;
            }

            let Ok(clients) = read_clients_file(&clients_path) else {
                continue;
            };
            let Ok(revoked) = read_revoked_file(&revoked_path) else {
                continue;
            };

            let Ok(next) = State::from_files(clients, revoked) else {
                continue;
            };
            let revocations_changed = {
                let current = inner.read().await;
                current.revoked != next.revoked
            };

            {
                let mut current = inner.write().await;
                *current = next;
            }

            if revocations_changed {
                let revoked = inner.read().await.revoked.clone();
                let _ = revocations_tx.send(RevocationSet { revoked });
            }

            last_clients_text = clients_text;
            last_revoked_text = revoked_text;
        }
    })
}

fn ensure_file<T: Serialize>(path: &Path, default_value: &T) -> crate::DaemonResult<()> {
    if path.exists() {
        return Ok(());
    }

    write_json(path, default_value)
}

fn read_clients_file(path: &Path) -> crate::DaemonResult<ClientsFile> {
    read_json(path)
}

fn write_clients_file(path: &Path, file: &ClientsFile) -> crate::DaemonResult<()> {
    write_json(path, file)
}

fn read_revoked_file(path: &Path) -> crate::DaemonResult<RevokedFile> {
    read_json(path)
}

fn write_revoked_file(path: &Path, file: &RevokedFile) -> crate::DaemonResult<()> {
    write_json(path, file)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> crate::DaemonResult<T> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|error| crate::DaemonError::ClientDb(error.to_string()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> crate::DaemonResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let text = serde_json::to_string_pretty(value)
        .map_err(|error| crate::DaemonError::ClientDb(error.to_string()))?;
    write_atomic(path, text.as_bytes())?;
    Ok(())
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn write_atomic(path: &Path, contents: &[u8]) -> crate::DaemonResult<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)?;
    }

    for attempt in 0..100 {
        let tmp_path = temp_path(path, attempt)?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(mut file) => {
                let write_result = write_and_sync(&mut file, contents);
                drop(file);

                if let Err(error) = write_result {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(error.into());
                }

                if let Err(error) = std::fs::rename(&tmp_path, path) {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(error.into());
                }

                sync_parent(parent);
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }

    Err(crate::DaemonError::ClientDb(
        "failed to create temp file for atomic write".to_string(),
    ))
}

fn write_and_sync(file: &mut File, contents: &[u8]) -> std::io::Result<()> {
    file.write_all(contents)?;
    file.sync_all()
}

fn temp_path(path: &Path, attempt: u32) -> crate::DaemonResult<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::DaemonError::ClientDb("path has no file name".to_string()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    Ok(parent.join(format!(
        ".{file_name}.tmp.{}.{}.{}",
        std::process::id(),
        nanos,
        attempt
    )))
}

fn sync_parent(parent: Option<&Path>) {
    let Some(parent) = parent else {
        return;
    };
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file_without_temp_file_leftover() {
        let dir = create_temp_dir("atomic-write");
        let path = dir.join("clients.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");

        std::fs::remove_dir_all(dir).unwrap();
    }

    fn create_temp_dir(name: &str) -> PathBuf {
        for attempt in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "cli-pocket-client-db-unit-{name}-{}-{}-{attempt}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            if std::fs::create_dir(&path).is_ok() {
                return path;
            }
        }

        panic!("failed to create temp directory");
    }
}
