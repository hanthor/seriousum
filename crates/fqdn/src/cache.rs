//! DNS cache with TTL-based expiration
//!
//! Manages DNS lookup results with automatic expiration. Tracks both forward
//! (name → IPs) and reverse (IP → names) lookups for efficient queries.

use crate::error::{Error, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// DNS cache entry with TTL information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// DNS name (may be unqualified)
    pub name: String,

    /// IP addresses returned by DNS lookup
    pub ips: Vec<IpAddr>,

    /// TTL in seconds
    pub ttl: u32,

    /// Time when this entry was created (Unix timestamp)
    pub lookup_time: u64,

    /// Time when this entry expires (Unix timestamp)
    pub expiration_time: u64,
}

impl CacheEntry {
    /// Creates a new cache entry with immediate expiration calculation
    pub fn new(name: impl Into<String>, ips: Vec<IpAddr>, ttl: u32) -> Self {
        let lookup_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiration_time = lookup_time + u64::from(ttl);

        Self {
            name: name.into(),
            ips,
            ttl,
            lookup_time,
            expiration_time,
        }
    }

    /// Checks if this entry has expired at the given time
    pub fn is_expired_at(&self, now: u64) -> bool {
        now >= self.expiration_time
    }

    /// Checks if this entry is currently expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.is_expired_at(now)
    }

    /// Returns remaining TTL in seconds (0 if expired)
    pub fn remaining_ttl(&self) -> u32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if self.is_expired_at(now) {
            0
        } else {
            (self.expiration_time - now) as u32
        }
    }
}

/// Update status after cache modification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateStatus {
    /// Whether any cache entry was updated
    pub updated: bool,

    /// Whether any new IP-to-name mapping was added
    pub upserted: bool,
}

/// DNS cache managing forward (name → IP) and reverse (IP → name) lookups
#[derive(Debug, Clone)]
pub struct DnsCache {
    /// Forward DNS lookups: name → IPs with TTL
    forward: Arc<DashMap<String, CacheEntry>>,

    /// Reverse DNS lookups: IP → name list
    reverse: Arc<DashMap<IpAddr, Vec<String>>>,

    /// Minimum TTL to enforce (overrides shorter TTLs)
    min_ttl: u32,

    /// Maximum IPs per hostname (0 = unlimited)
    per_host_limit: usize,
}

impl DnsCache {
    /// Creates a new DNS cache with minimum TTL
    pub fn new(min_ttl: u32) -> Self {
        Self {
            forward: Arc::new(DashMap::new()),
            reverse: Arc::new(DashMap::new()),
            min_ttl,
            per_host_limit: 0,
        }
    }

    /// Creates a new DNS cache with minimum TTL and per-host limit
    pub fn with_limits(min_ttl: u32, per_host_limit: usize) -> Self {
        Self {
            forward: Arc::new(DashMap::new()),
            reverse: Arc::new(DashMap::new()),
            min_ttl,
            per_host_limit,
        }
    }

    /// Updates the cache with a new DNS lookup result
    pub fn update(
        &self,
        name: impl Into<String>,
        ips: &[IpAddr],
        ttl: u32,
    ) -> Result<UpdateStatus> {
        let name = name.into();

        if name.is_empty() || ips.is_empty() {
            return Err(Error::CacheError(
                "name and ips must not be empty".to_string(),
            ));
        }

        // Enforce minimum TTL
        let effective_ttl = if ttl < self.min_ttl {
            self.min_ttl
        } else {
            ttl
        };

        // Check per-host limit
        if self.per_host_limit > 0 && ips.len() > self.per_host_limit {
            return Err(Error::CacheError(format!(
                "IP count {} exceeds per-host limit {}",
                ips.len(),
                self.per_host_limit
            )));
        }

        let new_entry = CacheEntry::new(name.clone(), ips.to_vec(), effective_ttl);
        let mut upserted = false;
        let mut updated = false;

        // Update forward lookup
        let old_entry = self.forward.insert(name.clone(), new_entry.clone());

        if let Some(old) = old_entry {
            // Check if actually updated (not just refreshing same entry)
            updated = old.ips != new_entry.ips || old.expiration_time != new_entry.expiration_time;
            if updated {
                // Remove old reverse entries only if IPs changed
                for ip in &old.ips {
                    if let Some(mut names) = self.reverse.get_mut(ip) {
                        names.retain(|n| n != &name);
                        if names.is_empty() {
                            drop(names);
                            self.reverse.remove(ip);
                        }
                    }
                }
            }
        } else {
            updated = true;
            upserted = true;
        }

        // Update reverse lookups
        for ip in &new_entry.ips {
            self.reverse
                .entry(*ip)
                .or_insert_with(Vec::new)
                .push(name.clone());
        }

        Ok(UpdateStatus { updated, upserted })
    }

