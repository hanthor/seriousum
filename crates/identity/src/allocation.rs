//! Local identity allocation primitives ported from `cilium/pkg/identity/cache/local.go`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use seriousum_core::NumericIdentity;
use thiserror::Error;
use tracing::warn;

/// Ordered label set using `source:key -> value` entries.
///
/// A `BTreeMap` is used so label sets have a stable representation and can be
/// used as keys in the allocator's forward map.
pub type LabelSet = BTreeMap<String, String>;

/// Callback invoked when identities are created or removed.
pub trait IdentityNotifier: Send + Sync {
    /// Called when an identity is allocated for a label set.
    fn identity_updated(&self, id: NumericIdentity, labels: &LabelSet);

    /// Called when an identity is fully released.
    fn identity_removed(&self, id: NumericIdentity);
}

#[derive(Debug, Error, PartialEq, Eq)]
enum LocalIdentityCacheError {
    #[error("out of local identity space ({min_id}..={max_id})")]
    Exhausted { min_id: u32, max_id: u32 },
}

/// Pure in-memory identity allocator for local identities.
#[derive(Debug, Clone)]
pub struct LocalIdentityCache {
    identities: Arc<RwLock<HashMap<LabelSet, NumericIdentity>>>,
    reverse: Arc<RwLock<HashMap<NumericIdentity, (LabelSet, u32)>>>,
    next_id: Arc<Mutex<u32>>,
    min_id: u32,
    max_id: u32,
}

impl LocalIdentityCache {
    /// Creates a new local identity cache for the inclusive numeric range.
    #[must_use]
    pub fn new(min_id: u32, max_id: u32) -> Self {
        assert!(min_id <= max_id, "min_id must be <= max_id");

        Self {
            identities: Arc::new(RwLock::new(HashMap::new())),
            reverse: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(min_id)),
            min_id,
            max_id,
        }
    }

    /// Allocates an identity for the provided labels.
    ///
    /// Returns the existing identity with `is_new = false` when the label set is
    /// already present; otherwise allocates a new numeric identity.
    #[must_use]
    pub fn allocate(&self, labels: LabelSet) -> (NumericIdentity, bool) {
        let mut identities = self
            .identities
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(id) = identities.get(&labels).copied() {
            let mut reverse = self
                .reverse
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((_, refcount)) = reverse.get_mut(&id) {
                *refcount += 1;
            }
            return (id, false);
        }

        let mut reverse = self
            .reverse
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = match self.next_free_id(&reverse) {
            Ok(id) => id,
            Err(error) => {
                warn!(min_id = self.min_id, max_id = self.max_id, "{error}");
                panic!("{error}");
            }
        };

        identities.insert(labels.clone(), id);
        reverse.insert(id, (labels, 1));

        (id, true)
    }

    /// Releases one reference to the provided identity.
    ///
    /// Returns `true` when the identity reaches a zero reference count and is
    /// removed from the cache.
    pub fn release(&self, id: NumericIdentity) -> bool {
        let mut identities = self
            .identities
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut reverse = self
            .reverse
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let labels = {
            let Some((labels, refcount)) = reverse.get_mut(&id) else {
                return false;
            };

            if *refcount > 1 {
                *refcount -= 1;
                return false;
            }

            labels.clone()
        };

        reverse.remove(&id);
        identities.remove(&labels);
        true
    }

    /// Returns the labels associated with an identity.
    #[must_use]
    pub fn lookup_by_id(&self, id: NumericIdentity) -> Option<LabelSet> {
        self.reverse
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
            .map(|(labels, _)| labels.clone())
    }

    /// Returns the identity associated with a label set.
    #[must_use]
    pub fn lookup_by_labels(&self, labels: &LabelSet) -> Option<NumericIdentity> {
        self.identities
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(labels)
            .copied()
    }

    fn next_free_id(
        &self,
        reverse: &HashMap<NumericIdentity, (LabelSet, u32)>,
    ) -> Result<NumericIdentity, LocalIdentityCacheError> {
        let mut next_id = self
            .next_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut candidate = *next_id;
        let capacity = (u64::from(self.max_id) - u64::from(self.min_id) + 1) as usize;

        for _ in 0..capacity {
            let id = NumericIdentity::new(candidate);
            if !reverse.contains_key(&id) {
                *next_id = self.bump_id(candidate);
                return Ok(id);
            }
            candidate = self.bump_id(candidate);
        }

        Err(LocalIdentityCacheError::Exhausted {
            min_id: self.min_id,
            max_id: self.max_id,
        })
    }

    fn bump_id(&self, candidate: u32) -> u32 {
        if candidate == self.max_id {
            self.min_id
        } else {
            candidate + 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_cache_allocate_same_labels_returns_same_id() {
        let cache = LocalIdentityCache::new(100, 200);
        let labels = BTreeMap::from([("k8s:app".to_string(), "web".to_string())]);
        let (id1, new1) = cache.allocate(labels.clone());
        let (id2, new2) = cache.allocate(labels.clone());

        assert_eq!(id1, id2);
        assert!(new1);
        assert!(!new2);
    }

    #[test]
    fn test_local_cache_release_frees_when_refcount_zero() {
        let cache = LocalIdentityCache::new(100, 101);
        let first = BTreeMap::from([("k8s:app".to_string(), "web".to_string())]);
        let second = BTreeMap::from([("k8s:app".to_string(), "api".to_string())]);
        let third = BTreeMap::from([("k8s:app".to_string(), "jobs".to_string())]);

        let (id1, _) = cache.allocate(first.clone());
        let _ = cache.allocate(first.clone());
        let (id2, _) = cache.allocate(second);

        assert!(!cache.release(id1));
        assert_eq!(cache.lookup_by_id(id1), Some(first.clone()));
        assert!(cache.release(id1));
        assert_eq!(cache.lookup_by_id(id1), None);

        let (id3, is_new) = cache.allocate(third);
        assert!(is_new);
        assert_eq!(id2, NumericIdentity::new(101));
        assert_eq!(id3, id1);
    }

    #[test]
    fn test_local_cache_lookup_methods() {
        let cache = LocalIdentityCache::new(500, 600);
        let labels = BTreeMap::from([
            ("cidr:10.0.0.0/24".to_string(), String::new()),
            ("reserved:world".to_string(), String::new()),
        ]);

        let (id, _) = cache.allocate(labels.clone());

        assert_eq!(cache.lookup_by_labels(&labels), Some(id));
        assert_eq!(cache.lookup_by_id(id), Some(labels));
    }

    #[test]
    fn test_local_cache_release_unknown_id_returns_false() {
        let cache = LocalIdentityCache::new(1, 2);
        assert!(!cache.release(NumericIdentity::new(1)));
    }
}
