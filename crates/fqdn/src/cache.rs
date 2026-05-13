//! DNS cache with TTL-based expiration
//!
//! Manages DNS lookup results with automatic expiration. Tracks both forward
//! (name → IPs) and reverse (IP → names) lookups for efficient queries.
//!
//! Semantics are ported from `cilium/pkg/fqdn/cache.go`:
//! - Multiple IPs per name, each with its own expiration (latest-expiration wins).
//! - Strict greater-than expiry: an entry is live while `now <= expiration_time`.
//! - Reverse map mirrors the forward map (ip → name → entry).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_unix_secs() -> i64 {
    unix_secs(SystemTime::now())
}

// ---------------------------------------------------------------------------
// CacheEntry — one entry per (name, IP) pair
// ---------------------------------------------------------------------------

/// DNS cache entry with TTL information.
///
/// In the multi-entry model each `(name, ip)` pair has its own entry
/// (the entry whose `expiration_time` is furthest in the future wins).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// DNS name (may be unqualified)
    #[serde(rename = "fqdn", default, skip_serializing_if = "String::is_empty")]
    pub name: String,

    /// IP addresses returned by DNS lookup
    #[serde(rename = "ips", default, skip_serializing_if = "Vec::is_empty")]
    pub ips: Vec<IpAddr>,

    /// TTL in seconds
    #[serde(rename = "ttl", default)]
    pub ttl: u32,

    /// Time when this entry was created (Unix timestamp in seconds)
    #[serde(rename = "lookup-time", default)]
    pub lookup_time: i64,

    /// Time when this entry expires (Unix timestamp in seconds)
    #[serde(rename = "expiration-time", default)]
    pub expiration_time: i64,
}

impl CacheEntry {
    /// Creates a new cache entry using the given `lookup_time` (Unix secs).
    pub fn with_lookup_time(
        name: impl Into<String>,
        ips: Vec<IpAddr>,
        ttl: u32,
        lookup_time: i64,
    ) -> Self {
        Self {
            name: name.into(),
            ips,
            ttl,
            lookup_time,
            expiration_time: lookup_time + i64::from(ttl),
        }
    }

    /// Creates a new cache entry with `lookup_time = now`.
    pub fn new(name: impl Into<String>, ips: Vec<IpAddr>, ttl: u32) -> Self {
        let lookup_time = now_unix_secs();
        Self::with_lookup_time(name, ips, ttl, lookup_time)
    }

    /// `true` when `now > expiration_time` (strict, matching Go semantics).
    pub fn is_expired_at(&self, now: i64) -> bool {
        now > self.expiration_time
    }

    /// `true` when the entry has expired right now.
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(now_unix_secs())
    }

    /// Remaining TTL in seconds, 0 when expired.
    pub fn remaining_ttl(&self) -> u32 {
        let now = now_unix_secs();
        if self.is_expired_at(now) {
            0
        } else {
            (self.expiration_time - now) as u32
        }
    }
}

// ---------------------------------------------------------------------------
// Update status
// ---------------------------------------------------------------------------

/// Update status after cache modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateStatus {
    /// Whether any cache entry was updated.
    pub updated: bool,
    /// Whether any new IP-to-name mapping was added.
    pub upserted: bool,
}

// ---------------------------------------------------------------------------
// Internal map types
// ---------------------------------------------------------------------------

/// ip → entry (for a given name). The latest-expiring entry per IP wins.
type IpEntries = HashMap<IpAddr, CacheEntry>;

/// name → ip-entries
type ForwardMap = HashMap<String, IpEntries>;

/// name → entry (for a given IP)
type NameEntries = HashMap<String, CacheEntry>;

/// ip → name-entries
type ReverseMap = HashMap<IpAddr, NameEntries>;

// ---------------------------------------------------------------------------
// DnsCache (inner, lock-free)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Inner {
    forward: ForwardMap,
    reverse: ReverseMap,
    /// expiration unix-second → list of names registered at that second.
    cleanup: BTreeMap<i64, Vec<String>>,
    /// last time cleanup was advanced
    last_cleanup: Option<i64>,
    /// names that exceed `per_host_limit`
    over_limit: HashSet<String>,
    per_host_limit: usize,
    min_ttl: u32,
}