    /// Looks up IPs for a given DNS name (returns only non-expired entries)
    pub fn lookup(&self, name: &str) -> Option<Vec<IpAddr>> {
        self.forward.get(name).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(entry.ips.clone())
            }
        })
    }

    /// Looks up DNS names for a given IP (returns all names, may be expired)
    pub fn reverse_lookup(&self, ip: IpAddr) -> Option<Vec<String>> {
        self.reverse.get(&ip).map(|names| names.clone())
    }

    /// Returns the entry for a given name without expiration check
    pub fn get_entry(&self, name: &str) -> Option<CacheEntry> {
        self.forward.get(name).map(|entry| entry.clone())
    }

    /// Gets all non-expired entries as a snapshot
    pub fn snapshot(&self) -> HashMap<String, Vec<IpAddr>> {
        let mut result = HashMap::new();
        for entry in self.forward.iter() {
            if !entry.is_expired() {
                result.insert(entry.name.clone(), entry.ips.clone());
            }
        }
        result
    }

    /// Removes expired entries from the cache
    pub fn cleanup_expired(&self) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut removed_count = 0;

        // Collect entries to remove (avoid holding lock during iteration)
        let to_remove: Vec<String> = self
            .forward
            .iter()
            .filter(|entry| entry.is_expired_at(now))
            .map(|entry| entry.name.clone())
            .collect();

        for name in to_remove {
            if let Some((_, entry)) = self.forward.remove(&name) {
                removed_count += 1;
                // Remove reverse entries
                for ip in &entry.ips {
                    if let Some(mut names) = self.reverse.get_mut(ip) {
                        names.retain(|n| n != &name);
                        if names.is_empty() {
                            drop(names);
                            self.reverse.remove(ip);
                        }
                    }
                }
            }
        }

        removed_count
    }

    /// Clears all entries from the cache
    pub fn clear(&self) {
        self.forward.clear();
        self.reverse.clear();
    }

    /// Returns the number of cached entries
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    /// Checks if cache is empty
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn cache_entry_creation() {
        let entry = CacheEntry::new(
            "example.com",
            vec!["192.0.2.1".parse().unwrap()],
            300,
        );
        assert_eq!(entry.name, "example.com");
        assert_eq!(entry.ttl, 300);
        assert!(!entry.is_expired());
    }

    #[test]
    fn dns_cache_update_and_lookup() {
        let cache = DnsCache::new(0);
        let ips = vec!["192.0.2.1".parse().unwrap()];

        let status = cache.update("example.com", &ips, 300).unwrap();
        assert!(status.upserted);

        let result = cache.lookup("example.com").unwrap();
        assert_eq!(result, ips);
    }

    #[test]
    fn dns_cache_reverse_lookup() {
        let cache = DnsCache::new(0);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();

        cache
            .update("example.com", &vec![ip], 300)
            .unwrap();

        let names = cache.reverse_lookup(ip).unwrap();
        assert_eq!(names, vec!["example.com"]);
    }

    #[test]
    fn dns_cache_min_ttl() {
        let cache = DnsCache::new(600); // min TTL = 600
        let ips = vec!["192.0.2.1".parse().unwrap()];

        cache.update("example.com", &ips, 300).unwrap(); // TTL < min

        let entry = cache.get_entry("example.com").unwrap();
        assert_eq!(entry.ttl, 600); // Should be enforced
    }

    #[test]
    fn dns_cache_per_host_limit() {
        let cache = DnsCache::with_limits(0, 1);
        let ips = vec![
            "192.0.2.1".parse().unwrap(),
            "192.0.2.2".parse().unwrap(),
        ];

        let result = cache.update("example.com", &ips, 300);
        assert!(result.is_err());
    }

    #[test]
    fn dns_cache_snapshot() {
        let cache = DnsCache::new(0);

        cache
            .update("example.com", &vec!["192.0.2.1".parse().unwrap()], 300)
            .unwrap();
        cache
            .update("example.org", &vec!["192.0.2.2".parse().unwrap()], 300)
            .unwrap();

        let snapshot = cache.snapshot();
        assert_eq!(snapshot.len(), 2);
    }

    #[test]
    fn dns_cache_clear() {
        let cache = DnsCache::new(0);

        cache
            .update("example.com", &vec!["192.0.2.1".parse().unwrap()], 300)
            .unwrap();
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());
    }
}
