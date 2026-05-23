//! File-backed [`KeyValueStore`] for native (desktop + mobile) builds.
//!
//! All entries are persisted in a single JSON file:
//! `<data_dir>/cli-pocket/store.json`.  Values are base64-encoded so the file
//! is human-readable and can be edited or backed up with standard tools.
//!
//! Writes are atomic: the new content is first written to a `.tmp` sibling,
//! then renamed over the target file.  On Unix the file is chmod 0o600 after
//! each write so private keys are not readable by other users.

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use cli_pocket_client_core::{ClientError, ClientResult, KeyValueStore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// On-disk representation
// ---------------------------------------------------------------------------

/// What gets serialised into `store.json`.
#[derive(Default, Serialize, Deserialize)]
struct Disk {
    /// keys -> base64-encoded values
    entries: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// In-memory state
// ---------------------------------------------------------------------------

struct Inner {
    file: PathBuf,
    mem: BTreeMap<String, Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Public type
// ---------------------------------------------------------------------------

/// [`KeyValueStore`] backed by an atomic JSON file on the local filesystem.
///
/// Clone is cheap — all clones share the same underlying mutex.
#[derive(Clone)]
pub struct FileKvStore {
    inner: Arc<Mutex<Inner>>,
}

impl FileKvStore {
    /// Open (or create) the store at the platform data directory:
    /// `<data_dir>/cli-pocket/store.json`.
    ///
    /// Uses [`directories::ProjectDirs`] to locate `data_dir`; fails if the
    /// platform cannot provide a suitable path (unusual, but possible in some
    /// sandboxed environments).
    pub fn open_default() -> ClientResult<Self> {
        let dirs =
            directories::ProjectDirs::from("dev", "cli-pocket", "cli-pocket").ok_or_else(|| {
                ClientError::Identity("cannot resolve platform data directory".into())
            })?;
        Self::open_at(dirs.data_dir())
    }

    /// Open (or create) the store rooted at `dir`.
    ///
    /// `dir` is created if it does not exist.  The JSON file is placed at
    /// `<dir>/store.json`.  This constructor is primarily used by tests.
    pub fn open_at(dir: &Path) -> ClientResult<Self> {
        std::fs::create_dir_all(dir)
            .map_err(|e| ClientError::Identity(format!("create_dir_all: {e}")))?;

        let file = dir.join("store.json");

        let mem = match std::fs::read(&file) {
            Ok(bytes) => {
                let disk: Disk = serde_json::from_slice(&bytes)
                    .map_err(|e| ClientError::Identity(format!("parse store.json: {e}")))?;
                disk.entries
                    .into_iter()
                    .map(|(k, v)| {
                        // Silently drop entries whose base64 is malformed; the
                        // worst outcome is re-creating the identity on next run,
                        // which is safe.
                        let raw = B64.decode(v.as_bytes()).unwrap_or_default();
                        (k, raw)
                    })
                    .collect()
            }
            Err(e) if e.kind() == ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(ClientError::Identity(format!("read store.json: {e}"))),
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner { file, mem })),
        })
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Atomically flush the in-memory map to disk.
    ///
    /// Writes to `<file>.tmp` then renames over `<file>`.  On Unix, sets
    /// permissions to `0o600` after the rename.
    fn flush(inner: &Inner) -> ClientResult<()> {
        let disk = Disk {
            entries: inner
                .mem
                .iter()
                .map(|(k, v)| (k.clone(), B64.encode(v)))
                .collect(),
        };

        let tmp = inner.file.with_extension("json.tmp");

        let bytes = serde_json::to_vec_pretty(&disk)
            .map_err(|e| ClientError::Identity(format!("serialize store: {e}")))?;

        std::fs::write(&tmp, &bytes)
            .map_err(|e| ClientError::Identity(format!("write store.tmp: {e}")))?;

        std::fs::rename(&tmp, &inner.file)
            .map_err(|e| ClientError::Identity(format!("rename store.tmp: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&inner.file, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Trait impl
// ---------------------------------------------------------------------------

#[async_trait(?Send)]
impl KeyValueStore for FileKvStore {
    async fn get(&self, key: &str) -> ClientResult<Option<Vec<u8>>> {
        let g = self.inner.lock().await;
        Ok(g.mem.get(key).cloned())
    }

    async fn put(&self, key: &str, value: &[u8]) -> ClientResult<()> {
        let mut g = self.inner.lock().await;
        g.mem.insert(key.to_owned(), value.to_vec());
        Self::flush(&g)
    }

    async fn delete(&self, key: &str) -> ClientResult<()> {
        let mut g = self.inner.lock().await;
        g.mem.remove(key);
        Self::flush(&g)
    }
}
