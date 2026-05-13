//! Pure key-value store data structures and an in-memory backend.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

/// Base prefix for all kvstore keys.
pub const BASE_KEY_PREFIX: &str = "cilium";
/// Prefix used for persisted Cilium state.
pub const STATE_PREFIX: &str = "cilium/state";
/// Prefix used for cached remote-cluster data.
pub const CACHE_PREFIX: &str = "cilium/cache";
/// Path used to validate quorum during initialization.
pub const INIT_LOCK_PATH: &str = "cilium/.initlock";
/// Path updated periodically with the operator heartbeat.
pub const HEARTBEAT_PATH: &str = "cilium/.heartbeat";
/// Prefix containing cluster configuration.
pub const CLUSTER_CONFIG_PREFIX: &str = "cilium/cluster-config";
/// Prefix used to signal external-source synchronization completion.
pub const SYNCED_PREFIX: &str = "cilium/synced";
/// Interval used for heartbeat writes.
pub const HEARTBEAT_WRITE_INTERVAL: Duration = Duration::from_mins(1);

/// A kvstore key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Key(pub String);

impl Key {
    /// Creates a new key from an owned or borrowed string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the underlying key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Joins this key with another path segment using `/`.
    pub fn join(&self, suffix: &str) -> Self {
        Self(format!(
            "{}/{}",
            self.0.trim_end_matches('/'),
            suffix.trim_start_matches('/'),
        ))
    }

    /// Returns true if this key starts with the provided prefix.
    pub fn has_prefix(&self, prefix: &Key) -> bool {
        self.0.starts_with(prefix.as_str())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Value stored in the kvstore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Value {
    /// Raw value bytes.
    pub data: Vec<u8>,
    /// Monotonic revision number for the value.
    pub revision: u64,
}

impl Value {
    /// Creates a new value with a zero revision.
    pub fn new(data: impl Into<Vec<u8>>) -> Self {
        Self {
            data: data.into(),
            revision: 0,
        }
    }

    /// Returns a copy of this value with the provided revision.
    pub fn with_revision(mut self, rev: u64) -> Self {
        self.revision = rev;
        self
    }

    /// Returns the value as a UTF-8 string, replacing invalid bytes lossily.
    pub fn as_str(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.data)
    }
}

/// Map of kvstore keys to values.
pub type KeyValuePairs = BTreeMap<Key, Value>;

/// A watch event emitted for kvstore mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchEvent {
    /// A key was created or updated.
    Put { key: Key, value: Value },
    /// A key was deleted.
    Delete { key: Key },
}

impl WatchEvent {
    /// Returns the key affected by the event.
    pub fn key(&self) -> &Key {
        match self {
            Self::Put { key, .. } | Self::Delete { key } => key,
        }
    }
}

/// Errors returned by kvstore data-model operations.
#[derive(Debug, thiserror::Error)]
pub enum KVStoreError {
    /// The requested key was not found.
    #[error("key not found: {0}")]
    NotFound(String),
    /// The backend could not be reached.
    #[error("connection error: {0}")]
    Connection(String),
    /// Serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// A lease-backed key expired.
    #[error("lease expired")]
    LeaseExpired,
    /// The operation timed out.
    #[error("operation timed out")]
    Timeout,
}

/// Backend abstraction for kvstore operations without network-specific behavior.
#[async_trait]
pub trait KVStoreBackend: Send + Sync {
    /// Retrieves a value for the provided key.
    async fn get(&self, key: &Key) -> Result<Option<Value>, KVStoreError>;

    /// Creates or replaces a value for the provided key.
    async fn set(&self, key: Key, value: Vec<u8>) -> Result<(), KVStoreError>;

    /// Deletes the provided key if it exists.
    async fn delete(&self, key: &Key) -> Result<(), KVStoreError>;

    /// Lists all `(key, value)` pairs under the provided prefix.
    async fn list_prefix(&self, prefix: &Key) -> Result<Vec<(Key, Value)>, KVStoreError>;

    /// Creates a key only if it does not already exist.
    async fn create_only(&self, key: Key, value: Vec<u8>) -> Result<bool, KVStoreError>;
}

/// Pure in-memory kvstore backend for tests and offline execution.
#[derive(Debug, Default, Clone)]
pub struct MemoryBackend {
    store: Arc<RwLock<BTreeMap<Key, Value>>>,
    revision: Arc<AtomicU64>,
}

impl MemoryBackend {
    /// Creates an empty in-memory backend.
    pub fn new() -> Self {
        Self::default()
    }

