//! DNS proxy server for intercepting and policy-enforcing DNS queries
//!
//! Listens on port 53 for DNS queries, enforces FQDN-based policies,
//! and caches results for performance.

use crate::cache::DnsCache;
use crate::error::Result;
use crate::policy::{FqdnPolicy, FqdnPolicyRepository};
use crate::types::FqdnSelector;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tracing::{debug, info};

/// Configuration for the DNS proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsProxyConfig {
    /// Listening address (default: 127.0.0.1)
    pub listen_addr: String,

    /// Listening port (default: 53)
    pub listen_port: u16,

    /// Upstream DNS servers to forward queries to
    pub upstream_servers: Vec<SocketAddr>,

    /// Enable response caching
    pub enable_caching: bool,

    /// DNS cache minimum TTL (seconds)
    pub min_cache_ttl: u32,

    /// Maximum IPs per hostname in cache
    pub cache_per_host_limit: usize,

    /// Enable DNS compression in responses
    pub enable_compression: bool,

    /// Query timeout (seconds)
    pub query_timeout: u64,
}

impl Default for DnsProxyConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 53,
            upstream_servers: vec!["8.8.8.8:53".parse().unwrap()],
            enable_caching: true,
            min_cache_ttl: 60,
            cache_per_host_limit: 0,
            enable_compression: false,
            query_timeout: 10,
        }
    }
}

/// DNS proxy server
pub struct DnsProxy {
    config: DnsProxyConfig,

    /// DNS cache for storing lookup results
    cache: Arc<DnsCache>,

    /// FQDN policy repository
    policy_repo: Arc<FqdnPolicyRepository>,

    /// Allowed FQDN patterns per endpoint
    allowed_fqdns: Arc<DashMap<String, Vec<FqdnSelector>>>,

    /// Statistics
    query_count: Arc<DashMap<String, u64>>,
}

impl DnsProxy {
    /// Creates a new DNS proxy with default configuration
    pub fn new() -> Self {
        Self::with_config(DnsProxyConfig::default())
    }

    /// Creates a new DNS proxy with custom configuration
    pub fn with_config(config: DnsProxyConfig) -> Self {
        let cache = DnsCache::with_limits(config.min_cache_ttl, config.cache_per_host_limit);

        Self {
            config,
            cache: Arc::new(cache),
            policy_repo: Arc::new(FqdnPolicyRepository::new()),
            allowed_fqdns: Arc::new(DashMap::new()),
            query_count: Arc::new(DashMap::new()),
        }
    }

    /// Registers a policy in the proxy
    pub fn register_policy(&self, policy: FqdnPolicy) {
        self.policy_repo.add_policy(policy);
    }

    /// Sets allowed FQDNs for an endpoint
    pub fn set_allowed_fqdns(&self, endpoint_id: impl Into<String>, fqdns: Vec<FqdnSelector>) {
        self.allowed_fqdns.insert(endpoint_id.into(), fqdns);
    }

    /// Checks if a query would be allowed by policy
    pub fn is_query_allowed(&self, endpoint_id: &str, fqdn: &str) -> bool {
        // If no policy configured for endpoint, allow everything
        if let Some(allowed) = self.allowed_fqdns.get(endpoint_id) {
            allowed
                .iter()
                .any(|selector| selector.matches(fqdn))
        } else {
            true
        }
    }

    /// Adds a DNS lookup to the cache
    pub fn cache_lookup(
        &self,
        name: impl Into<String>,
        ips: Vec<IpAddr>,
        ttl: u32,
    ) -> Result<()> {
        self.cache.update(name, &ips, ttl)?;
        Ok(())
    }

    /// Gets cached result for a domain
    pub fn lookup_cached(&self, domain: &str) -> Option<Vec<IpAddr>> {
        self.cache.lookup(domain)
    }

    /// Records a query for statistics
    pub fn record_query(&self, domain: impl Into<String>) {
        let domain = domain.into();
        self.query_count
            .entry(domain.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);

        debug!("Recorded query for {}", domain);
    }

    /// Gets query statistics
    pub fn get_query_stats(&self, domain: &str) -> Option<u64> {
        self.query_count.get(domain).map(|r| *r)
    }

    /// Clears the cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Cleans up expired cache entries
    pub fn cleanup_expired_cache(&self) -> usize {
        self.cache.cleanup_expired()
    }

    /// Gets cache size
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Gets configuration reference
    pub fn config(&self) -> &DnsProxyConfig {
        &self.config
    }

    /// Gets cache reference
    pub fn cache(&self) -> &DnsCache {
        &self.cache
    }

    /// Gets policy repository reference
    pub fn policy_repo(&self) -> &FqdnPolicyRepository {
        &self.policy_repo
    }

    /// Simulates starting the DNS proxy listener (no actual UDP binding)
    pub async fn start(&self) -> Result<()> {
        info!(
            "DNS proxy starting on {}:{}",
            &self.config.listen_addr, self.config.listen_port
        );

        debug!("DNS proxy initialized");

        Ok(())
    }
}

impl Default for DnsProxy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_proxy_creation() {
        let proxy = DnsProxy::new();
        assert_eq!(proxy.config.listen_port, 53);
        assert!(proxy.cache_size() == 0);
    }

    #[test]
    fn dns_proxy_cache_lookup() {
        let proxy = DnsProxy::new();
        let ips = vec!["192.0.2.1".parse().unwrap()];

        proxy.cache_lookup("example.com", ips.clone(), 300).unwrap();
        let result = proxy.lookup_cached("example.com");

        assert_eq!(result, Some(ips));
    }

    #[test]
    fn dns_proxy_policy_allowed() {
        let proxy = DnsProxy::new();
        let selector = FqdnSelector::new("*.example.com");

        proxy.set_allowed_fqdns("ep1", vec![selector]);

        assert!(proxy.is_query_allowed("ep1", "sub.example.com"));
        assert!(!proxy.is_query_allowed("ep1", "example.org"));
    }

    #[test]
    fn dns_proxy_no_policy_allows_all() {
        let proxy = DnsProxy::new();

        // No policy set for endpoint
        assert!(proxy.is_query_allowed("ep1", "any.domain.com"));
    }

    #[test]
    fn dns_proxy_query_stats() {
        let proxy = DnsProxy::new();

        proxy.record_query("example.com");
        proxy.record_query("example.com");
        proxy.record_query("example.org");

        assert_eq!(proxy.get_query_stats("example.com"), Some(2));
        assert_eq!(proxy.get_query_stats("example.org"), Some(1));
    }

    #[test]
    fn dns_proxy_cache_clear() {
        let proxy = DnsProxy::new();

        proxy
            .cache_lookup("example.com", vec!["192.0.2.1".parse().unwrap()], 300)
            .unwrap();
        assert!(proxy.cache_size() > 0);

        proxy.clear_cache();
        assert_eq!(proxy.cache_size(), 0);
    }

    #[test]
    fn dns_proxy_config_custom() {
        let mut config = DnsProxyConfig::default();
        config.listen_port = 5353;
        config.min_cache_ttl = 120;

        let proxy = DnsProxy::with_config(config);

        assert_eq!(proxy.config().listen_port, 5353);
        assert_eq!(proxy.config().min_cache_ttl, 120);
    }

    #[tokio::test]
    async fn dns_proxy_start() {
        let proxy = DnsProxy::new();
        let result = proxy.start().await;
        assert!(result.is_ok());
    }
}
