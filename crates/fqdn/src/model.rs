//! Pure FQDN policy and cache data model types.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};

/// A fully-qualified domain name normalized to lowercase without a trailing dot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FQDN(String);

impl FQDN {
    /// Creates a new normalized FQDN.
    pub fn new(name: impl Into<String>) -> Self {
        let normalized = name.into().to_lowercase();
        let normalized = normalized.trim_end_matches('.').to_string();
        Self(normalized)
    }

    /// Returns the normalized string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns true when the name begins with a wildcard marker.
    pub fn is_wildcard(&self) -> bool {
        self.0.starts_with('*')
    }
}

impl fmt::Display for FQDN {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A DNS match pattern for either an exact name or a wildcard suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DNSPattern {
    /// Matches a single exact FQDN.
    Exact(FQDN),
    /// Matches any name ending in the stored suffix after `*`.
    Wildcard(String),
}

impl DNSPattern {
    /// Parses a selector string into an exact or wildcard DNS pattern.
    pub fn parse(s: &str) -> Self {
        if let Some(suffix) = s.strip_prefix('*') {
            Self::Wildcard(suffix.to_lowercase().trim_end_matches('.').to_string())
        } else {
            Self::Exact(FQDN::new(s))
        }
    }

    /// Returns true when the pattern matches the provided FQDN.
    pub fn matches(&self, name: &FQDN) -> bool {
        match self {
            Self::Exact(fqdn) => fqdn == name,
            Self::Wildcard(suffix) => name.as_str().ends_with(suffix.as_str()),
        }
    }
}

/// A single DNS cache entry storing the latest IPs and TTL for a name.
#[derive(Debug, Clone)]
pub struct DNSCacheEntry {
    /// The normalized DNS name for this entry.
    pub name: FQDN,
    /// The IPs currently associated with the name.
    pub ips: Vec<IpAddr>,
    /// The TTL applied to this lookup result.
    pub ttl: Duration,
    /// The time at which the lookup was recorded.
    pub lookup_time: SystemTime,
}

impl DNSCacheEntry {
    /// Creates a new DNS cache entry with the current lookup time.
    pub fn new(name: FQDN, ips: Vec<IpAddr>, ttl: Duration) -> Self {
        Self {
            name,
            ips,
            ttl,
            lookup_time: SystemTime::now(),
        }
    }

    /// Returns true if the entry has expired.
    pub fn is_expired(&self) -> bool {
        self.lookup_time
            .elapsed()
            .map(|elapsed| elapsed > self.ttl)
            .unwrap_or(true)
    }

    /// Returns the IPs if the entry is still valid.
    pub fn valid_ips(&self) -> Option<&[IpAddr]> {
        if self.is_expired() {
            None
        } else {
            Some(&self.ips)
        }
    }
}

/// In-memory DNS cache containing the latest entry for each FQDN.
#[derive(Debug, Default)]
pub struct DNSCache {
    entries: HashMap<FQDN, DNSCacheEntry>,
}

impl DNSCache {
    /// Creates an empty DNS cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a cache entry for its FQDN.
    pub fn update(&mut self, entry: DNSCacheEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Looks up a cache entry by name.
    pub fn lookup(&self, name: &FQDN) -> Option<&DNSCacheEntry> {
        self.entries.get(name)
    }

    /// Returns the valid IPs for a name, or an empty vector when not found or expired.
    pub fn lookup_ips(&self, name: &FQDN) -> Vec<IpAddr> {
        self.entries
            .get(name)
            .and_then(DNSCacheEntry::valid_ips)
            .map(<[IpAddr]>::to_vec)
            .unwrap_or_default()
    }

    /// Removes expired entries and returns the number removed.
    pub fn gc(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, entry| !entry.is_expired());
        before - self.entries.len()
    }

    /// Returns the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when the cache has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Matches endpoints by DNS name patterns used in `toFQDNs` policy rules.
#[derive(Debug, Clone)]
pub struct FQDNSelector {
    /// The set of DNS match patterns for this selector.
    pub patterns: Vec<DNSPattern>,
}

impl FQDNSelector {
    /// Creates a new selector from DNS patterns.
    pub fn new(patterns: Vec<DNSPattern>) -> Self {
        Self { patterns }
    }

    /// Returns true when any pattern matches the provided name.
    pub fn matches(&self, name: &FQDN) -> bool {
        self.patterns.iter().any(|pattern| pattern.matches(name))
    }