    fn next_rev(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::SeqCst) + 1
    }
}

#[async_trait]
impl KVStoreBackend for MemoryBackend {
    async fn get(&self, key: &Key) -> Result<Option<Value>, KVStoreError> {
        Ok(self.store.read().await.get(key).cloned())
    }

    async fn set(&self, key: Key, value: Vec<u8>) -> Result<(), KVStoreError> {
        let revision = self.next_rev();
        self.store.write().await.insert(
            key.clone(),
            Value {
                data: value,
                revision,
            },
        );
        debug!(%key, revision, "stored kvstore value");
        Ok(())
    }

    async fn delete(&self, key: &Key) -> Result<(), KVStoreError> {
        self.store.write().await.remove(key);
        debug!(%key, "deleted kvstore key");
        Ok(())
    }

    async fn list_prefix(&self, prefix: &Key) -> Result<Vec<(Key, Value)>, KVStoreError> {
        let items = self
            .store
            .read()
            .await
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.has_prefix(prefix))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Ok(items)
    }

    async fn create_only(&self, key: Key, value: Vec<u8>) -> Result<bool, KVStoreError> {
        let mut store = self.store.write().await;
        if store.contains_key(&key) {
            debug!(%key, "skipped create_only for existing key");
            return Ok(false);
        }

        let revision = self.next_rev();
        store.insert(
            key.clone(),
            Value {
                data: value,
                revision,
            },
        );
        debug!(%key, revision, "created kvstore value");
        Ok(true)
    }
}

/// A namespaced view of a kvstore backend.
#[derive(Clone)]
pub struct KVStore {
    prefix: Key,
    backend: Arc<dyn KVStoreBackend>,
}

impl KVStore {
    /// Creates a namespaced kvstore wrapper over the provided backend.
    pub fn new(prefix: impl Into<String>, backend: Arc<dyn KVStoreBackend>) -> Self {
        Self {
            prefix: Key::new(prefix),
            backend,
        }
    }

    fn full_key(&self, key: &Key) -> Key {
        self.prefix.join(key.as_str())
    }

    /// Retrieves a value from the namespaced backend.
    pub async fn get(&self, key: &Key) -> Result<Option<Value>, KVStoreError> {
        self.backend.get(&self.full_key(key)).await
    }

    /// Stores a value in the namespaced backend.
    pub async fn set(&self, key: Key, value: Vec<u8>) -> Result<(), KVStoreError> {
        self.backend.set(self.full_key(&key), value).await
    }

    /// Deletes a value from the namespaced backend.
    pub async fn delete(&self, key: &Key) -> Result<(), KVStoreError> {
        self.backend.delete(&self.full_key(key)).await
    }

    /// Lists values from the namespaced backend with the provided sub-prefix.
    pub async fn list_prefix(&self, sub_prefix: &Key) -> Result<Vec<(Key, Value)>, KVStoreError> {
        self.backend.list_prefix(&self.full_key(sub_prefix)).await
    }

    /// Creates a value only if the key is absent in the namespaced backend.
    pub async fn create_only(&self, key: Key, value: Vec<u8>) -> Result<bool, KVStoreError> {
        self.backend.create_only(self.full_key(&key), value).await
    }
}

/// Returns the lock path for the given key path.
///
/// Mirrors `getLockPath` in `pkg/kvstore/lock.go`.
pub fn get_lock_path(path: &str) -> String {
    format!("{path}.lock")
}

/// Extracts the `scope/version` segment from a kvstore key.
///
/// Mirrors `GetScopeFromKey` in `pkg/kvstore/metrics.go`.
pub fn get_scope_from_key(key: &str) -> &str {
    let parts: Vec<&str> = key.splitn(5, '/').collect();
    if parts.len() < 4 {
        if key.len() >= 12 {
            return &key[..12];
        }
        return key;
    }

    let start = parts[0].len() + 1 + parts[1].len() + 1;
    let mid = start + parts[2].len() + 1;
    let end = mid + parts[3].len();
    &key[start..end]
}

/// Converts a state prefix to its cache equivalent.
///
/// Mirrors `StateToCachePrefix` in `pkg/kvstore/kvstore.go`.
pub fn state_to_cache_prefix(prefix: &str) -> Cow<'_, str> {
    if let Some(rest) = prefix.strip_prefix(STATE_PREFIX) {
        Cow::Owned(format!("{CACHE_PREFIX}{rest}"))
    } else {
        Cow::Borrowed(prefix)
    }
}

