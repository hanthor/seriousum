//! Core types for FQDN DNS proxy

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Represents a DNS name to IP address mapping
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NameToIp {
    /// DNS name (may be unqualified, e.g., "myservice.namespace")
    pub name: String,

    /// IP address
    pub ip: IpAddr,

    /// TTL in seconds
    pub ttl: u32,
}

impl NameToIp {
    /// Creates a new name-to-IP mapping
    pub fn new(name: impl Into<String>, ip: IpAddr, ttl: u32) -> Self {
        Self {
            name: name.into(),
            ip,
            ttl,
        }
    }
}

/// IP CIDR block representation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IpCidr {
    /// CIDR network
    pub network: IpNet,
}

impl IpCidr {
    /// Creates a new CIDR block
    pub fn new(network: IpNet) -> Self {
        Self { network }
    }

    /// Creates from IPv4 network
    pub fn ipv4(net: Ipv4Net) -> Self {
        Self {
            network: IpNet::V4(net),
        }
    }

    /// Creates from IPv6 network
    pub fn ipv6(net: Ipv6Net) -> Self {
        Self {
            network: IpNet::V6(net),
        }
    }

    /// Checks if an IP address is in this CIDR block
    pub fn contains(&self, ip: IpAddr) -> bool {
        self.network.contains(&ip)
    }
}

impl std::fmt::Display for IpCidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.network)
    }
}

/// FQDN selector for policy matching
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FqdnSelector {
    /// Regular expression pattern for FQDN matching
    pub pattern: String,

    /// Whether to match subdomains
    pub match_subdomains: bool,
}

impl std::fmt::Display for FqdnSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

impl FqdnSelector {
    /// Creates a new FQDN selector
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            match_subdomains: false,
        }
    }

    /// Sets subdomain matching
    pub fn with_subdomains(mut self, match_subdomains: bool) -> Self {
        self.match_subdomains = match_subdomains;
        self
    }

    /// Normalizes FQDN (lowercase, adds trailing dot if needed)
    pub fn normalize_fqdn(fqdn: &str) -> String {
        let lower = fqdn.to_lowercase();
        if lower.ends_with('.') {
            lower
        } else {
            format!("{lower}.")
        }
    }

    /// Checks if FQDN matches this selector
    pub fn matches(&self, fqdn: &str) -> bool {
        let fqdn_normalized = Self::normalize_fqdn(fqdn);
        let pattern_normalized = Self::normalize_fqdn(&self.pattern);

        // Exact match
        if fqdn_normalized == pattern_normalized {
            return true;
        }

        // Wildcard matching (basic)
        if let Some(suffix) = pattern_normalized.strip_prefix("*.") {
            return fqdn_normalized.ends_with(suffix) && fqdn_normalized != suffix;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn name_to_ip_creation() {
        let mapping = NameToIp::new("example.com", "192.0.2.1".parse().unwrap(), 300);
        assert_eq!(mapping.name, "example.com");
        assert_eq!(mapping.ttl, 300);
    }

    #[test]
    fn ip_cidr_ipv4() {
        let cidr = IpCidr::ipv4(Ipv4Net::from_str("10.0.0.0/8").unwrap());
        let ip: IpAddr = "10.1.2.3".parse().unwrap();
        assert!(cidr.contains(ip));
    }

    #[test]
    fn ip_cidr_ipv6() {
        let cidr = IpCidr::ipv6(Ipv6Net::from_str("2001:db8::/32").unwrap());
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(cidr.contains(ip));
    }

    #[test]
    fn fqdn_selector_exact_match() {
        let selector = FqdnSelector::new("example.com");
        assert!(selector.matches("example.com"));
        assert!(selector.matches("EXAMPLE.COM"));
        assert!(!selector.matches("sub.example.com"));
    }

    #[test]
    fn fqdn_selector_wildcard() {
        let selector = FqdnSelector::new("*.example.com");
        assert!(selector.matches("sub.example.com"));
        assert!(selector.matches("deep.sub.example.com"));
        assert!(!selector.matches("example.com"));
    }

    #[test]
    fn fqdn_normalize() {
        assert_eq!(FqdnSelector::normalize_fqdn("example.com"), "example.com.");
        assert_eq!(FqdnSelector::normalize_fqdn("EXAMPLE.COM"), "example.com.");
        assert_eq!(FqdnSelector::normalize_fqdn("example.com."), "example.com.");
    }

    #[test]
    fn fqdn_selector_display() {
        let selector = FqdnSelector::new("*.example.com");
        assert_eq!(selector.to_string(), "*.example.com");
    }
}