    /// Resolves all unique IPs in the cache that match this selector.
    pub fn resolve_ips(&self, cache: &DNSCache) -> Vec<IpAddr> {
        let mut ips = HashSet::new();
        for (name, entry) in &cache.entries {
            if self.matches(name)
                && let Some(valid_ips) = entry.valid_ips()
            {
                ips.extend(valid_ips.iter().copied());
            }
        }

        let mut result: Vec<IpAddr> = ips.into_iter().collect();
        result.sort();
        result
    }
}

/// Tracks active FQDN selectors together with the local DNS cache state.
#[derive(Debug, Default)]
pub struct NameManager {
    selectors: Vec<FQDNSelector>,
    cache: DNSCache,
}

impl NameManager {
    /// Creates a new empty name manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a selector to the managed selector set.
    pub fn add_selector(&mut self, sel: FQDNSelector) {
        self.selectors.push(sel);
    }

    /// Updates the DNS cache with a new lookup result.
    pub fn update_dns(&mut self, entry: DNSCacheEntry) {
        self.cache.update(entry);
    }

    /// Resolves all IPs currently matching the supplied selector.
    pub fn ips_for_selector(&self, sel: &FQDNSelector) -> Vec<IpAddr> {
        sel.resolve_ips(&self.cache)
    }

    /// Returns the managed DNS cache.
    pub fn cache(&self) -> &DNSCache {
        &self.cache
    }

    /// Returns the number of registered selectors.
    pub fn selector_count(&self) -> usize {
        self.selectors.len()
    }
}

/// Errors returned by pure FQDN data model operations.
#[derive(Debug, thiserror::Error)]
pub enum FQDNError {
    /// The provided DNS name was invalid.
    #[error("invalid FQDN: {0}")]
    InvalidName(String),
    /// A DNS lookup operation failed.
    #[error("DNS lookup failed: {0}")]
    LookupFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fqdn_normalization() {
        let f = FQDN::new("WWW.EXAMPLE.COM.");
        assert_eq!(f.as_str(), "www.example.com");
        assert!(!f.is_wildcard());
    }

    #[test]
    fn test_dns_pattern_exact_match() {
        let pat = DNSPattern::parse("api.example.com");
        assert!(pat.matches(&FQDN::new("api.example.com")));
        assert!(!pat.matches(&FQDN::new("other.example.com")));
    }

    #[test]
    fn test_dns_pattern_wildcard_match() {
        let pat = DNSPattern::parse("*.example.com");
        assert!(pat.matches(&FQDN::new("api.example.com")));
        assert!(pat.matches(&FQDN::new("foo.example.com")));
        assert!(!pat.matches(&FQDN::new("example.com")));
        assert!(!pat.matches(&FQDN::new("other.net")));
    }

    #[test]
    fn test_dns_cache_lookup_and_gc() {
        let mut cache = DNSCache::new();
        let name = FQDN::new("api.svc.local");
        let entry = DNSCacheEntry::new(
            name.clone(),
            vec!["10.0.0.1".parse().unwrap()],
            Duration::from_mins(5),
        );
        cache.update(entry);
        assert_eq!(
            cache.lookup_ips(&name),
            vec!["10.0.0.1".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(cache.gc(), 0);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_fqdn_selector_resolve() {
        let mut cache = DNSCache::new();
        cache.update(DNSCacheEntry::new(
            FQDN::new("api.example.com"),
            vec!["1.2.3.4".parse().unwrap()],
            Duration::from_mins(5),
        ));
        cache.update(DNSCacheEntry::new(
            FQDN::new("other.net"),
            vec!["5.6.7.8".parse().unwrap()],
            Duration::from_mins(5),
        ));
        let sel = FQDNSelector::new(vec![DNSPattern::parse("*.example.com")]);
        let ips = sel.resolve_ips(&cache);
        assert_eq!(ips, vec!["1.2.3.4".parse::<IpAddr>().unwrap()]);
    }

    #[test]
    fn test_name_manager() {
        let mut nm = NameManager::new();
        nm.add_selector(FQDNSelector::new(vec![DNSPattern::parse("*.svc.local")]));
        nm.update_dns(DNSCacheEntry::new(
            FQDN::new("redis.svc.local"),
            vec!["10.0.0.5".parse().unwrap()],
            Duration::from_mins(1),
        ));
        assert_eq!(nm.selector_count(), 1);
        let sel = FQDNSelector::new(vec![DNSPattern::parse("*.svc.local")]);
        assert_eq!(nm.ips_for_selector(&sel).len(), 1);
    }
}
