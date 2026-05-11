//! In-memory key-value store primitives.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// A simple asynchronous key-value store.
#[derive(Debug, Clone, Default)]
pub struct KvStore {
    inner: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl KvStore {
    /// Creates a new store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or updates a value.
    pub async fn set(&self, key: impl Into<String>, value: impl Into<Vec<u8>>) {
        self.inner.write().await.insert(key.into(), value.into());
    }

    /// Gets a value.
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.read().await.get(key).cloned()
    }

    /// Deletes a value.
    pub async fn delete(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.write().await.remove(key)
    }

    /// Returns true if the key exists.
    pub async fn contains(&self, key: &str) -> bool {
        self.inner.read().await.contains_key(key)
    }

    /// Returns all keys with the given prefix.
    pub async fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.inner
            .read()
            .await
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn store_roundtrip() {
        let store = KvStore::new();
        store.set("a", b"b".to_vec()).await;
        assert_eq!(store.get("a").await, Some(b"b".to_vec()));
        assert!(store.contains("a").await);
        assert_eq!(store.delete("a").await, Some(b"b".to_vec()));
    }
}
