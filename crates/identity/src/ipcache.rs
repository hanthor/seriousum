//! IPCache primitives ported from `cilium/pkg/ipcache/ipcache.go`.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use seriousum_core::NumericIdentity;

/// Single prefix-to-identity mapping entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IPPrefixEntry {
    /// Stored IP prefix.
    pub prefix: IpNet,
    /// Security identity associated with the prefix.
    pub identity: NumericIdentity,
    /// Optional tunnel peer for encapsulated traffic.
    pub tunnel_peer: Option<IpAddr>,
    /// Encryption key index.
    pub encrypt_key: u8,
}

/// In-memory prefix cache with longest-prefix-match lookups.
#[derive(Debug, Clone, Default)]
pub struct IPCache {
    entries: Arc<RwLock<HashMap<IpNet, IPPrefixEntry>>>,
}

impl IPCache {
    /// Creates an empty IPCache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Inserts or replaces a prefix entry.
    pub fn upsert(&self, prefix: IpNet, identity: NumericIdentity) {
        let entry = IPPrefixEntry {
            prefix,
            identity,
            tunnel_peer: None,
            encrypt_key: 0,
        };

        self.entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(prefix, entry);
    }

    /// Removes a prefix entry.
    pub fn delete(&self, prefix: &IpNet) -> bool {
        self.entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(prefix)
            .is_some()
    }

    /// Looks up an exact prefix entry.
    #[must_use]
    pub fn lookup_by_prefix(&self, prefix: &IpNet) -> Option<IPPrefixEntry> {
        self.entries
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(prefix)
            .cloned()
    }

    /// Looks up the identity for an IP using longest-prefix-match semantics.
    #[must_use]
    pub fn lookup_by_ip(&self, ip: IpAddr) -> Option<IPPrefixEntry> {
        self.entries
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|entry| entry.prefix.contains(&ip))
            .max_by_key(|entry| entry.prefix.prefix_len())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipcache_lookup_most_specific() {
        let cache = IPCache::new();
        cache.upsert("10.0.0.0/8".parse().unwrap(), NumericIdentity::new(1));
        cache.upsert("10.1.0.0/16".parse().unwrap(), NumericIdentity::new(2));
        cache.upsert("10.1.2.3/32".parse().unwrap(), NumericIdentity::new(3));

        let result = cache.lookup_by_ip("10.1.2.3".parse().unwrap());

        assert_eq!(result.unwrap().identity, NumericIdentity::new(3));
    }

    #[test]
    fn test_ipcache_delete() {
        let cache = IPCache::new();
        let prefix: IpNet = "10.0.0.0/8".parse().unwrap();

        cache.upsert(prefix, NumericIdentity::new(42));

        assert!(cache.delete(&prefix));
        assert!(cache.lookup_by_prefix(&prefix).is_none());
        assert!(!cache.delete(&prefix));
    }

    #[test]
    fn test_ipcache_lookup_by_prefix() {
        let cache = IPCache::new();
        let prefix: IpNet = "2001:db8::/64".parse().unwrap();

        cache.upsert(prefix, NumericIdentity::new(7));

        let entry = cache.lookup_by_prefix(&prefix).unwrap();
        assert_eq!(entry.prefix, prefix);
        assert_eq!(entry.identity, NumericIdentity::new(7));
    }

    #[test]
    fn test_ipcache_returns_none_for_unmatched_ip() {
        let cache = IPCache::new();
        cache.upsert("10.0.0.0/24".parse().unwrap(), NumericIdentity::new(9));

        assert!(cache.lookup_by_ip("192.168.0.1".parse().unwrap()).is_none());
    }
}