/// Converts a state prefix to its cache equivalent and returns an owned string.
pub fn state_to_cache_prefix_owned(prefix: &str) -> String {
    state_to_cache_prefix(prefix).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_get_delete() {
        let store = KVStore::new("cilium/test", Arc::new(MemoryBackend::new()));
        let key = Key::new("foo");

        store.set(key.clone(), b"bar".to_vec()).await.unwrap();
        let value = store.get(&key).await.unwrap().unwrap();
        assert_eq!(value.as_str(), "bar");

        store.delete(&key).await.unwrap();
        assert!(store.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_prefix() {
        let store = KVStore::new("cilium", Arc::new(MemoryBackend::new()));
        store.set(Key::new("ids/1"), b"a".to_vec()).await.unwrap();
        store.set(Key::new("ids/2"), b"b".to_vec()).await.unwrap();
        store.set(Key::new("nodes/1"), b"c".to_vec()).await.unwrap();

        let ids = store.list_prefix(&Key::new("ids")).await.unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].0.as_str(), "cilium/ids/1");
        assert_eq!(ids[1].0.as_str(), "cilium/ids/2");
    }

    #[tokio::test]
    async fn test_create_only() {
        let store = KVStore::new("cilium", Arc::new(MemoryBackend::new()));
        let key = Key::new("lock");

        assert!(store.create_only(key.clone(), b"1".to_vec()).await.unwrap());
        assert!(!store.create_only(key.clone(), b"2".to_vec()).await.unwrap());
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_str(), "1");
    }

    #[tokio::test]
    async fn test_revision_monotonic() {
        let backend = Arc::new(MemoryBackend::new());
        let store = KVStore::new("t", backend);

        store.set(Key::new("a"), b"1".to_vec()).await.unwrap();
        store.set(Key::new("b"), b"2".to_vec()).await.unwrap();

        let value_a = store.get(&Key::new("a")).await.unwrap().unwrap();
        let value_b = store.get(&Key::new("b")).await.unwrap().unwrap();
        assert!(value_b.revision > value_a.revision);
    }

    #[test]
    fn test_key_join_and_prefix() {
        let base = Key::new("cilium/state");
        let full = base.join("identities/1");
        assert_eq!(full.as_str(), "cilium/state/identities/1");
        assert!(full.has_prefix(&base));
        assert!(!base.has_prefix(&full));
    }

    #[test]
    fn test_watch_event_key_accessor() {
        let key = Key::new("cilium/state/ids/1");
        let event = WatchEvent::Delete { key: key.clone() };
        assert_eq!(event.key(), &key);
    }

    /// Ported from `TestGetLockPath`.
    #[test]
    fn test_get_lock_path() {
        let path = "foo/path";
        assert_eq!(get_lock_path(path), format!("{path}.lock"));
    }

    /// Ported from `TestValidateScopesFromKey`.
    #[test]
    fn test_get_scope_from_key() {
        let cases = [
            ("cilium/state/identities/v1/id", "identities/v1"),
            (
                "cilium/state/identities/v1/value/Y29udGFpbmVyOmlkPWFwcDE7Y29udGFpbmVyOmlkLnNlcnZpY2UxPTs=",
                "identities/v1",
            ),
            ("cilium/state/ip/v1/default/10.15.189.183", "ip/v1"),
            ("cilium/state/ip/v1/default/f00d::a0f:0:0:6f2e", "ip/v1"),
            ("cilium/state/nodes/v1/default/runtime", "nodes/v1"),
            ("cilium/state/nodes/v1", "nodes/v1"),
        ];

        for (key, expected) in cases {
            assert_eq!(
                get_scope_from_key(key),
                expected,
                "get_scope_from_key({key:?})"
            );
        }
    }

    /// Ported from `TestStateToCachePrefix`.
    #[test]
    fn test_state_to_cache_prefix() {
        let cases = [
            (
                "a prefix starting with cilium/state",
                "cilium/state/foo/bar",
                "cilium/cache/foo/bar",
            ),
            (
                "a prefix not starting with cilium/state",
                "cilium/foo/bar",
                "cilium/foo/bar",
            ),
            (
                "a prefix containing but not starting with cilium/state",
                "cilium/foo/bar/cilium/state/qux",
                "cilium/foo/bar/cilium/state/qux",
            ),
        ];

        for (name, input, expected) in cases {
            assert_eq!(state_to_cache_prefix_owned(input), expected, "{name}");
        }
    }
}
