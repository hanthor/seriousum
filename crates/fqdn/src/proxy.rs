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
            allowed.iter().any(|selector| selector.matches(fqdn))
        } else {
            true
        }
    }

    /// Adds a DNS lookup to the cache
    pub fn cache_lookup(&self, name: impl Into<String>, ips: &[IpAddr], ttl: u32) -> Result<()> {
        self.cache.update(name, ips, ttl)?;
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

        debug!("Recorded query for {domain}");
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
    pub fn start(&self) -> Result<()> {
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

        proxy.cache_lookup("example.com", &ips, 300).unwrap();
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
            .cache_lookup("example.com", &["192.0.2.1".parse().unwrap()], 300)
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

    #[test]
    fn dns_proxy_start() {
        let proxy = DnsProxy::new();
        let result = proxy.start();
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod parity_tests {
    //! Parity tests ported from `cilium/pkg/fqdn/dnsproxy/helpers_test.go`.
    //!
    //! Implemented here:
    //! - `GeneratePattern` pure-string logic from `pkg/fqdn/dnsproxy/proxy.go`.
    //! - `setPortRulesForID` and `setPortRulesForIDFromUnifiedFormat` via a
    //!   minimal in-memory model of Cilium's selector and regex-cache wiring.

    use crate::types::FqdnSelector;
    use regex::Regex;
    use std::{collections::BTreeMap, sync::Arc};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct PortRuleDns {
        match_name: String,
        match_pattern: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct SelectorKey(&'static str);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct PortProto {
        port: u16,
        protocol: &'static str,
    }

    type ExplicitPortRules = BTreeMap<SelectorKey, Vec<PortRuleDns>>;
    type UnifiedPortRules = BTreeMap<SelectorKey, Arc<Regex>>;
    type CachedSelectorRegexEntry = BTreeMap<SelectorKey, Arc<Regex>>;
    type PortProtoToSelectorAllow = BTreeMap<PortProto, CachedSelectorRegexEntry>;
    type PerEndpointAllow = BTreeMap<u64, PortProtoToSelectorAllow>;

    #[derive(Debug)]
    struct RegexCacheEntry {
        regex: Arc<Regex>,
        reference_count: usize,
    }

    #[derive(Debug, Default)]
    struct RegexCache {
        entries: BTreeMap<String, RegexCacheEntry>,
    }

    impl RegexCache {
        fn len(&self) -> usize {
            self.entries.len()
        }

        fn patterns(&self) -> BTreeMap<String, usize> {
            self.entries
                .iter()
                .map(|(pattern, entry)| (pattern.clone(), entry.reference_count))
                .collect()
        }

        fn lookup_or_compile_regex(&mut self, pattern: &str) -> Result<Arc<Regex>, regex::Error> {
            if let Some(entry) = self.entries.get_mut(pattern) {
                entry.reference_count += 1;
                return Ok(entry.regex.clone());
            }

            let regex = Arc::new(Regex::new(pattern)?);
            self.entries.insert(
                pattern.to_string(),
                RegexCacheEntry {
                    regex: regex.clone(),
                    reference_count: 1,
                },
            );
            Ok(regex)
        }

        fn lookup_or_insert_regex(&mut self, regex: Arc<Regex>) -> Arc<Regex> {
            let pattern = regex.as_str().to_string();
            if let Some(entry) = self.entries.get_mut(&pattern) {
                entry.reference_count += 1;
                return entry.regex.clone();
            }

            self.entries.insert(
                pattern,
                RegexCacheEntry {
                    regex: regex.clone(),
                    reference_count: 1,
                },
            );
            regex
        }

        fn release_regex(&mut self, regex: &Arc<Regex>) {
            let pattern = regex.as_str();
            let remove_pattern = match self.entries.get_mut(pattern) {
                Some(entry) if entry.reference_count == 1 => true,
                Some(entry) => {
                    entry.reference_count -= 1;
                    false
                }
                None => false,
            };
            if remove_pattern {
                self.entries.remove(pattern);
            }
        }
    }

    fn remove_and_release_port_rules_for_id(
        allow: &mut PerEndpointAllow,
        cache: &mut RegexCache,
        endpoint_id: u64,
        dest_port_proto: PortProto,
    ) {
        let mut remove_endpoint = false;
        if let Some(ep_port_protos) = allow.get_mut(&endpoint_id) {
            if let Some(existing_rules) = ep_port_protos.remove(&dest_port_proto) {
                for regex in existing_rules.values() {
                    cache.release_regex(regex);
                }
            }
            remove_endpoint = ep_port_protos.is_empty();
        }
        if remove_endpoint {
            allow.remove(&endpoint_id);
        }
    }

    fn set_port_rules_for_id(
        allow: &mut PerEndpointAllow,
        cache: &mut RegexCache,
        endpoint_id: u64,
        dest_port_proto: PortProto,
        new_rules: &ExplicitPortRules,
    ) -> Result<(), regex::Error> {
        if new_rules.is_empty() {
            remove_and_release_port_rules_for_id(allow, cache, endpoint_id, dest_port_proto);
            return Ok(());
        }

        let mut compiled_rules = CachedSelectorRegexEntry::new();
        for (selector, ruleset) in new_rules {
            let pattern = generate_pattern(ruleset);
            let regex = match cache.lookup_or_compile_regex(&pattern) {
                Ok(regex) => regex,
                Err(err) => {
                    for compiled in compiled_rules.values() {
                        cache.release_regex(compiled);
                    }
                    return Err(err);
                }
            };
            compiled_rules.insert(selector.clone(), regex);
        }

        remove_and_release_port_rules_for_id(allow, cache, endpoint_id, dest_port_proto);
        allow
            .entry(endpoint_id)
            .or_default()
            .insert(dest_port_proto, compiled_rules);
        Ok(())
    }

    fn set_port_rules_for_id_from_unified_format(
        allow: &mut PerEndpointAllow,
        cache: &mut RegexCache,
        endpoint_id: u64,
        dest_port_proto: PortProto,
        new_rules: &UnifiedPortRules,
    ) {
        if new_rules.is_empty() {
            remove_and_release_port_rules_for_id(allow, cache, endpoint_id, dest_port_proto);
            return;
        }

        let mut compiled_rules = CachedSelectorRegexEntry::new();
        for (selector, regex) in new_rules {
            compiled_rules.insert(
                selector.clone(),
                cache.lookup_or_insert_regex(regex.clone()),
            );
        }

        remove_and_release_port_rules_for_id(allow, cache, endpoint_id, dest_port_proto);
        allow
            .entry(endpoint_id)
            .or_default()
            .insert(dest_port_proto, compiled_rules);
    }

    fn compile_unified_rules(rules: &ExplicitPortRules) -> UnifiedPortRules {
        rules
            .iter()
            .map(|(selector, ruleset)| {
                let pattern = generate_pattern(ruleset);
                let regex = Arc::new(Regex::new(&pattern).expect("generated regex should compile"));
                (selector.clone(), regex)
            })
            .collect()
    }

    fn selector_patterns(
        allow: &PerEndpointAllow,
        endpoint_id: u64,
        dest_port_proto: PortProto,
    ) -> BTreeMap<String, String> {
        allow
            .get(&endpoint_id)
            .and_then(|port_rules| port_rules.get(&dest_port_proto))
            .map(|selectors| {
                selectors
                    .iter()
                    .map(|(selector, regex)| (selector.0.to_string(), regex.as_str().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn selector_regex(
        allow: &PerEndpointAllow,
        endpoint_id: u64,
        dest_port_proto: PortProto,
        selector: &SelectorKey,
    ) -> Arc<Regex> {
        allow
            .get(&endpoint_id)
            .and_then(|port_rules| port_rules.get(&dest_port_proto))
            .and_then(|selectors| selectors.get(selector))
            .cloned()
            .expect("selector should exist")
    }

    const MATCH_ALL_ANCHORED_PATTERN: &str = "(?:)";
    const MATCH_ALL_UNANCHORED_PATTERN: &str = ".*";
    const ALLOWED_DNS_CHARS_RE_GROUP: &str = "[-a-zA-Z0-9_]";
    const DNS_WILDCARD_RE_GROUP: &str = "([-a-zA-Z0-9_]+([.][-a-zA-Z0-9_]+){0,})[.]";

    fn is_dns_wildcard(pattern: &str) -> bool {
        let trimmed = pattern.trim_end_matches('.');
        !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '*')
    }

    fn fqdn(input: &str) -> String {
        FqdnSelector::normalize_fqdn(input)
    }

    fn sanitize(pattern: &str) -> String {
        if is_dns_wildcard(pattern) {
            pattern.to_string()
        } else {
            fqdn(pattern)
        }
    }

    fn replace_subdomain_wildcard_prefix(mut escaped: String) -> String {
        let stars = escaped.chars().take_while(|&ch| ch == '*').count();
        if stars >= 2 {
            let remainder = &escaped[stars..];
            if let Some(suffix) = remainder.strip_prefix("[.]") {
                escaped = format!("{DNS_WILDCARD_RE_GROUP}{suffix}");
            }
        }
        escaped
    }

    fn replace_wildcards(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '*' {
                while matches!(chars.peek(), Some('*')) {
                    chars.next();
                }
                out.push_str(ALLOWED_DNS_CHARS_RE_GROUP);
                out.push('*');
            } else {
                out.push(ch);
            }
        }

        out
    }

    fn to_unanchored_regexp(pattern: &str) -> String {
        let pattern = pattern.trim().to_lowercase();
        if is_dns_wildcard(&pattern) {
            return MATCH_ALL_UNANCHORED_PATTERN.to_string();
        }

        let escaped_dots = pattern.replace('.', "[.]");
        let with_subdomain_prefix = replace_subdomain_wildcard_prefix(escaped_dots);
        replace_wildcards(&with_subdomain_prefix)
    }

    fn generate_pattern(rules: &[PortRuleDns]) -> String {
        if rules.is_empty() {
            return MATCH_ALL_ANCHORED_PATTERN.to_string();
        }

        let mut re_strings = Vec::with_capacity(rules.len());
        for rule in rules {
            if !rule.match_name.is_empty() {
                let dns_rule_name = fqdn(&rule.match_name);
                re_strings.push(to_unanchored_regexp(&dns_rule_name));
            }
            if !rule.match_pattern.is_empty() {
                let dns_pattern = sanitize(&rule.match_pattern);
                let dns_pattern_re = to_unanchored_regexp(&dns_pattern);
                if dns_pattern_re == MATCH_ALL_UNANCHORED_PATTERN {
                    return MATCH_ALL_ANCHORED_PATTERN.to_string();
                }
                re_strings.push(dns_pattern_re);
            }
        }

        format!("^(?:{})$", re_strings.join("|"))
    }

    #[test]
    fn test_set_port_rules_for_id() {
        let endpoint_id = 1;
        let dest_port_proto = PortProto {
            port: 8053,
            protocol: "udp",
        };
        let selector_one = SelectorKey("selector-one");
        let selector_two = SelectorKey("selector-two");
        let selector_one_rules = vec![
            PortRuleDns {
                match_name: "cilium.io.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*.cilium.io.".into(),
            },
        ];
        let selector_two_rules = vec![
            PortRuleDns {
                match_name: "cilium2.io.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*.cilium2.io.".into(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*.cilium3.io.".into(),
            },
        ];
        let mut allow = PerEndpointAllow::new();
        let mut cache = RegexCache::default();
        let mut rules =
            ExplicitPortRules::from([(selector_one.clone(), selector_one_rules.clone())]);

        set_port_rules_for_id(&mut allow, &mut cache, endpoint_id, dest_port_proto, &rules)
            .expect("initial rules should compile");
        assert_eq!(cache.len(), 1);
        assert_eq!(
            selector_patterns(&allow, endpoint_id, dest_port_proto),
            BTreeMap::from([(
                String::from("selector-one"),
                generate_pattern(&selector_one_rules),
            )])
        );

        rules.insert(selector_two.clone(), selector_two_rules.clone());
        set_port_rules_for_id(&mut allow, &mut cache, endpoint_id, dest_port_proto, &rules)
            .expect("adding a selector should succeed");
        assert_eq!(cache.len(), 2);
        assert_eq!(
            selector_patterns(&allow, endpoint_id, dest_port_proto),
            BTreeMap::from([
                (
                    String::from("selector-one"),
                    generate_pattern(&selector_one_rules),
                ),
                (
                    String::from("selector-two"),
                    generate_pattern(&selector_two_rules),
                ),
            ])
        );

        rules.remove(&selector_two);
        set_port_rules_for_id(&mut allow, &mut cache, endpoint_id, dest_port_proto, &rules)
            .expect("removing a selector should succeed");
        assert_eq!(cache.len(), 1);
        assert_eq!(
            selector_patterns(&allow, endpoint_id, dest_port_proto),
            BTreeMap::from([(
                String::from("selector-one"),
                generate_pattern(&selector_one_rules),
            )])
        );

        set_port_rules_for_id(
            &mut allow,
            &mut cache,
            endpoint_id,
            dest_port_proto,
            &ExplicitPortRules::new(),
        )
        .expect("empty rules should clear state");
        assert!(allow.is_empty());
        assert!(cache.patterns().is_empty());

        rules.insert(
            selector_two,
            vec![
                PortRuleDns {
                    match_name: "cilium2.io.".into(),
                    match_pattern: String::new(),
                },
                PortRuleDns {
                    match_name: String::new(),
                    match_pattern: "*.cilium2.io.".into(),
                },
                PortRuleDns {
                    match_name: String::new(),
                    match_pattern: "-invalid-pattern(".into(),
                },
                PortRuleDns {
                    match_name: String::new(),
                    match_pattern: "*.cilium3.io.".into(),
                },
            ],
        );
        assert!(
            set_port_rules_for_id(&mut allow, &mut cache, endpoint_id, dest_port_proto, &rules)
                .is_err()
        );
        assert!(allow.is_empty());
        assert!(cache.patterns().is_empty());
    }

    #[test]
    fn test_set_port_rules_for_id_from_unified_format() {
        let endpoint_id = 1;
        let dest_port_proto = PortProto {
            port: 8053,
            protocol: "udp",
        };
        let selector_one = SelectorKey("selector-one");
        let selector_duplicate = SelectorKey("selector-duplicate");
        let selector_two = SelectorKey("selector-two");
        let shared_rules = vec![
            PortRuleDns {
                match_name: "cilium.io.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*.cilium.io.".into(),
            },
        ];
        let distinct_rules = vec![PortRuleDns {
            match_name: "sub.cilium.io.".into(),
            match_pattern: String::new(),
        }];
        let mut explicit_allow = PerEndpointAllow::new();
        let mut unified_allow = PerEndpointAllow::new();
        let mut explicit_cache = RegexCache::default();
        let mut unified_cache = RegexCache::default();
        let mut explicit_rules = ExplicitPortRules::from([
            (selector_one.clone(), shared_rules.clone()),
            (selector_duplicate.clone(), shared_rules.clone()),
        ]);

        set_port_rules_for_id(
            &mut explicit_allow,
            &mut explicit_cache,
            endpoint_id,
            dest_port_proto,
            &explicit_rules,
        )
        .expect("explicit rules should compile");
        let mut unified_rules = compile_unified_rules(&explicit_rules);
        set_port_rules_for_id_from_unified_format(
            &mut unified_allow,
            &mut unified_cache,
            endpoint_id,
            dest_port_proto,
            &unified_rules,
        );
        assert_eq!(
            selector_patterns(&explicit_allow, endpoint_id, dest_port_proto),
            selector_patterns(&unified_allow, endpoint_id, dest_port_proto)
        );
        assert_eq!(explicit_cache.len(), 1);
        assert_eq!(unified_cache.len(), 1);
        assert!(Arc::ptr_eq(
            &selector_regex(&explicit_allow, endpoint_id, dest_port_proto, &selector_one),
            &selector_regex(
                &explicit_allow,
                endpoint_id,
                dest_port_proto,
                &selector_duplicate,
            ),
        ));
        assert!(Arc::ptr_eq(
            &selector_regex(&unified_allow, endpoint_id, dest_port_proto, &selector_one),
            &selector_regex(
                &unified_allow,
                endpoint_id,
                dest_port_proto,
                &selector_duplicate,
            ),
        ));

        explicit_rules.insert(selector_two.clone(), distinct_rules.clone());
        unified_rules = compile_unified_rules(&explicit_rules);
        set_port_rules_for_id(
            &mut explicit_allow,
            &mut explicit_cache,
            endpoint_id,
            dest_port_proto,
            &explicit_rules,
        )
        .expect("updating explicit rules should succeed");
        set_port_rules_for_id_from_unified_format(
            &mut unified_allow,
            &mut unified_cache,
            endpoint_id,
            dest_port_proto,
            &unified_rules,
        );
        assert_eq!(
            selector_patterns(&explicit_allow, endpoint_id, dest_port_proto),
            selector_patterns(&unified_allow, endpoint_id, dest_port_proto)
        );
        assert_eq!(explicit_cache.len(), 2);
        assert_eq!(unified_cache.len(), 2);

        explicit_rules.remove(&selector_two);
        unified_rules = compile_unified_rules(&explicit_rules);
        set_port_rules_for_id(
            &mut explicit_allow,
            &mut explicit_cache,
            endpoint_id,
            dest_port_proto,
            &explicit_rules,
        )
        .expect("removing explicit selector should succeed");
        set_port_rules_for_id_from_unified_format(
            &mut unified_allow,
            &mut unified_cache,
            endpoint_id,
            dest_port_proto,
            &unified_rules,
        );
        assert_eq!(
            selector_patterns(&explicit_allow, endpoint_id, dest_port_proto),
            selector_patterns(&unified_allow, endpoint_id, dest_port_proto)
        );
        assert_eq!(explicit_cache.len(), 1);
        assert_eq!(unified_cache.len(), 1);

        set_port_rules_for_id(
            &mut explicit_allow,
            &mut explicit_cache,
            endpoint_id,
            dest_port_proto,
            &ExplicitPortRules::new(),
        )
        .expect("empty explicit rules should clear state");
        set_port_rules_for_id_from_unified_format(
            &mut unified_allow,
            &mut unified_cache,
            endpoint_id,
            dest_port_proto,
            &UnifiedPortRules::new(),
        );
        assert!(explicit_allow.is_empty());
        assert!(unified_allow.is_empty());
        assert!(explicit_cache.patterns().is_empty());
        assert!(unified_cache.patterns().is_empty());
    }

    #[test]
    fn test_generate_pattern() {
        let rules = vec![
            PortRuleDns {
                match_name: "example.name.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: "example.com.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: "demo.io.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: "demoo.tld.".into(),
                match_pattern: String::new(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*pattern.com".into(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*.*.*middle.*".into(),
            },
        ];
        let matching = [
            "example.name.",
            "example.com.",
            "demo.io.",
            "demoo.tld.",
            "testpattern.com.",
            "pattern.com.",
            "a.b.cmiddle.io.",
        ];
        let not_matching = [
            "eexample.name.",
            "eexample.com.",
            "vdemo.io.",
            "demo.ioo.",
            "emoo.tld.",
            "test.ppattern.com.",
            "b.cmiddle.io.",
        ];

        let pattern = generate_pattern(&rules);
        let regex = Regex::new(&pattern).expect("generated regex should compile");
        for fqdn in matching {
            assert!(
                regex.is_match(fqdn),
                "expected fqdn {fqdn:?} to match, but it did not"
            );
        }
        for fqdn in not_matching {
            assert!(
                !regex.is_match(fqdn),
                "expected fqdn {fqdn:?} to not match, but it did"
            );
        }

        let wildcard_pattern = generate_pattern(&[
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "domo.io.".into(),
            },
            PortRuleDns {
                match_name: String::new(),
                match_pattern: "*".into(),
            },
        ]);
        let wildcard_regex =
            Regex::new(&wildcard_pattern).expect("wildcard policy regex should compile");
        for fqdn in matching.iter().chain(not_matching.iter()) {
            assert!(
                wildcard_regex.is_match(fqdn),
                "expected fqdn {fqdn:?} to match with wildcard policy, but it did not"
            );
        }

        let no_rules_pattern = generate_pattern(&[]);
        let no_rules_regex =
            Regex::new(&no_rules_pattern).expect("empty rules regex should compile");
        for fqdn in matching.iter().chain(not_matching.iter()) {
            assert!(
                no_rules_regex.is_match(fqdn),
                "expected fqdn {fqdn:?} to match with no DNS rules, but it did not"
            );
        }
    }

    #[test]
    fn test_generate_pattern_trailing_dot() {
        let dns_name = "example.name";
        let dns_pattern = "*.example.name";

        let generate = |name: &str, pattern: &str| {
            generate_pattern(&[
                PortRuleDns {
                    match_name: name.to_string(),
                    match_pattern: String::new(),
                },
                PortRuleDns {
                    match_name: String::new(),
                    match_pattern: pattern.to_string(),
                },
            ])
        };

        assert_eq!(
            generate(&fqdn(dns_pattern), &fqdn(dns_name)),
            generate(dns_pattern, dns_name)
        );
    }
}