impl Inner {
    fn new(min_ttl: u32, per_host_limit: usize) -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
            cleanup: BTreeMap::new(),
            last_cleanup: None,
            over_limit: HashSet::new(),
            per_host_limit,
            min_ttl,
        }
    }

    // ------------------------------------------------------------------
    // Core insert
    // ------------------------------------------------------------------

    /// Insert one (name, ip) → entry. Replaces existing entry only if the
    /// new one expires later (or the existing one is absent/expired).
    ///
    /// Returns `(updated, upserted)`.
    fn update_ip(&mut self, ip: IpAddr, entry: &CacheEntry) -> (bool, bool) {
        let ip_entries = self
            .forward
            .entry(entry.name.clone())
            .or_insert_with(HashMap::new);

        let old = ip_entries.get(&ip);
        let exists = old.is_some();
        // Replace when there is no entry, or the existing entry expires sooner.
        let should_replace = match old {
            None => true,
            Some(o) => o.is_expired_at(entry.expiration_time),
        };

        if should_replace {
            ip_entries.insert(ip, entry.clone());
            // Upsert in reverse map
            let name_entries = self.reverse.entry(ip).or_insert_with(HashMap::new);
            name_entries.insert(entry.name.clone(), entry.clone());
            // Register in cleanup index
            self.add_to_cleanup(entry);
            (true, !exists)
        } else {
            (false, false)
        }
    }

    fn add_to_cleanup(&mut self, entry: &CacheEntry) {
        // Track the earliest expiration as the start of the cleanup window.
        let exp = entry.expiration_time;
        if self.last_cleanup.is_none() || Some(exp) < self.last_cleanup {
            self.last_cleanup = Some(exp);
        }
        self.cleanup
            .entry(exp)
            .or_insert_with(Vec::new)
            .push(entry.name.clone());
    }

    /// Full update for a slice of IPs sharing the same TTL/lookup_time.
    fn update_with_entry(&mut self, entry: &CacheEntry) -> UpdateStatus {
        let mut updated = false;
        let mut upserted = false;
        for ip in &entry.ips {
            let (u, n) = self.update_ip(*ip, entry);
            if u {
                updated = true;
            }
            if n {
                upserted = true;
            }
        }
        // Check over-limit.
        if self.per_host_limit > 0 {
            if let Some(ip_map) = self.forward.get(&entry.name) {
                if ip_map.len() > self.per_host_limit {
                    self.over_limit.insert(entry.name.clone());
                }
            }
        }
        UpdateStatus { updated, upserted }
    }

    // ------------------------------------------------------------------
    // Lookup
    // ------------------------------------------------------------------

    fn lookup_at(&self, name: &str, now: i64) -> Option<Vec<IpAddr>> {
        let entries = self.forward.get(name)?;
        let ips: Vec<IpAddr> = entries
            .iter()
            .filter(|(_, e)| !e.is_expired_at(now))
            .map(|(ip, _)| *ip)
            .collect();
        if ips.is_empty() { None } else { Some(ips) }
    }

    fn lookup_ip_at(&self, ip: IpAddr, now: i64) -> Vec<String> {
        match self.reverse.get(&ip) {
            None => vec![],
            Some(name_map) => {
                let mut names: Vec<String> = name_map
                    .iter()
                    .filter(|(_, e)| !e.is_expired_at(now))
                    .map(|(n, _)| n.clone())
                    .collect();
                names.sort();
                names
            }
        }
    }

    // ------------------------------------------------------------------
    // Remove helpers
    // ------------------------------------------------------------------

    /// Remove a single (name, ip) from forward and reverse.
    fn remove_pair(&mut self, name: &str, ip: IpAddr) {
        if let Some(ip_map) = self.forward.get_mut(name) {
            ip_map.remove(&ip);
            if ip_map.is_empty() {
                self.forward.remove(name);
            }
        }
        if let Some(name_map) = self.reverse.get_mut(&ip) {
            name_map.remove(name);
            if name_map.is_empty() {
                self.reverse.remove(&ip);
            }
        }
    }

    /// Remove all entries for `name`.
    fn remove_name(&mut self, name: &str) {
        if let Some(ip_map) = self.forward.remove(name) {
            for ip in ip_map.keys() {
                if let Some(nm) = self.reverse.get_mut(ip) {
                    nm.remove(name);
                    if nm.is_empty() {
                        self.reverse.remove(ip);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Force-expire by predicate
    // ------------------------------------------------------------------

    fn force_expire<F>(&mut self, predicate: F) -> Vec<String>
    where
        F: Fn(&str) -> bool,
    {
        let to_remove: Vec<String> = self
            .forward
            .keys()
            .filter(|n| predicate(n))
            .cloned()
            .collect();

        let mut removed = Vec::new();
        for name in to_remove {
            self.remove_name(&name);
            removed.push(name);
        }
        removed
    }

    // ------------------------------------------------------------------
    // force_expire_by_names
    // ------------------------------------------------------------------

    /// For each name, remove entries where `entry.lookup_time < expire_before`.
    /// Returns the names that were affected.
    fn force_expire_by_names(&mut self, expire_before: i64, names: &[&str]) -> Vec<String> {
        let mut affected = Vec::new();
        for &name in names {
            let entries = match self.forward.get_mut(name) {
                Some(e) => e,
                None => continue,
            };
            let to_drop: Vec<IpAddr> = entries
                .iter()
                .filter(|(_, e)| e.lookup_time < expire_before)
                .map(|(ip, _)| *ip)
                .collect();
            if !to_drop.is_empty() {
                for ip in to_drop {
                    entries.remove(&ip);
                    if let Some(nm) = self.reverse.get_mut(&ip) {
                        nm.remove(name);
                        if nm.is_empty() {
                            self.reverse.remove(&ip);
                        }
                    }
                }
                if entries.is_empty() {
                    self.forward.remove(name);
                }
                affected.push(name.to_string());
            }
        }
        affected
    }

    // ------------------------------------------------------------------
    // cleanup_expired_entries
    // ------------------------------------------------------------------

    /// Remove all entries registered in the cleanup index with expiration
    /// strictly before `expires`. Returns affected names.
    fn cleanup_expired_entries(&mut self, expires: i64) -> Vec<String> {
        let (names, _) = self.cleanup_expired_entries_with_removed(expires);
        names
    }

    /// Like `cleanup_expired_entries` but also returns removed (ip, name, expiration_time) tuples.
    fn cleanup_expired_entries_with_removed(
        &mut self,
        expires: i64,
    ) -> (Vec<String>, Vec<(IpAddr, String, i64)>) {
        let lc = match self.last_cleanup {
            None => return (vec![], vec![]),
            Some(v) => v,
        };

        // Collect names from the cleanup buckets up to (but not including) expires.
        let mut to_clean: HashSet<String> = HashSet::new();
        let mut keys_to_remove: Vec<i64> = Vec::new();
        let mut cursor = lc;
        while cursor < expires {
            if let Some(names) = self.cleanup.get(&cursor) {
                for n in names {
                    to_clean.insert(n.clone());
                }
                keys_to_remove.push(cursor);
            }
            cursor += 1;
        }
        for k in &keys_to_remove {
            self.cleanup.remove(k);
        }
        // Advance last_cleanup
        self.last_cleanup = Some(expires);

        // For each candidate name, remove expired entries.
        let mut affected = Vec::new();
        let mut removed_entries: Vec<(IpAddr, String, i64)> = Vec::new();
        for name in to_clean {
            let removed = if let Some(ip_map) = self.forward.get_mut(&name) {
                let to_drop: Vec<(IpAddr, i64)> = ip_map
                    .iter()
                    .filter(|(_, e)| e.is_expired_at(expires)) // entry expired at `expires`
                    .map(|(ip, e)| (*ip, e.expiration_time))
                    .collect();
                for (ip, exp_time) in &to_drop {
                    ip_map.remove(ip);
                    if let Some(nm) = self.reverse.get_mut(ip) {
                        nm.remove(&name);
                        if nm.is_empty() {
                            self.reverse.remove(ip);
                        }
                    }
                    removed_entries.push((*ip, name.clone(), *exp_time));
                }
                !to_drop.is_empty()
            } else {
                false
            };
            if let Some(ip_map) = self.forward.get(&name) {
                if ip_map.is_empty() {
                    self.forward.remove(&name);
                }
            }
            if removed {
                affected.push(name);
            }
        }
        (affected, removed_entries)
    }

    // ------------------------------------------------------------------
    // cleanup_over_limit_entries
    // ------------------------------------------------------------------

    fn cleanup_over_limit_entries(&mut self) -> Vec<String> {
        let (names, _) = self.cleanup_over_limit_entries_with_removed();
        names
    }

    /// Like `cleanup_over_limit_entries` but also returns removed (ip, name, expiration_time).
    fn cleanup_over_limit_entries_with_removed(
        &mut self,
    ) -> (Vec<String>, Vec<(IpAddr, String, i64)>) {
        if self.per_host_limit == 0 {
            self.over_limit.clear();
            return (vec![], vec![]);
        }

        let mut affected = Vec::new();
        let mut removed_entries: Vec<(IpAddr, String, i64)> = Vec::new();
        let names: Vec<String> = self.over_limit.drain().collect();

        for name in names {
            let ip_map = match self.forward.get_mut(&name) {
                Some(m) => m,
                None => continue,
            };
            let overlimit = ip_map.len() as isize - self.per_host_limit as isize;
            if overlimit <= 0 {
                continue;
            }
            // Sort by expiration time ascending (oldest first → remove first).
            let mut entries: Vec<(IpAddr, i64)> = ip_map
                .iter()
                .map(|(ip, e)| (*ip, e.expiration_time))
                .collect();
            entries.sort_by_key(|(_, exp)| *exp);

            for (ip, exp_time) in entries.into_iter().take(overlimit as usize) {
                ip_map.remove(&ip);
                if let Some(nm) = self.reverse.get_mut(&ip) {
                    nm.remove(&name);
                    if nm.is_empty() {
                        self.reverse.remove(&ip);
                    }
                }
                removed_entries.push((ip, name.clone(), exp_time));
            }
            if ip_map.is_empty() {
                self.forward.remove(&name);
            }
            affected.push(name);
        }
        (affected, removed_entries)
    }

    // ------------------------------------------------------------------
    // Count
    // ------------------------------------------------------------------

    fn count(&self) -> (u64, u64) {
        let fqdns = self.forward.len() as u64;
        let ips: u64 = self.forward.values().map(|m| m.len() as u64).sum();
        (fqdns, ips)
    }

    // ------------------------------------------------------------------
    // Dump — returns unique entries.
    //
    // In Go, cacheEntry objects are shared pointers: multiple IPs in one
    // Update() call share a single pointer. Dump() deduplicates by pointer,
    // so one Go cacheEntry (potentially listing multiple IPs) appears once.
    //
    // In Rust, each CacheEntry stored in `forward[name][ip]` carries the
    // `.ips` vector from the update call that placed it there.  We
    // deduplicate by (name, ips, expiration_time) so that two IPs inserted
    // in the same update() call collapse into one entry (matching Go), while
    // two separate update() calls for the same name with different IPs
    // produce two entries.
    // ------------------------------------------------------------------

    fn dump(&self) -> Vec<CacheEntry> {
        // Key: (name, sorted ips as strings, expiration_time).
        // Entries that came from the same update() call have identical .ips.
        let mut seen: HashSet<(String, Vec<String>, i64)> = HashSet::new();
        let mut result = Vec::new();
        for ip_map in self.forward.values() {
            for entry in ip_map.values() {
                let mut ip_strs: Vec<String> = entry.ips.iter().map(|ip| ip.to_string()).collect();
                ip_strs.sort();
                let key = (entry.name.clone(), ip_strs, entry.expiration_time);
                if seen.insert(key) {
                    result.push(entry.clone());
                }
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// DnsCache — public API wrapping a Mutex<Inner>
// ---------------------------------------------------------------------------

/// DNS cache managing forward (name → IP) and reverse (IP → name) lookups.
///
/// Thread-safe via an internal `Mutex`. All methods take `&self` and lock
/// internally (consistent with the original DashMap-based API).
#[derive(Debug, Clone)]
pub struct DnsCache {
    inner: Arc<Mutex<Inner>>,
}

impl DnsCache {
    /// Creates a new DNS cache with minimum TTL.
    pub fn new(min_ttl: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::new(min_ttl, 0))),
        }
    }

    /// Creates a new DNS cache with minimum TTL and per-host limit.
    pub fn with_limits(min_ttl: u32, per_host_limit: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::new(min_ttl, per_host_limit))),
        }
    }

    /// Creates a new DNS cache with per-host IP limit (Go: `NewDNSCacheWithLimit`).
    pub fn new_with_limit(min_ttl: u32, limit: usize) -> Self {
        Self::with_limits(min_ttl, limit)
    }

    // ------------------------------------------------------------------
    // Update
    // ------------------------------------------------------------------

    /// Updates the cache with a new DNS lookup result.
    ///
    /// Uses `SystemTime::now()` as the lookup time. Each IP gets its own
    /// entry; entries are replaced only when the new expiration is later.
    pub fn update(
        &self,
        name: impl Into<String>,
        ips: &[IpAddr],
        ttl: u32,
    ) -> Result<UpdateStatus> {
        self.update_at(name, ips, ttl, now_unix_secs())
    }

    /// Like `update` but with an explicit lookup time (Unix seconds).
    pub fn update_at(
        &self,
        name: impl Into<String>,
        ips: &[IpAddr],
        ttl: u32,
        lookup_time: i64,
    ) -> Result<UpdateStatus> {
        let name = name.into();
        if name.is_empty() || ips.is_empty() {
            return Err(Error::CacheError(
                "name and ips must not be empty".to_string(),
            ));
        }
        let mut g = self.inner.lock().unwrap();
        let effective_ttl = ttl.max(g.min_ttl);
        let entry = CacheEntry::with_lookup_time(name, ips.to_vec(), effective_ttl, lookup_time);
        Ok(g.update_with_entry(&entry))
    }

    // ------------------------------------------------------------------
    // Lookup
    // ------------------------------------------------------------------

    /// Looks up IPs for a given DNS name (returns only non-expired entries).
    pub fn lookup(&self, name: &str) -> Option<Vec<IpAddr>> {
        self.lookup_at(name, now_unix_secs())
    }

    /// Looks up IPs for a given name as-of the given Unix timestamp.
    pub fn lookup_at(&self, name: &str, at: i64) -> Option<Vec<IpAddr>> {
        self.inner.lock().unwrap().lookup_at(name, at)
    }

    /// Looks up DNS names for a given IP as-of the given Unix timestamp.
    pub fn lookup_ip_at(&self, ip: IpAddr, now_secs: i64) -> Vec<String> {
        self.inner.lock().unwrap().lookup_ip_at(ip, now_secs)
    }

    /// Looks up DNS names for a given IP (current time).
    pub fn reverse_lookup(&self, ip: IpAddr) -> Option<Vec<String>> {
        let names = self.lookup_ip_at(ip, now_unix_secs());
        if names.is_empty() { None } else { Some(names) }
    }

    // ------------------------------------------------------------------
    // Force-expire
    // ------------------------------------------------------------------

    /// Removes all names matching `name_predicate`.
    pub fn force_expire<F>(&self, name_predicate: F) -> Vec<String>
    where
        F: Fn(&str) -> bool,
    {
        self.inner.lock().unwrap().force_expire(name_predicate)
    }

    /// For each name in `names`, remove entries whose `lookup_time < expire_before`.
    pub fn force_expire_by_names(&self, expire_before: SystemTime, names: &[&str]) -> Vec<String> {
        let ts = unix_secs(expire_before);
        self.inner.lock().unwrap().force_expire_by_names(ts, names)
    }

    // ------------------------------------------------------------------
    // TTL cleanup
    // ------------------------------------------------------------------

    /// Removes entries that have expired by `now` according to the cleanup index.
    pub fn cleanup_expired_entries(&self, now: SystemTime) -> Vec<String> {
        let ts = unix_secs(now);
        self.inner.lock().unwrap().cleanup_expired_entries(ts)
    }

    // ------------------------------------------------------------------
    // Over-limit cleanup
    // ------------------------------------------------------------------

    /// Trims names that exceed the per-host limit; removes oldest-expiring IPs.
    pub fn cleanup_over_limit_entries(&self) -> Vec<String> {
        self.inner.lock().unwrap().cleanup_over_limit_entries()
    }

    // ------------------------------------------------------------------
    // Count
    // ------------------------------------------------------------------

    /// Returns `(num_fqdns, num_ips)`.
    pub fn count(&self) -> (u64, u64) {
        self.inner.lock().unwrap().count()
    }

    // ------------------------------------------------------------------
    // Misc
    // ------------------------------------------------------------------

    /// Returns all (name, IPs) pairs currently in the forward map.
    pub fn dump(&self) -> Vec<(String, Vec<IpAddr>)> {
        self.inner
            .lock()
            .unwrap()
            .dump()
            .into_iter()
            .map(|e| (e.name, e.ips))
            .collect()
    }

    /// Returns the entry for a given name without expiration check.
    /// Because names now map to multiple IPs, this returns the first entry found.
    pub fn get_entry(&self, name: &str) -> Option<CacheEntry> {
        let g = self.inner.lock().unwrap();
        let ip_map = g.forward.get(name)?;
        // Return a synthetic entry aggregating all IPs (backwards-compat with tests).
        let mut ips = Vec::new();
        let mut ttl = 0u32;
        let mut lookup_time = i64::MAX;
        let mut expiration_time = 0i64;
        for (ip, e) in ip_map {
            ips.push(*ip);
            ttl = e.ttl;
            if e.lookup_time < lookup_time {
                lookup_time = e.lookup_time;
            }
            if e.expiration_time > expiration_time {
                expiration_time = e.expiration_time;
            }
        }
        if ips.is_empty() {
            None
        } else {
            Some(CacheEntry {
                name: name.to_string(),
                ips,
                ttl,
                lookup_time,
                expiration_time,
            })
        }
    }

    /// Gets all non-expired entries as a name → IPs snapshot.
    pub fn snapshot(&self) -> HashMap<String, Vec<IpAddr>> {
        let now = now_unix_secs();
        let g = self.inner.lock().unwrap();
        let mut result = HashMap::new();
        for (name, ip_map) in &g.forward {
            let ips: Vec<IpAddr> = ip_map
                .iter()
                .filter(|(_, e)| !e.is_expired_at(now))
                .map(|(ip, _)| *ip)
                .collect();
            if !ips.is_empty() {
                result.insert(name.clone(), ips);
            }
        }
        result
    }

    /// Removes expired entries from the cache.
    pub fn cleanup_expired(&self) -> usize {
        let now = now_unix_secs();
        let mut g = self.inner.lock().unwrap();
        let names: Vec<String> = g.forward.keys().cloned().collect();
        let mut count = 0;
        for name in names {
            let to_drop: Vec<IpAddr> = g
                .forward
                .get(&name)
                .map(|m| {
                    m.iter()
                        .filter(|(_, e)| e.is_expired_at(now))
                        .map(|(ip, _)| *ip)
                        .collect()
                })
                .unwrap_or_default();
            for ip in to_drop {
                g.remove_pair(&name, ip);
                count += 1;
            }
        }
        count
    }

    /// Clears all entries.
    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap();
        g.forward.clear();
        g.reverse.clear();
        g.cleanup.clear();
        g.over_limit.clear();
        g.last_cleanup = None;
    }

    /// Returns the number of distinct FQDNs.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().forward.len()
    }

    /// Returns `true` when the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().forward.is_empty()
    }

    // ------------------------------------------------------------------
    // JSON marshalling (Go parity)
    // ------------------------------------------------------------------

    /// Serialises the cache to JSON (array of cache entries, Go format).
    pub fn marshal_json(&self) -> String {
        let entries = self.inner.lock().unwrap().dump();
        serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
    }

    /// Rebuilds the cache from JSON produced by `marshal_json` / Go's MarshalJSON.
    pub fn unmarshal_json(&self, raw: &str) -> std::result::Result<(), serde_json::Error> {
        let entries: Vec<CacheEntry> = serde_json::from_str(raw)?;
        let mut g = self.inner.lock().unwrap();
        g.forward.clear();
        g.reverse.clear();
        g.cleanup.clear();
        g.over_limit.clear();
        g.last_cleanup = None;
        for entry in entries {
            g.update_with_entry(&entry);
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Test-internal accessors (pub(crate) / test-only)
    // ------------------------------------------------------------------

    #[cfg(test)]
    pub fn forward_is_empty(&self) -> bool {
        self.inner.lock().unwrap().forward.is_empty()
    }

    #[cfg(test)]
    pub fn reverse_is_empty(&self) -> bool {
        self.inner.lock().unwrap().reverse.is_empty()
    }

    #[cfg(test)]
    pub fn over_limit_len(&self) -> usize {
        self.inner.lock().unwrap().over_limit.len()
    }

    #[cfg(test)]
    pub fn cleanup_len(&self) -> usize {
        self.inner.lock().unwrap().cleanup.len()
    }

    #[cfg(test)]
    pub fn set_last_cleanup(&self, secs: i64) {
        self.inner.lock().unwrap().last_cleanup = Some(secs);
    }

    #[cfg(test)]
    pub fn insert_cleanup_entry(&self, secs: i64, name: &str) {
        self.inner
            .lock()
            .unwrap()
            .cleanup
            .entry(secs)
            .or_insert_with(Vec::new)
            .push(name.to_string());
    }

    // ------------------------------------------------------------------
    // GC — cascades expired / over-limit entries into zombie mappings
    // ------------------------------------------------------------------

    /// Run TTL-cleanup and over-limit cleanup, feeding removed entries into
    /// `zombies` (if Some). Returns the set of affected names.
    ///
    /// Ported from Go's `(*DNSCache).GC`.
    pub fn gc(&self, now: SystemTime, zombies: Option<&mut DnsZombieMappings>) -> HashSet<String> {
        let now_secs = unix_secs(now);
        let mut g = self.inner.lock().unwrap();

        let (expired_names, expired_removed) = g.cleanup_expired_entries_with_removed(now_secs);
        let (overlimit_names, overlimit_removed) = g.cleanup_over_limit_entries_with_removed();

        // Merge affected names.
        let mut affected: HashSet<String> = HashSet::new();
        for n in expired_names {
            affected.insert(n);
        }
        for n in overlimit_names {
            affected.insert(n);
        }

        if let Some(z) = zombies {
            // Feed expired entries.
            for (ip, name, exp_time) in &expired_removed {
                // Use `now` as the expiry (same as Go: expireTime = now unless entry is in future).
                let expire_time = if *exp_time > now_secs {
                    *exp_time
                } else {
                    now_secs
                };
                let expire_st = UNIX_EPOCH + Duration::from_secs(expire_time.max(0) as u64);
                z.upsert(expire_st, *ip, &[name.as_str()]);
            }
            // Feed over-limit entries.
            for (ip, name, exp_time) in &overlimit_removed {
                let expire_time = if *exp_time > now_secs {
                    *exp_time
                } else {
                    now_secs
                };
                let expire_st = UNIX_EPOCH + Duration::from_secs(expire_time.max(0) as u64);
                z.upsert(expire_st, *ip, &[name.as_str()]);
            }
        }

        affected
    }

    // ------------------------------------------------------------------
    // Test-only direct-map accessors (for cascade tests)
    // ------------------------------------------------------------------

    #[cfg(test)]
    pub fn forward_contains(&self, name: &str) -> bool {
        self.inner.lock().unwrap().forward.contains_key(name)
    }

    #[cfg(test)]
    pub fn reverse_contains(&self, ip: IpAddr) -> bool {
        self.inner.lock().unwrap().reverse.contains_key(&ip)
    }
}

// ---------------------------------------------------------------------------
// DnsZombieMapping / DnsZombieMappings — ported from Go's DNSZombieMapping /
// DNSZombieMappings in cilium/pkg/fqdn/cache.go lines ~860–1338.
// ---------------------------------------------------------------------------

/// One zombie entry: an IP that has expired from the DNS cache but may still
/// be in active use by live connections.
#[derive(Debug, Clone)]
pub struct DnsZombieMapping {
    /// DNS names that resolved to this IP.
    pub names: Vec<String>,
    /// The IP address.
    pub ip: IpAddr,
    /// When this zombie was last marked alive by CT GC.
    pub alive_at: SystemTime,
    /// When this IP was most-recently scheduled for deletion (= DNS expiry time).
    pub delete_pending_at: SystemTime,
    /// GC revision at which this entry was added.
    revision_added_at: u64,
}

/// Collects DNS Name→IP mappings that may be inactive and ready to evict.
///
/// Ported from Go's `DNSZombieMappings`.
pub struct DnsZombieMappings {
    /// Maximum number of alive zombies (0 = unlimited in the sense of the Go
    /// default of 10 000; we treat 0 the same as Go treats very large numbers).
    max: usize,
    /// Maximum IPs per hostname.
    per_host_limit: usize,
    /// ip → zombie mapping.
    mappings: HashMap<IpAddr, DnsZombieMapping>,
    /// Time of the most-recent CT GC start.
    last_ctgc_update: SystemTime,
    /// Running count of CT GC cycles.
    gc_revision: u64,
}

impl DnsZombieMappings {
    /// Creates a new `DnsZombieMappings`.
    ///
    /// `max` is the maximum number of alive zombies (0 = the Go default large limit).
    /// `per_host_limit` is the maximum IPs per hostname (0 = no limit).
    pub fn new(max: usize, per_host_limit: usize) -> Self {
        Self {
            max,
            per_host_limit,
            mappings: HashMap::new(),
            last_ctgc_update: UNIX_EPOCH,
            gc_revision: 0,
        }
    }

    // ------------------------------------------------------------------
    // Upsert
    // ------------------------------------------------------------------

    /// Enqueue `ip` with the given `names` as a possible deletion at `expiry_time`.
    ///
    /// Returns `true` if an existing entry was updated, `false` if a new one was created.
    ///
    /// Ported from Go's `(*DNSZombieMappings).Upsert`.
    pub fn upsert(&mut self, expiry_time: SystemTime, ip: IpAddr, names: &[&str]) -> bool {
        if let Some(zombie) = self.mappings.get_mut(&ip) {
            // Merge names (deduplicated).
            for &n in names {
                if !zombie.names.iter().any(|x| x == n) {
                    zombie.names.push(n.to_string());
                }
            }
            // Keep the later expiry time.
            if expiry_time > zombie.delete_pending_at {
                zombie.delete_pending_at = expiry_time;
            }
            // Bump alive_at to last_ctgc_update if it's later.
            if self.last_ctgc_update > zombie.alive_at {
                zombie.alive_at = self.last_ctgc_update;
            }
            true
        } else {
            let zombie = DnsZombieMapping {
                names: {
                    let mut v: Vec<String> = names.iter().map(|s| s.to_string()).collect();
                    v.dedup();
                    v
                },
                ip,
                alive_at: self.last_ctgc_update,
                delete_pending_at: expiry_time,
                revision_added_at: self.gc_revision,
            };
            self.mappings.insert(ip, zombie);
            false
        }
    }

    // ------------------------------------------------------------------
    // MarkAlive / SetCTGCTime
    // ------------------------------------------------------------------

    /// Mark an IP as alive at `now`.
    ///
    /// Ported from Go's `MarkAlive`.
    pub fn mark_alive(&mut self, now: SystemTime, ip: IpAddr) {
        if let Some(zombie) = self.mappings.get_mut(&ip) {
            zombie.alive_at = now;
        }
    }

    /// Record the start of a CT GC cycle. Increments the internal GC revision.
    ///
    /// Ported from Go's `SetCTGCTime`.
    pub fn set_ctgc_time(&mut self, ct_gc_start: SystemTime, _est_next: SystemTime) {
        self.last_ctgc_update = ct_gc_start;
        self.gc_revision += 1;
    }

    // ------------------------------------------------------------------
    // is_connection_alive — internal liveness check
    // ------------------------------------------------------------------

    /// Returns `true` if `zombie` is considered alive.
    ///
    /// A zombie is *dead* only when ALL of:
    /// 1. CT GC has run after the DNS expiry (`last_ctgc_update > delete_pending_at`).
    /// 2. CT GC did not mark the zombie alive (`last_ctgc_update > alive_at`).
    /// 3. CT GC has run at least 2 times since the zombie was entered
    ///    (`gc_revision >= revision_added_at + 2`).
    ///
    /// Ported from Go's `isConnectionAlive` (grace period = 0).
    fn is_connection_alive(&self, zombie: &DnsZombieMapping) -> bool {
        // Condition 1: CT GC ran after DNS expiry.
        if self.last_ctgc_update <= zombie.delete_pending_at {
            return true;
        }
        // Condition 2: CT GC ran after the zombie was marked alive.
        if self.last_ctgc_update <= zombie.alive_at {
            return true;
        }
        // Condition 3: need at least 2 GC cycles after insertion.
        if self.gc_revision < zombie.revision_added_at + 2 {
            return true;
        }
        false
    }

    // ------------------------------------------------------------------
    // get_alive_names — internal helper
    // ------------------------------------------------------------------

    /// Returns a map of name → list of zombies that are alive (connection-alive).
    /// Dead zombies for the same name are also added if the name has at least
    /// one alive zombie.
    ///
    /// Ported from Go's `getAliveNames`.
    fn get_alive_names(&self) -> HashMap<String, Vec<IpAddr>> {
        let mut alive_names: HashMap<String, Vec<IpAddr>> = HashMap::new();

        // First pass: add alive zombies.
        for zombie in self.mappings.values() {
            if self.is_connection_alive(zombie) {
                for name in &zombie.names {
                    alive_names.entry(name.clone()).or_default().push(zombie.ip);
                }
            }
        }

        // Second pass: add dead zombies for names that have at least one alive zombie.
        for zombie in self.mappings.values() {
            if !self.is_connection_alive(zombie) {
                for name in &zombie.names {
                    if let Some(v) = alive_names.get_mut(name) {
                        v.push(zombie.ip);
                    }
                }
            }
        }

        alive_names
    }

    // ------------------------------------------------------------------
    // is_zombie_alive — public-ish helper
    // ------------------------------------------------------------------

    /// Returns `(alive, over_limit)` for a zombie given the pre-computed
    /// `alive_names` map.
    ///
    /// Ported from Go's `isZombieAlive`.
    fn is_zombie_alive(
        &self,
        zombie: &DnsZombieMapping,
        alive_names: &HashMap<String, Vec<IpAddr>>,
    ) -> (bool, bool) {
        let conn_alive = self.is_connection_alive(zombie);
        if conn_alive && self.per_host_limit == 0 {
            return (true, false);
        }

        let mut alive = conn_alive;
        let mut over_limit = false;

        for name in &zombie.names {
            if let Some(ips) = alive_names.get(name) {
                alive = true;
                if self.per_host_limit == 0 {
                    return (true, false);
                } else if ips.len() > self.per_host_limit {
                    over_limit = true;
                    return (alive, over_limit);
                }
            }
        }

        (alive, over_limit)
    }

    // ------------------------------------------------------------------
    // GC
    // ------------------------------------------------------------------

    /// Collect alive and dead zombies. Dead zombies are removed from the
    /// internal map. Returns `(alive, dead)`.
    ///
    /// Ported from Go's `(*DNSZombieMappings).GC`.
    pub fn gc(&mut self) -> (Vec<DnsZombieMapping>, Vec<DnsZombieMapping>) {
        let alive_names = self.get_alive_names();

        let mut alive: Vec<DnsZombieMapping> = Vec::new();
        let mut dead: Vec<DnsZombieMapping> = Vec::new();

        // First pass: classify zombies that are not over-limit.
        for zombie in self.mappings.values() {
            let (zombie_alive, over_limit) = self.is_zombie_alive(zombie, &alive_names);
            if over_limit {
                // Will be handled in the per-host-limit pass below.
            } else if zombie_alive {
                alive.push(zombie.clone());
            } else {
                dead.push(zombie.clone());
            }
        }

        // Per-host-limit pass.
        if self.per_host_limit > 0 {
            let dead_start = dead.len();
            let mut possible_alive: Vec<IpAddr> = Vec::new();

            for (_name, alive_ips_for_name) in &alive_names {
                if alive_ips_for_name.len() <= self.per_host_limit {
                    // Already handled above.
                    continue;
                }
                let over = alive_ips_for_name.len() - self.per_host_limit;
                // Collect the zombie structs for this name.
                let mut name_zombies: Vec<DnsZombieMapping> = alive_ips_for_name
                    .iter()
                    .filter_map(|ip| self.mappings.get(ip))
                    .cloned()
                    .collect();
                sort_zombie_mapping_slice(&mut name_zombies);
                for z in name_zombies.iter().take(over) {
                    dead.push(z.clone());
                }
                for z in name_zombies.iter().skip(over) {
                    possible_alive.push(z.ip);
                }
            }

            // Remove from possible_alive anything that ended up in dead[dead_start..].
            let dead_ips: HashSet<IpAddr> = dead[dead_start..].iter().map(|z| z.ip).collect();
            for ip in possible_alive {
                if !dead_ips.contains(&ip) {
                    if let Some(z) = self.mappings.get(&ip) {
                        // Avoid duplicating entries already in alive.
                        if !alive.iter().any(|a| a.ip == ip) {
                            alive.push(z.clone());
                        }
                    }
                }
            }
        }

        // Global max limit.
        if self.max > 0 && alive.len() > self.max {
            sort_zombie_mapping_slice(&mut alive);
            let excess = alive.len() - self.max;
            dead.extend(alive.drain(..excess));
        }

        // Remove dead from internal map.
        // Deduplicate by IP to avoid double-deleting.
        let mut dead_ips_to_remove: HashSet<IpAddr> = HashSet::new();
        for z in &dead {
            dead_ips_to_remove.insert(z.ip);
        }
        for ip in dead_ips_to_remove {
            self.mappings.remove(&ip);
        }

        // Deduplicate dead by IP (keep last occurrence).
        let mut seen: HashSet<IpAddr> = HashSet::new();
        dead.retain(|z| seen.insert(z.ip));

        (alive, dead)
    }

    // ------------------------------------------------------------------
    // ForceExpire
    // ------------------------------------------------------------------

    /// Remove zombies matching the criteria. `expire_before == UNIX_EPOCH` means
    /// match all regardless of time. `name_match` filters by substring.
    ///
    /// Returns the list of names that were affected.
    ///
    /// Ported from Go's `(*DNSZombieMappings).ForceExpire`.
    pub fn force_expire(
        &mut self,
        expire_before: SystemTime,
        name_match: Option<&str>,
    ) -> Vec<String> {
        let match_all_time = expire_before == UNIX_EPOCH;

        let mut to_delete: Vec<IpAddr> = Vec::new();
        let mut names_affected: Vec<String> = Vec::new();

        for zombie in self.mappings.values_mut() {
            // Check time constraint.
            if !match_all_time && zombie.delete_pending_at > expire_before {
                continue;
            }

            let mut new_names: Vec<String> = Vec::new();
            for name in &zombie.names {
                let matches = match name_match {
                    None => true,
                    Some(pattern) => name == pattern,
                };
                if matches {
                    names_affected.push(name.clone());
                } else {
                    new_names.push(name.clone());
                }
            }
            zombie.names = new_names;

            if zombie.names.is_empty() {
                to_delete.push(zombie.ip);
            }
        }

        for ip in to_delete {
            self.mappings.remove(&ip);
        }

        names_affected
    }

    /// Like `force_expire` but matches against a specific (name, ip) pair.
    ///
    /// Ported from Go's `(*DNSZombieMappings).ForceExpireByNameIP`.
    pub fn force_expire_by_name_ip(
        &mut self,
        expire_before: SystemTime,
        name: &str,
        ips: &[IpAddr],
    ) {
        let match_all_time = expire_before == UNIX_EPOCH;

        for ip in ips {
            if let Some(zombie) = self.mappings.get_mut(ip) {
                if !match_all_time && zombie.delete_pending_at > expire_before {
                    continue;
                }
                zombie.names.retain(|n| n != name);
                if zombie.names.is_empty() {
                    self.mappings.remove(ip);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // DumpAlive
    // ------------------------------------------------------------------

    /// Returns clones of alive zombies whose IP passes `prefix_matcher`.
    ///
    /// Ported from Go's `(*DNSZombieMappings).DumpAlive`.
    pub fn dump_alive<F>(&self, prefix_matcher: Option<F>) -> Vec<DnsZombieMapping>
    where
        F: Fn(IpAddr) -> bool,
    {
        let alive_names = self.get_alive_names();
        let mut result = Vec::new();

        for zombie in self.mappings.values() {
            let (alive, _) = self.is_zombie_alive(zombie, &alive_names);
            if !alive {
                continue;
            }
            if let Some(ref matcher) = prefix_matcher {
                if !matcher(zombie.ip) {
                    continue;
                }
            }
            result.push(zombie.clone());
        }

        result
    }

    // ------------------------------------------------------------------
    // Test-only accessors
    // ------------------------------------------------------------------

    /// Number of entries in the internal map (for test assertions).
    #[cfg(test)]
    pub fn len_deletes(&self) -> usize {
        self.mappings.len()
    }

    /// Whether the internal map contains a given IP (for test assertions).
    #[cfg(test)]
    pub fn contains_ip(&self, ip: IpAddr) -> bool {
        self.mappings.contains_key(&ip)
    }
}

// ---------------------------------------------------------------------------
// sort_zombie_mapping_slice — free function (also used internally)
// ---------------------------------------------------------------------------

/// Sort zombies so that least-important (first to evict) are at the front.
///
/// Sort key (ascending): AliveAt → DeletePendingAt → names.len().
///
/// Ported from Go's `sortZombieMappingSlice`.
pub fn sort_zombie_mapping_slice(zombies: &mut Vec<DnsZombieMapping>) {
    zombies.sort_by(|a, b| {
        match a.alive_at.cmp(&b.alive_at) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match a.delete_pending_at.cmp(&b.delete_pending_at) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        a.names.len().cmp(&b.names.len())
    });
}

// ---------------------------------------------------------------------------
// Legacy tests (kept passing)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_entry_creation() {
        let entry = CacheEntry::new("example.com", vec!["192.0.2.1".parse().unwrap()], 300);
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

        cache.update("example.com", &vec![ip], 300).unwrap();

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
        // In the new model, inserts beyond the limit just mark over_limit; they
        // don't fail. This test verifies the over_limit tracking instead.
        let cache = DnsCache::with_limits(0, 1);
        let ips = vec![
            "192.0.2.1".parse::<IpAddr>().unwrap(),
            "192.0.2.2".parse::<IpAddr>().unwrap(),
        ];
        // Two separate updates, each with one IP — second update pushes over limit.
        cache.update("example.com", &[ips[0]], 300).unwrap();
        cache.update("example.com", &[ips[1]], 300).unwrap();
        assert_eq!(cache.over_limit_len(), 1);
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

// ---------------------------------------------------------------------------
// Parity tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod parity_tests {
    //! Parity tests ported from `cilium/pkg/fqdn/cache_test.go`.

    use super::*;

    fn now_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    // ------------------------------------------------------------------
    // TestUpdateLookup
    // ------------------------------------------------------------------
    #[test]
    fn test_update_lookup() {
        let name = "test.com";
        let now = now_secs();
        let cache = DnsCache::new(0);
        let end_time_seconds = 4i64;

        // Add 1 new entry "per second", and one with a redundant IP (ttl/2).
        // Each IP reflects the second at which it expires.
        for i in 1..=end_time_seconds {
            let ttl = i as u32;
            cache
                .update_at(
                    name,
                    &[
                        format!("1.1.1.{i}").parse().unwrap(),
                        format!("2.2.2.{i}").parse().unwrap(),
                    ],
                    ttl,
                    now,
                )
                .unwrap();
            cache
                .update_at(name, &[format!("1.1.1.{i}").parse().unwrap()], ttl / 2, now)
                .unwrap();
        }

        // For each secondsPastNow, expect 2*(endTimeSeconds - secondsPastNow + 1) IPs.
        for seconds_past_now in 1..=end_time_seconds {
            let at = now + seconds_past_now;
            let mut ips = cache.lookup_at(name, at).unwrap_or_default();
            let expected_len = 2 * (end_time_seconds - seconds_past_now + 1) as usize;
            assert_eq!(
                ips.len(),
                expected_len,
                "Incorrect number of IPs at t+{seconds_past_now}: got {:?}",
                ips
            );
            // Sort IPs and verify.
            ips.sort();
            let half = expected_len / 2;
            let j_start = seconds_past_now as u8;
            for (k, ip) in ips[..half].iter().enumerate() {
                let expected: IpAddr = format!("1.1.1.{}", j_start + k as u8).parse().unwrap();
                assert_eq!(*ip, expected, "Wrong 1.1.1.x IP at position {k}");
            }
            for (k, ip) in ips[half..].iter().enumerate() {
                let expected: IpAddr = format!("2.2.2.{}", j_start + k as u8).parse().unwrap();
                assert_eq!(*ip, expected, "Wrong 2.2.2.x IP at position {k}");
            }
        }
    }

    /// Ported from `TestPrivilegedDelete` in `cilium/pkg/fqdn/cache_test.go`.
    #[test]
    fn test_privileged_delete() {
        let shared_ip: IpAddr = "1.1.1.1".parse().unwrap();
        let ip1: IpAddr = "2.2.2.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();
        let ip3: IpAddr = "2.2.2.3".parse().unwrap();

        let cache = DnsCache::new(0);

        cache.update("test1.com", &[shared_ip, ip1], 5).unwrap();
        cache.update("test2.com", &[shared_ip, ip2], 5).unwrap();
        cache.update("test3.com", &[shared_ip, ip3], 5).unwrap();

        let now = now_secs();

        // Non-matching predicate: nothing removed.
        let removed = cache.force_expire(|n| n == "notatest.com");
        assert!(removed.is_empty(), "Expected no removals, got: {removed:?}");
        for name in &["test1.com", "test2.com", "test3.com"] {
            let ips = cache.lookup_at(name, now);
            assert_eq!(
                ips.as_ref().map(|v| v.len()),
                Some(2),
                "Expected 2 IPs for {name}, got {ips:?}"
            );
        }

        // Remove test1.com.
        let removed = cache.force_expire(|n| n == "test1.com");
        assert_eq!(removed.len(), 1, "Expected 1 removal, got {removed:?}");
        assert!(removed.contains(&"test1.com".to_string()));
        assert!(cache.lookup_at("test1.com", now).is_none());
        for name in &["test2.com", "test3.com"] {
            assert_eq!(
                cache.lookup_at(name, now).as_ref().map(|v| v.len()),
                Some(2),
                "Expected 2 IPs for {name} after partial expire"
            );
        }

        // Remove everything.
        let removed = cache.force_expire(|_| true);
        assert_eq!(removed.len(), 2, "Expected 2 removals, got {removed:?}");
        for name in &["test1.com", "test2.com", "test3.com"] {
            assert!(cache.lookup_at(name, now).is_none());
        }
        assert!(cache.forward_is_empty(), "forward map should be empty");
        assert!(cache.reverse_is_empty(), "reverse map should be empty");
        assert!(cache.dump().is_empty(), "dump should be empty");
    }

    /// Ported from `TestTTLInsertWithMinValue`.
    #[test]
    fn test_ttl_insert_with_min_value() {
        let cache = DnsCache::new(60);
        let now = now_secs();
        cache
            .update("test.com", &["1.2.3.4".parse::<IpAddr>().unwrap()], 3)
            .unwrap();

        let res = cache.lookup_at("test.com", now);
        assert_eq!(res.as_ref().map(|v| v.len()), Some(1));

        let res = cache.lookup_at("test.com", now + 3);
        assert_eq!(res.as_ref().map(|v| v.len()), Some(1));

        let res = cache.lookup_at("test.com", now + 70);
        assert!(res.is_none(), "Expected entry to be expired at now+70");
    }

    /// Ported from `TestTTLInsertWithZeroValue`.
    #[test]
    fn test_ttl_insert_with_zero_value() {
        let cache = DnsCache::new(0);
        let now = now_secs();
        cache
            .update("test.com", &["1.2.3.4".parse::<IpAddr>().unwrap()], 10)
            .unwrap();

        let res = cache.lookup_at("test.com", now);
        assert_eq!(res.as_ref().map(|v| v.len()), Some(1));

        let res = cache.lookup_at("test.com", now + 10);
        assert_eq!(
            res.as_ref().map(|v| v.len()),
            Some(1),
            "Expected entry still visible at TTL boundary"
        );

        let res = cache.lookup_at("test.com", now + 11);
        assert!(res.is_none(), "Expected entry expired at now+11");
    }

    // ------------------------------------------------------------------
    // Zombie tests — ported from cilium/pkg/fqdn/cache_test.go
    // ------------------------------------------------------------------

    /// Helper: assert that `zombies` contains exactly the entries in `expected`.
    /// `expected` maps IP string → sorted list of names.
    fn assert_zombies_contain(zombies: &[DnsZombieMapping], expected: &[(&str, &[&str])]) {
        assert_eq!(
            zombies.len(),
            expected.len(),
            "Different number of zombies: got {:?} expected {:?}",
            zombies.iter().map(|z| z.ip.to_string()).collect::<Vec<_>>(),
            expected.iter().map(|(ip, _)| *ip).collect::<Vec<_>>(),
        );
        for zombie in zombies {
            let ip_str = zombie.ip.to_string();
            let expected_names = expected
                .iter()
                .find(|(ip, _)| *ip == ip_str)
                .unwrap_or_else(|| panic!("Unexpected zombie {ip_str}"));
            let mut got_names = zombie.names.clone();
            let mut exp_names: Vec<&str> = expected_names.1.to_vec();
            got_names.sort();
            exp_names.sort();
            assert_eq!(
                got_names,
                exp_names.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                "Names mismatch for IP {ip_str}",
            );
        }
    }

    // ported from TestZombiesSiblingsGC
    #[test]
    fn test_zombies_siblings_gc() {
        let now = SystemTime::now();
        // Use high defaults matching Go's defaults.ToFQDNsMaxDeferredConnectionDeletes=10000
        // and defaults.ToFQDNsMaxIPsPerHost=1000.
        let mut zombies = DnsZombieMappings::new(10000, 1000);

        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);
        zombies.upsert(now, "1.1.1.2".parse().unwrap(), &["test.com"]);
        zombies.upsert(now, "3.3.3.3".parse().unwrap(), &["pizza.com"]);

        // First CT GC: advances clock.
        let now2 = now + Duration::from_secs(300);
        let next = now2 + Duration::from_secs(300);
        zombies.set_ctgc_time(now2, next);

        // Mark 1.1.1.2 alive — its sibling 1.1.1.1 (same name) should stay alive too.
        let now3 = now2 + Duration::from_secs(1);
        zombies.mark_alive(now3 + Duration::from_secs(1), "1.1.1.2".parse().unwrap());
        zombies.set_ctgc_time(now3, next);

        let (alive, dead) = zombies.gc();
        assert_zombies_contain(
            &alive,
            &[("1.1.1.1", &["test.com"]), ("1.1.1.2", &["test.com"])],
        );
        assert_zombies_contain(&dead, &[("3.3.3.3", &["pizza.com"])]);
    }

    // ported from TestZombiesGC
    #[test]
    fn test_zombies_gc() {
        let now = SystemTime::now();
        let mut zombies = DnsZombieMappings::new(10000, 1000);

        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);
        zombies.upsert(now, "2.2.2.2".parse().unwrap(), &["somethingelse.com"]);

        // Without any MarkAlive or SetCTGCTime, all entries remain alive.
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com"]),
                ("2.2.2.2", &["somethingelse.com"]),
            ],
        );

        // Adding another name to 1.1.1.1 keeps it alive and adds the name.
        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["anotherthing.com"]);
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com", "anotherthing.com"]),
                ("2.2.2.2", &["somethingelse.com"]),
            ],
        );

        // First CT GC run — still alive (need 2 cycles).
        let now2 = now + Duration::from_secs(300);
        let next = now2 + Duration::from_secs(300);
        zombies.set_ctgc_time(now2, next);
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com", "anotherthing.com"]),
                ("2.2.2.2", &["somethingelse.com"]),
            ],
        );

        // Second CT GC — mark 2.2.2.2 alive; 1.1.1.1 dies.
        let now3 = now2 + Duration::from_secs(300);
        let next2 = now3 + Duration::from_secs(300);
        zombies.mark_alive(now3 + Duration::from_secs(1), "2.2.2.2".parse().unwrap());
        zombies.set_ctgc_time(now3, next2);

        let (alive, dead) = zombies.gc();
        assert_zombies_contain(&alive, &[("2.2.2.2", &["somethingelse.com"])]);
        assert_zombies_contain(&dead, &[("1.1.1.1", &["test.com", "anotherthing.com"])]);

        // Second GC call: only alive entries remain.
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_eq!(alive.len(), 1);

        // Re-add 1.1.1.1 and add another name to 2.2.2.2.
        zombies.upsert(now3, "2.2.2.2".parse().unwrap(), &["thelastthing.com"]);
        zombies.upsert(now3, "1.1.1.1".parse().unwrap(), &["onemorething.com"]);

        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["onemorething.com"]),
                ("2.2.2.2", &["somethingelse.com", "thelastthing.com"]),
            ],
        );

        // Cause all but 2.2.2.2 to die.
        let now4 = now3 + Duration::from_secs(300);
        let next3 = now4 + Duration::from_secs(300);
        zombies.set_ctgc_time(now4, next3);
        let now5 = now4 + Duration::from_secs(300);
        let next4 = now5 + Duration::from_secs(300);
        zombies.mark_alive(now5 + Duration::from_secs(1), "2.2.2.2".parse().unwrap());
        zombies.set_ctgc_time(now5, next4);
        let (alive, dead) = zombies.gc();
        assert_eq!(alive.len(), 1);
        assert_zombies_contain(
            &alive,
            &[("2.2.2.2", &["somethingelse.com", "thelastthing.com"])],
        );
        assert_zombies_contain(&dead, &[("1.1.1.1", &["onemorething.com"])]);

        // Cause all to die.
        let now6 = now5 + Duration::from_secs(2);
        zombies.set_ctgc_time(now6, next4);
        let (alive, dead) = zombies.gc();
        assert!(alive.is_empty());
        assert_zombies_contain(
            &dead,
            &[("2.2.2.2", &["somethingelse.com", "thelastthing.com"])],
        );
    }

    // ported from TestZombiesGCOverLimit
    #[test]
    fn test_zombies_gc_over_limit() {
        let now = SystemTime::now();
        let mut zombies = DnsZombieMappings::new(10000, 1);

        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);
        zombies.upsert(
            now,
            "2.2.2.2".parse().unwrap(),
            &["somethingelse.com", "test.com"],
        );
        zombies.upsert(now, "3.3.3.3".parse().unwrap(), &["anothertest.com"]);

        // 2.2.2.2 is more important (multiple names) → kept; 1.1.1.1 dies.
        let (alive, dead) = zombies.gc();
        assert_zombies_contain(&dead, &[("1.1.1.1", &["test.com"])]);
        assert_zombies_contain(
            &alive,
            &[
                ("2.2.2.2", &["somethingelse.com", "test.com"]),
                ("3.3.3.3", &["anothertest.com"]),
            ],
        );
    }

    // ported from TestZombiesGCOverLimitWithCTGC
    #[test]
    fn test_zombies_gc_over_limit_with_ctgc() {
        let now = SystemTime::now();
        let after_now = now + Duration::from_nanos(1);
        let max_connections = 3usize;
        let mut zombies = DnsZombieMappings::new(10000, max_connections);
        zombies.set_ctgc_time(now, after_now);

        for i in 1..=(max_connections + 1) {
            let ip: IpAddr = format!("1.1.1.{i}").parse().unwrap();
            zombies.upsert(now, ip, &["test.com"]);
        }

        // Mark first max_connections IPs alive.
        for i in 1..=max_connections {
            let ip: IpAddr = format!("1.1.1.{i}").parse().unwrap();
            zombies.mark_alive(after_now, ip);
        }
        zombies.set_ctgc_time(after_now, after_now + Duration::from_secs(300));

        let (alive, dead) = zombies.gc();
        assert_zombies_contain(&dead, &[("1.1.1.4", &["test.com"])]);
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com"]),
                ("1.1.1.2", &["test.com"]),
                ("1.1.1.3", &["test.com"]),
            ],
        );
    }

    // ported from TestZombiesGCDeferredDeletes
    #[test]
    fn test_zombies_gc_deferred_deletes() {
        let now = SystemTime::now();
        let mut zombies = DnsZombieMappings::new(10000, 1000);

        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);
        zombies.upsert(
            now + Duration::from_secs(1),
            "2.2.2.2".parse().unwrap(),
            &["somethingelse.com"],
        );
        zombies.upsert(
            now + Duration::from_secs(2),
            "3.3.3.3".parse().unwrap(),
            &["onemorething.com"],
        );

        // No zombies evicted (high limit).
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com"]),
                ("2.2.2.2", &["somethingelse.com"]),
                ("3.3.3.3", &["onemorething.com"]),
            ],
        );

        // New zombies with limit=2.
        let mut zombies = DnsZombieMappings::new(2, 1000);
        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);

        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(&alive, &[("1.1.1.1", &["test.com"])]);

        // Adding 2 more pushes over limit; 1.1.1.1 (earliest expiry) is evicted.
        zombies.upsert(
            now + Duration::from_secs(1),
            "2.2.2.2".parse().unwrap(),
            &["somethingelse.com"],
        );
        zombies.upsert(
            now + Duration::from_secs(2),
            "3.3.3.3".parse().unwrap(),
            &["onemorething.com"],
        );
        let (alive, dead) = zombies.gc();
        assert_zombies_contain(&dead, &[("1.1.1.1", &["test.com"])]);
        assert_zombies_contain(
            &alive,
            &[
                ("2.2.2.2", &["somethingelse.com"]),
                ("3.3.3.3", &["onemorething.com"]),
            ],
        );

        // Re-add 1.1.1.1; mark 1.1.1.1 and 2.2.2.2 alive — 3.3.3.3 dies.
        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);
        let gc_time = now + Duration::from_secs(4);
        let next = now + Duration::from_secs(4);
        zombies.mark_alive(gc_time, "1.1.1.1".parse().unwrap());
        zombies.mark_alive(gc_time, "2.2.2.2".parse().unwrap());
        zombies.set_ctgc_time(gc_time, next);

        let (alive, dead) = zombies.gc();
        assert_zombies_contain(&dead, &[("3.3.3.3", &["onemorething.com"])]);
        assert_zombies_contain(
            &alive,
            &[
                ("2.2.2.2", &["somethingelse.com"]),
                ("1.1.1.1", &["test.com"]),
            ],
        );
    }

    // ported from TestZombiesForceExpire
    #[test]
    fn test_zombies_force_expire() {
        let now = SystemTime::now();
        let mut zombies = DnsZombieMappings::new(10000, 1000);

        zombies.upsert(
            now,
            "1.1.1.1".parse().unwrap(),
            &["test.com", "anothertest.com"],
        );
        zombies.upsert(now, "2.2.2.2".parse().unwrap(), &["somethingelse.com"]);

        // Without any MarkAlive or SetCTGCTime, all entries remain alive.
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_eq!(alive.len(), 2);

        // Expire only "test.com" from 1.1.1.1.
        zombies.force_expire(UNIX_EPOCH, Some("test.com"));

        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["anothertest.com"]),
                ("2.2.2.2", &["somethingelse.com"]),
            ],
        );

        // Expire the last name on 1.1.1.1 — it gets deleted entirely.
        zombies.force_expire(UNIX_EPOCH, Some("anothertest.com"));
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(&alive, &[("2.2.2.2", &["somethingelse.com"])]);

        // Add test.com to 2.2.2.2.
        zombies.upsert(now, "2.2.2.2".parse().unwrap(), &["test.com"]);

        // ForceExpireByNameIP: non-matching IP — nothing removed.
        zombies.force_expire_by_name_ip(
            UNIX_EPOCH,
            "somethingelse.com",
            &["1.1.1.1".parse::<IpAddr>().unwrap()],
        );
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(&alive, &[("2.2.2.2", &["somethingelse.com", "test.com"])]);

        // Expire somethingelse.com for 2.2.2.2 — leaves test.com.
        zombies.force_expire_by_name_ip(
            UNIX_EPOCH,
            "somethingelse.com",
            &["2.2.2.2".parse::<IpAddr>().unwrap()],
        );
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(&alive, &[("2.2.2.2", &["test.com"])]);

        // Non-matching name — nothing removed.
        zombies.force_expire_by_name_ip(
            UNIX_EPOCH,
            "blarg.com",
            &["2.2.2.2".parse::<IpAddr>().unwrap()],
        );
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(&alive, &[("2.2.2.2", &["test.com"])]);

        // Clear everything.
        zombies.force_expire_by_name_ip(
            UNIX_EPOCH,
            "test.com",
            &["2.2.2.2".parse::<IpAddr>().unwrap()],
        );
        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert!(alive.is_empty());
    }

    // ported from TestCacheToZombiesGCCascade
    #[test]
    fn test_cache_to_zombies_gc_cascade() {
        let now = SystemTime::now();
        let now_secs_val = unix_secs(now);
        let cache = DnsCache::new(0);
        let mut zombies = DnsZombieMappings::new(10000, 1000);

        // Add entries that expire at different times.
        cache
            .update_at(
                "test.com",
                &[
                    "1.1.1.1".parse::<IpAddr>().unwrap(),
                    "2.2.2.2".parse::<IpAddr>().unwrap(),
                ],
                3,
                now_secs_val,
            )
            .unwrap();
        cache
            .update_at(
                "test.com",
                &["3.3.3.3".parse::<IpAddr>().unwrap()],
                5,
                now_secs_val,
            )
            .unwrap();

        // 4s later: 1.1.1.1 and 2.2.2.2 have expired (TTL=3), 3.3.3.3 still alive.
        let now2 = now + Duration::from_secs(4);
        let affected = cache.gc(now2, Some(&mut zombies));
        assert_eq!(affected.len(), 1, "Expected test.com in affected");
        assert!(affected.contains("test.com"));

        // 3.3.3.3 still in cache; 1.1.1.1 and 2.2.2.2 removed.
        assert!(
            cache.forward_contains("test.com"),
            "test.com should still be in cache"
        );
        assert!(cache.reverse_contains("3.3.3.3".parse().unwrap()));
        assert!(!cache.reverse_contains("1.1.1.1".parse().unwrap()));
        assert!(!cache.reverse_contains("2.2.2.2".parse().unwrap()));

        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[("1.1.1.1", &["test.com"]), ("2.2.2.2", &["test.com"])],
        );

        // 8s later (total): 3.3.3.3 also expired.
        let now3 = now2 + Duration::from_secs(4);
        let affected = cache.gc(now3, Some(&mut zombies));
        assert_eq!(affected.len(), 1, "Expected test.com in affected");
        assert!(
            !cache.forward_contains("test.com"),
            "test.com should be removed from cache"
        );

        let (alive, dead) = zombies.gc();
        assert!(dead.is_empty());
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com"]),
                ("2.2.2.2", &["test.com"]),
                ("3.3.3.3", &["test.com"]),
            ],
        );
    }

    // ported from TestZombiesDumpAlive
    #[test]
    fn test_zombies_dump_alive() {
        let now = SystemTime::now();
        let mut zombies = DnsZombieMappings::new(10000, 1000);

        // Empty.
        let alive = zombies.dump_alive::<fn(IpAddr) -> bool>(None);
        assert!(alive.is_empty());

        zombies.upsert(now, "1.1.1.1".parse().unwrap(), &["test.com"]);
        zombies.upsert(now, "2.2.2.2".parse().unwrap(), &["example.com"]);
        zombies.upsert(now, "3.3.3.3".parse().unwrap(), &["example.org"]);

        let alive = zombies.dump_alive::<fn(IpAddr) -> bool>(None);
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com"]),
                ("2.2.2.2", &["example.com"]),
                ("3.3.3.3", &["example.org"]),
            ],
        );

        // First CT GC — still need 2 cycles, so all still alive.
        let now2 = now + Duration::from_secs(1);
        let next = now2 + Duration::from_secs(300);
        zombies.set_ctgc_time(now2, next);
        let alive = zombies.dump_alive::<fn(IpAddr) -> bool>(None);
        assert_zombies_contain(
            &alive,
            &[
                ("1.1.1.1", &["test.com"]),
                ("2.2.2.2", &["example.com"]),
                ("3.3.3.3", &["example.org"]),
            ],
        );

        // Second CT GC — mark 1.1.1.1 and 2.2.2.2 alive; 3.3.3.3 dies.
        let now3 = now2 + Duration::from_secs(300);
        let next2 = now3 + Duration::from_secs(300);
        zombies.mark_alive(now3, "1.1.1.1".parse().unwrap());
        zombies.mark_alive(now3, "2.2.2.2".parse().unwrap());
        zombies.set_ctgc_time(now3, next2);

        let alive = zombies.dump_alive::<fn(IpAddr) -> bool>(None);
        assert_zombies_contain(
            &alive,
            &[("1.1.1.1", &["test.com"]), ("2.2.2.2", &["example.com"])],
        );

        // cidrMatcher = false for all → empty.
        let alive = zombies.dump_alive(Some(|_: IpAddr| false));
        assert!(alive.is_empty());

        // cidrMatcher = true for all.
        let alive = zombies.dump_alive(Some(|_: IpAddr| true));
        assert_zombies_contain(
            &alive,
            &[("1.1.1.1", &["test.com"]), ("2.2.2.2", &["example.com"])],
        );

        // Only 1.1.1.0/24 prefix.
        let alive = zombies.dump_alive(Some(|ip: IpAddr| match ip {
            IpAddr::V4(v4) => {
                let octs = v4.octets();
                octs[0] == 1 && octs[1] == 1 && octs[2] == 1
            }
            _ => false,
        }));
        assert_zombies_contain(&alive, &[("1.1.1.1", &["test.com"])]);

        // Add 1.1.1.2 — should also appear under prefix.
        zombies.upsert(now3, "1.1.1.2".parse().unwrap(), &["test2.com"]);
        let alive = zombies.dump_alive(Some(|ip: IpAddr| match ip {
            IpAddr::V4(v4) => {
                let octs = v4.octets();
                octs[0] == 1 && octs[1] == 1 && octs[2] == 1
            }
            _ => false,
        }));
        assert_zombies_contain(
            &alive,
            &[("1.1.1.1", &["test.com"]), ("1.1.1.2", &["test2.com"])],
        );

        // 4.4.0.0/16 — no matches.
        let alive = zombies.dump_alive(Some(|ip: IpAddr| match ip {
            IpAddr::V4(v4) => {
                let octs = v4.octets();
                octs[0] == 4 && octs[1] == 4
            }
            _ => false,
        }));
        assert!(alive.is_empty());
    }

    // ported from TestOverlimitPreferNewerEntries
    #[test]
    fn test_overlimit_prefer_newer_entries() {
        let to_fqdns_min_ttl = 100u32;
        let to_fqdns_max_ips_per_host = 5usize;
        let cache = DnsCache::new_with_limit(to_fqdns_min_ttl, to_fqdns_max_ips_per_host);

        let to_fqdns_max_deferred = 10usize;
        let mut zombies = DnsZombieMappings::new(to_fqdns_max_deferred, to_fqdns_max_ips_per_host);

        let name = "test.com";
        let ips: Vec<IpAddr> = (1u8..=20)
            .map(|i| format!("1.1.1.{i}").parse::<IpAddr>().unwrap())
            .collect();

        let now = SystemTime::now();
        let now_secs_val = unix_secs(now);
        for (i, ip) in ips.iter().enumerate() {
            // Entries with lower last-octet expire earlier.
            let lookup_time = now_secs_val - (ips.len() - i) as i64;
            cache
                .update_at(
                    name,
                    &[*ip],
                    0, /* overridden by min_ttl */
                    lookup_time,
                )
                .unwrap();
        }

        // Set last_cleanup so cleanup runs.
        {
            let mut g = cache.inner.lock().unwrap();
            if g.last_cleanup.is_none() {
                g.last_cleanup = Some(now_secs_val - (ips.len() as i64 + 1));
            }
        }

        let affected = cache.gc(now, Some(&mut zombies));
        assert_eq!(affected.len(), 1);
        assert!(affected.contains(name));

        // Cache should keep only toFQDNsMaxIPsPerHost IPs.
        {
            let g = cache.inner.lock().unwrap();
            if let Some(ip_map) = g.forward.get(name) {
                assert_eq!(
                    ip_map.len(),
                    to_fqdns_max_ips_per_host,
                    "Cache should keep exactly {to_fqdns_max_ips_per_host} IPs"
                );
            }
        }

        let (alive, dead) = zombies.gc();

        assert_eq!(
            alive.len(),
            to_fqdns_max_ips_per_host,
            "Should have {to_fqdns_max_ips_per_host} alive zombies"
        );

        // More recent entries (higher index = later expiry) should be kept alive.
        let alive_ips: std::collections::HashSet<String> =
            alive.iter().map(|z| z.ip.to_string()).collect();
        for ip in &["1.1.1.11", "1.1.1.12", "1.1.1.13", "1.1.1.14", "1.1.1.15"] {
            assert!(alive_ips.contains(*ip), "Expected {ip} in alive zombies");
        }

        // Older entries should be dead.
        let dead_ips: std::collections::HashSet<String> =
            dead.iter().map(|z| z.ip.to_string()).collect();
        for ip in &[
            "1.1.1.1", "1.1.1.2", "1.1.1.3", "1.1.1.4", "1.1.1.5", "1.1.1.6", "1.1.1.7", "1.1.1.8",
            "1.1.1.9", "1.1.1.10",
        ] {
            assert!(dead_ips.contains(*ip), "Expected {ip} in dead zombies");
        }
    }

    // ported from TestPerHostLimitBehaviourForS3
    #[test]
    fn test_per_host_limit_behaviour_for_s3() {
        let some_domain = "s3.example.com";
        let max_ips = 5usize;
        let dns_ttl = 4u32;

        let cache = DnsCache::new_with_limit(0, max_ips);
        let mut z = DnsZombieMappings::new(10000, max_ips);

        let really_old_lookup: Vec<IpAddr> = vec!["1.0.0.1".parse().unwrap()];
        let recent_lookup: Vec<IpAddr> = vec![
            "1.1.0.1".parse().unwrap(),
            "1.1.0.2".parse().unwrap(),
            "1.1.0.3".parse().unwrap(),
            "1.1.0.4".parse().unwrap(),
            "1.1.0.5".parse().unwrap(),
        ];
        let keepalive_lookup: IpAddr = "1.2.0.1".parse().unwrap();

        let simulate_fqdn_gc = |when: SystemTime, z: &mut DnsZombieMappings| -> HashSet<String> {
            let names = cache.gc(when, Some(z));
            z.gc();
            names
        };

        let simulate_ct_gc = |when: SystemTime, alive_ips: &[IpAddr], z: &mut DnsZombieMappings| {
            for ip in alive_ips {
                z.mark_alive(when, *ip);
            }
            z.set_ctgc_time(when, when + Duration::from_secs(10));
        };

        let tick = |tock: SystemTime, z: &mut DnsZombieMappings| {
            let tock_secs = unix_secs(tock);
            cache
                .update_at(some_domain, &[keepalive_lookup], dns_ttl, tock_secs)
                .unwrap();
            let gc_time = tock + Duration::from_secs((2 * dns_ttl + 1) as u64);
            simulate_fqdn_gc(gc_time, z);
            let ct_time = gc_time + Duration::from_secs(1);
            simulate_ct_gc(ct_time, &[], z);
        };

        let now = SystemTime::now();
        let a_long_time_ago = now - Duration::from_secs(36000);
        let seven_seconds_ago = now - Duration::from_secs(7);

        // A long time ago, we looked up the domain.
        let alt_secs = unix_secs(a_long_time_ago);
        cache
            .update_at(some_domain, &really_old_lookup, dns_ttl, alt_secs)
            .unwrap();

        // FQDN GC: moves old lookup to zombies.
        simulate_fqdn_gc(a_long_time_ago + Duration::from_secs(8), &mut z);
        // CT GC marks it alive.
        simulate_ct_gc(
            a_long_time_ago + Duration::from_secs(10),
            &really_old_lookup,
            &mut z,
        );
        assert!(
            z.contains_ip(really_old_lookup[0]),
            "expected really old lookup to still be present"
        );

        // Keep-alive ticks.
        tick(a_long_time_ago + Duration::from_secs(20), &mut z);
        tick(a_long_time_ago + Duration::from_secs(30), &mut z);
        tick(a_long_time_ago + Duration::from_secs(40), &mut z);
        tick(a_long_time_ago + Duration::from_secs(50), &mut z);
        assert!(
            z.contains_ip(really_old_lookup[0]),
            "expected really old lookup to still be present"
        );

        // Recent lookup with multiple IPs.
        let seven_secs = unix_secs(seven_seconds_ago);
        cache
            .update_at(some_domain, &recent_lookup, dns_ttl, seven_secs)
            .unwrap();
        // FQDN GC: should push over-limit IPs to zombies.
        let affected_names = simulate_fqdn_gc(now, &mut z);
        assert_eq!(affected_names.len(), 1);
        assert!(affected_names.contains(some_domain));

        // Zombies should respect the limit.
        assert_eq!(
            z.len_deletes(),
            max_ips,
            "expected zombies to contain {max_ips} entries, but has {}",
            z.len_deletes()
        );
        // Old lookup should be gone.
        assert!(
            !z.contains_ip(really_old_lookup[0]),
            "expected really old lookup not to be present"
        );
        // Recent lookups should be present.
        for ip in &recent_lookup {
            assert!(
                z.contains_ip(*ip),
                "expected recent lookup {ip} to be present"
            );
        }
    }

    // ported from Test_sortZombieMappingSlice
    #[test]
    fn test_sort_zombie_mapping_slice() {
        let moments = [
            UNIX_EPOCH + Duration::from_secs(978307261), // 2001-01-01
            UNIX_EPOCH + Duration::from_secs(1012615322), // 2002-02-02
            UNIX_EPOCH + Duration::from_secs(1046660583), // 2003-03-03
        ];

        // Helper to make a zombie.
        let make_zombie =
            |alive_at: SystemTime, delete_pending_at: SystemTime, names: Vec<&str>| {
                DnsZombieMapping {
                    names: names.iter().map(|s| s.to_string()).collect(),
                    ip: "0.0.0.0".parse().unwrap(),
                    alive_at,
                    delete_pending_at,
                    revision_added_at: 0,
                }
            };

        // Validation: check properties for every pair.
        let validate = |zombies: &[DnsZombieMapping]| {
            let sl = zombies.len();
            for i in 0..sl {
                for j in (i + 1)..sl {
                    let a = &zombies[i];
                    let b = &zombies[j];
                    if a.alive_at < b.alive_at {
                        continue;
                    } else if a.alive_at > b.alive_at {
                        panic!(
                            "order wrong: AliveAt: {:?} is after {:?}",
                            a.alive_at, b.alive_at
                        );
                    }
                    if a.delete_pending_at < b.delete_pending_at {
                        continue;
                    } else if a.delete_pending_at > b.delete_pending_at {
                        panic!(
                            "order wrong: DeletePendingAt: {:?} is after {:?}",
                            a.delete_pending_at, b.delete_pending_at
                        );
                    }
                    if a.names.len() > b.names.len() {
                        panic!(
                            "order wrong: len(names): {:?} longer than {:?}",
                            a.names, b.names
                        );
                    }
                }
            }
        };

        // Edge cases.
        let mut empty: Vec<DnsZombieMapping> = vec![];
        sort_zombie_mapping_slice(&mut empty);
        assert!(empty.is_empty());

        let mut single = vec![make_zombie(moments[0], moments[1], vec!["test.com"])];
        sort_zombie_mapping_slice(&mut single);
        assert_eq!(single.len(), 1);

        let mut swapped_alive = vec![
            make_zombie(moments[2], UNIX_EPOCH, vec![]),
            make_zombie(moments[0], UNIX_EPOCH, vec![]),
        ];
        sort_zombie_mapping_slice(&mut swapped_alive);
        validate(&swapped_alive);

        let mut equal_alive_swapped_delete = vec![
            make_zombie(moments[0], moments[2], vec![]),
            make_zombie(moments[0], moments[1], vec![]),
        ];
        sort_zombie_mapping_slice(&mut equal_alive_swapped_delete);
        validate(&equal_alive_swapped_delete);

        let mut tiebreaker = vec![
            make_zombie(moments[0], moments[1], vec!["test.com", "test2.com"]),
            make_zombie(moments[0], moments[1], vec!["test.com"]),
        ];
        sort_zombie_mapping_slice(&mut tiebreaker);
        validate(&tiebreaker);

        // Generate all combinations of moments and name counts.
        let names = ["example.org", "test.com"];
        let mut all_mappings: Vec<DnsZombieMapping> = Vec::new();
        for &mi in &moments {
            for &mj in &moments {
                for k in 0..names.len() {
                    all_mappings.push(make_zombie(mi, mj, names[..k].to_vec()));
                }
            }
        }

        // Run 5 random shuffle + sort tests.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        for seed in 0u64..5 {
            let mut ts = all_mappings.clone();
            // Simple deterministic shuffle using seed.
            let n = ts.len();
            for i in 0..n {
                let mut h = DefaultHasher::new();
                (seed, i as u64).hash(&mut h);
                let j = h.finish() as usize % n;
                ts.swap(i, j);
            }
            let orig_len = ts.len();
            sort_zombie_mapping_slice(&mut ts);
            assert_eq!(ts.len(), orig_len, "length changed by sorting");
            validate(&ts);
        }
    }

    // ------------------------------------------------------------------
    // TestReverseUpdateLookup
    // ------------------------------------------------------------------
    #[test]
    fn test_reverse_update_lookup() {
        let ip1: IpAddr = "2.2.2.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();
        let ip3: IpAddr = "2.2.2.3".parse().unwrap();
        let shared_ip: IpAddr = "1.1.1.1".parse().unwrap();
        let now = now_secs();

        let cache = DnsCache::new(0);
        cache
            .update_at("test1.com", &[shared_ip, ip1], 2, now)
            .unwrap();
        cache
            .update_at("test2.com", &[shared_ip, ip2], 4, now)
            .unwrap();

        // Within TTL for both names.
        let current = now + 1;
        let mut names = cache.lookup_ip_at(shared_ip, current);
        assert_eq!(names.len(), 2, "Expected 2 names for shared IP at t+1");
        names.sort();
        assert!(names.contains(&"test1.com".to_string()));
        assert!(names.contains(&"test2.com".to_string()));

        let names = cache.lookup_ip_at(ip1, current);
        assert_eq!(names, vec!["test1.com"]);
        let names = cache.lookup_ip_at(ip2, current);
        assert_eq!(names, vec!["test2.com"]);
        let names = cache.lookup_ip_at(ip3, current);
        assert!(names.is_empty(), "Expected no names for ip3");

        // 3 seconds later: test1.com expired (TTL=2), only test2.com alive.
        let current = now + 3;
        let names = cache.lookup_ip_at(shared_ip, current);
        assert_eq!(names, vec!["test2.com"]);
        let names = cache.lookup_ip_at(ip1, current);
        assert!(names.is_empty());
        let names = cache.lookup_ip_at(ip2, current);
        assert_eq!(names, vec!["test2.com"]);

        // 5 seconds later: everything expired.
        let current = now + 5;
        for ip in &[shared_ip, ip1, ip2, ip3] {
            let names = cache.lookup_ip_at(*ip, current);
            assert!(names.is_empty(), "Expected no names for {ip} at t+5");
        }
    }

    // ------------------------------------------------------------------
    // TestJSONMarshal
    // ------------------------------------------------------------------
    #[test]
    fn test_json_marshal() {
        let ip1: IpAddr = "2.2.2.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();
        let ip3: IpAddr = "2.2.2.3".parse().unwrap();
        let shared_ip: IpAddr = "1.1.1.1".parse().unwrap();
        let now = now_secs();

        let cache = DnsCache::new(0);
        cache.update_at("test1.com", &[shared_ip], 5, now).unwrap();
        cache.update_at("test2.com", &[shared_ip], 5, now).unwrap();
        cache.update_at("test3.com", &[shared_ip], 5, now).unwrap();
        cache.update_at("test1.com", &[ip1], 5, now).unwrap();
        cache.update_at("test2.com", &[ip2], 5, now).unwrap();
        cache.update_at("test3.com", &[ip3], 5, now).unwrap();

        let json_str = cache.marshal_json();

        // Parse raw to count entries — should be 6 (one per IP per name).
        let raw: Vec<CacheEntry> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(raw.len(), 6, "Expected 6 entries in marshalled JSON");

        // Unmarshal into a new cache.
        let new_cache = DnsCache::new(0);
        new_cache.unmarshal_json(&json_str).unwrap();

        // Check data at insertion time.
        let names_to_check = ["test1.com", "test2.com", "test3.com"];
        let unique_ips = [ip1, ip2, ip3];
        for (i, name) in names_to_check.iter().enumerate() {
            let mut ips = new_cache.lookup_at(name, now).unwrap_or_default();
            ips.sort();
            assert_eq!(ips.len(), 2, "Expected 2 IPs for {name}");
            assert!(ips.contains(&shared_ip));
            assert!(ips.contains(&unique_ips[i]));
        }

        // Check data is expired 10 s later.
        for name in &names_to_check {
            let ips = new_cache.lookup_at(name, now + 10);
            assert!(
                ips.is_none() || ips.unwrap().is_empty(),
                "Expected {name} to be expired at t+10"
            );
        }
    }

    // ------------------------------------------------------------------
    // TestCountIPs
    // ------------------------------------------------------------------
    #[test]
    fn test_count_ips() {
        let cache = DnsCache::new(0);
        let shared_ip: IpAddr = "8.8.8.8".parse().unwrap();
        let ip1: IpAddr = "1.1.1.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();
        let ip3: IpAddr = "3.3.3.3".parse().unwrap();

        cache.update("test1.com", &[shared_ip, ip1], 5).unwrap();
        cache.update("test2.com", &[shared_ip, ip2], 5).unwrap();
        cache.update("test3.com", &[shared_ip, ip3], 5).unwrap();

        let (fqdns, ips) = cache.count();
        assert_eq!(fqdns, 3, "Expected 3 FQDNs");
        assert_eq!(ips, 6, "Expected 6 IPs (3 names × 2 IPs each)");
    }

    // ------------------------------------------------------------------
    // TestTTLCleanupEntries
    // ------------------------------------------------------------------
    #[test]
    fn test_ttl_cleanup_entries() {
        let cache = DnsCache::new(0);
        let now = now_secs();
        cache
            .update_at("test.com", &["1.2.3.4".parse::<IpAddr>().unwrap()], 3, now)
            .unwrap();
        assert_eq!(
            cache.cleanup_len(),
            1,
            "Expected 1 cleanup bucket after insert"
        );

        let affected =
            cache.cleanup_expired_entries(SystemTime::now() + std::time::Duration::from_secs(5));
        assert_eq!(affected.len(), 1, "Expected 1 affected name");
        assert_eq!(cache.cleanup_len(), 0, "Expected cleanup to be empty");
        assert!(
            cache.lookup("test.com").is_none(),
            "test.com should be expired"
        );
    }

    // ------------------------------------------------------------------
    // TestTTLCleanupWithoutForward
    // ------------------------------------------------------------------
    #[test]
    fn test_ttl_cleanup_without_forward() {
        let cache = DnsCache::new(0);
        let now = now_secs();
        // Manually insert into cleanup without a corresponding forward entry.
        cache.insert_cleanup_entry(now, "test.com");
        cache.set_last_cleanup(now - 60); // one minute ago
        let affected =
            cache.cleanup_expired_entries(SystemTime::now() + std::time::Duration::from_secs(5));
        // No forward entry → nothing really removed, but no panic.
        assert!(
            affected.is_empty(),
            "Expected no affected names when forward is absent"
        );
        assert_eq!(cache.cleanup_len(), 0, "Cleanup bucket should be consumed");
    }

    // ------------------------------------------------------------------
    // TestOverlimitEntriesWithValidLimit
    // ------------------------------------------------------------------
    #[test]
    fn test_overlimit_entries_with_valid_limit() {
        let limit = 5usize;
        let cache = DnsCache::new_with_limit(0, limit);

        cache
            .update("foo.bar", &["1.1.1.1".parse::<IpAddr>().unwrap()], 1)
            .unwrap();
        cache
            .update("bar.foo", &["2.1.1.1".parse::<IpAddr>().unwrap()], 1)
            .unwrap();
        for i in 1..=(limit + 1) {
            cache
                .update(
                    "test.com",
                    &[format!("1.1.1.{i}").parse::<IpAddr>().unwrap()],
                    i as u32,
                )
                .unwrap();
        }

        let affected = cache.cleanup_over_limit_entries();
        assert_eq!(
            affected,
            vec!["test.com"],
            "Expected only test.com affected"
        );

        let ips = cache.lookup("test.com").unwrap_or_default();
        assert_eq!(
            ips.len(),
            limit,
            "Expected exactly {limit} IPs after cleanup"
        );

        // 1.1.1.1 was the lowest-TTL IP for test.com → removed; foo.bar still has it.
        let foo_ips = cache.lookup("foo.bar").unwrap_or_default();
        assert_eq!(foo_ips.len(), 1);
        let bar_ips = cache.lookup("bar.foo").unwrap_or_default();
        assert_eq!(bar_ips.len(), 1);

        assert_eq!(
            cache.over_limit_len(),
            0,
            "over_limit should be empty after cleanup"
        );
    }

    // ------------------------------------------------------------------
    // TestOverlimitEntriesWithoutLimit
    // ------------------------------------------------------------------
    #[test]
    fn test_overlimit_entries_without_limit() {
        let cache = DnsCache::new_with_limit(0, 0);
        for i in 0..5usize {
            cache
                .update(
                    "test.com",
                    &[format!("1.1.1.{i}").parse::<IpAddr>().unwrap()],
                    i as u32,
                )
                .unwrap();
        }
        let affected = cache.cleanup_over_limit_entries();
        assert!(
            affected.is_empty(),
            "Expected no names affected when limit=0"
        );
        let ips = cache.lookup("test.com").unwrap_or_default();
        assert_eq!(ips.len(), 5, "All 5 IPs should remain");
    }

    // ------------------------------------------------------------------
    // TestGCOverlimitAfterTTLCleanup
    // ------------------------------------------------------------------
    #[test]
    fn test_gc_overlimit_after_ttl_cleanup() {
        let limit = 5usize;
        let cache = DnsCache::new_with_limit(0, limit);
        let now = now_secs();
        // Set last_cleanup to 1 minute ago so cleanup bucket is old enough.
        cache.set_last_cleanup(now - 60);

        for i in 1..=(limit + 1) {
            cache
                .update_at(
                    "test.com",
                    &[format!("1.1.1.{i}").parse::<IpAddr>().unwrap()],
                    1, // TTL=1 → all expire quickly
                    now - 60,
                )
                .unwrap();
        }

        // All entries should be visible before cleanup.
        let ips = cache.lookup_at("test.com", now - 60).unwrap_or_default();
        assert_eq!(ips.len(), limit + 1);
        assert_eq!(cache.over_limit_len(), 1);

        // Run TTL cleanup (5 s in the future → all expired).
        let affected =
            cache.cleanup_expired_entries(SystemTime::now() + std::time::Duration::from_secs(5));
        assert!(affected.contains(&"test.com".to_string()));

        // Now over_limit cleanup should return nothing (entries already gone).
        let affected = cache.cleanup_over_limit_entries();
        assert!(
            affected.is_empty(),
            "Expected empty after TTL cleanup removed all entries"
        );
    }

    // ------------------------------------------------------------------
    // TestOverlimitAfterDeleteForwardEntry
    // ------------------------------------------------------------------
    #[test]
    fn test_overlimit_after_delete_forward_entry() {
        // If over_limit contains a name with no forward entry, cleanup should be a no-op.
        let cache = DnsCache::new(0);
        // Manually poke over_limit (via internal accessor).
        {
            let mut g = cache.inner.lock().unwrap();
            g.over_limit.insert("test.com".to_string());
        }
        let affected = cache.cleanup_over_limit_entries();
        assert!(
            affected.is_empty(),
            "Expected empty when forward entry is absent"
        );
    }

    // ------------------------------------------------------------------
    // Test_forceExpiredByNames
    // ------------------------------------------------------------------
    #[test]
    fn test_force_expire_by_names() {
        let names_to_expire = vec!["test1.com", "test2.com"];
        let cache = DnsCache::new(0);
        let now = now_secs();
        for i in 1..=3usize {
            cache
                .update_at(
                    format!("test{i}.com"),
                    &[format!("1.1.1.{i}").parse::<IpAddr>().unwrap()],
                    5,
                    now,
                )
                .unwrap();
        }
        assert_eq!(cache.len(), 3);

        let expire_before = SystemTime::now() + std::time::Duration::from_secs(1); // future → all lookups < this
        cache.force_expire_by_names(expire_before, &names_to_expire);

        // test3.com must survive.
        assert!(
            cache.lookup_at("test3.com", now).is_some(),
            "test3.com should still exist"
        );
        // test1 and test2 must be gone.
        assert!(cache.lookup_at("test1.com", now).is_none());
        assert!(cache.lookup_at("test2.com", now).is_none());
    }
}
