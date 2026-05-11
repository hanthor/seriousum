//! L4 policy (port and protocol) handling
//!
//! Represents allowed/denied traffic on specific ports and protocols.

use std::collections::HashMap;

use crate::{PolicyError, Result};

/// Protocols supported in policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    TCP,
    UDP,
    ICMP,
    ICMPv6,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TCP => write!(f, "TCP"),
            Self::UDP => write!(f, "UDP"),
            Self::ICMP => write!(f, "ICMP"),
            Self::ICMPv6 => write!(f, "ICMPv6"),
        }
    }
}

impl Protocol {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_uppercase().as_str() {
            "TCP" => Some(Self::TCP),
            "UDP" => Some(Self::UDP),
            "ICMP" => Some(Self::ICMP),
            "ICMPV6" => Some(Self::ICMPv6),
            _ => None,
        }
    }
}

/// L4 traffic specification: protocol + port range
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct L4Traffic {
    pub protocol: Protocol,
    pub port_start: u16,
    pub port_end: u16,
}

impl L4Traffic {
    pub fn new(protocol: Protocol, port: u16) -> Self {
        Self {
            protocol,
            port_start: port,
            port_end: port,
        }
    }

    pub fn range(protocol: Protocol, start: u16, end: u16) -> Result<Self> {
        if start > end {
            return Err(PolicyError::InvalidL4Policy(
                format!("port range invalid: {start} > {end}"),
            ));
        }
        Ok(Self {
            protocol,
            port_start: start,
            port_end: end,
        })
    }

    pub fn matches(&self, protocol: Protocol, port: u16) -> bool {
        self.protocol == protocol && port >= self.port_start && port <= self.port_end
    }
}

/// L4 selector: matches traffic by protocol and port
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L4Selector {
    pub protocol: Protocol,
    pub port_start: u16,
    pub port_end: u16,
}

impl L4Selector {
    pub fn new(protocol: Protocol, port: u16) -> Self {
        Self {
            protocol,
            port_start: port,
            port_end: port,
        }
    }

    pub fn matches(&self, traffic: &L4Traffic) -> bool {
        self.protocol == traffic.protocol
            && traffic.port_start >= self.port_start
            && traffic.port_end <= self.port_end
    }
}

/// L4 policy: allowed/denied L4 traffic combinations
#[derive(Debug, Clone, Default)]
pub struct L4Policy {
    /// Allowed L4 traffic (empty = deny all)
    pub allowed: Vec<L4Traffic>,
    /// Whether to redirect to L7 proxy
    pub proxy_required: bool,
    /// Per-traffic L7 rules (for HTTP, DNS, etc.)
    pub l7_rules: HashMap<String, Vec<String>>,
}

impl L4Policy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_all() -> Self {
        Self {
            allowed: vec![
                L4Traffic::range(Protocol::TCP, 0, u16::MAX).unwrap(),
                L4Traffic::range(Protocol::UDP, 0, u16::MAX).unwrap(),
                L4Traffic::new(Protocol::ICMP, 0),
            ],
            proxy_required: false,
            l7_rules: HashMap::new(),
        }
    }

    pub fn deny_all() -> Self {
        Self {
            allowed: vec![],
            proxy_required: false,
            l7_rules: HashMap::new(),
        }
    }

    pub fn add_allowed(&mut self, traffic: L4Traffic) {
        self.allowed.push(traffic);
    }

    pub fn allows(&self, traffic: &L4Traffic) -> bool {
        self.allowed.iter().any(|allowed| {
            allowed.protocol == traffic.protocol
                && traffic.port_start >= allowed.port_start
                && traffic.port_end <= allowed.port_end
        })
    }

    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    pub fn requires_proxy(&self) -> bool {
        self.proxy_required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_from_name() {
        assert_eq!(Protocol::from_name("tcp"), Some(Protocol::TCP));
        assert_eq!(Protocol::from_name("UDP"), Some(Protocol::UDP));
        assert_eq!(Protocol::from_name("unknown"), None);
    }

    #[test]
    fn test_l4_traffic_new() {
        let traffic = L4Traffic::new(Protocol::TCP, 80);
        assert_eq!(traffic.protocol, Protocol::TCP);
        assert_eq!(traffic.port_start, 80);
        assert_eq!(traffic.port_end, 80);
    }

    #[test]
    fn test_l4_traffic_range() {
        let traffic = L4Traffic::range(Protocol::TCP, 8000, 9000).unwrap();
        assert!(traffic.matches(Protocol::TCP, 8500));
        assert!(!traffic.matches(Protocol::TCP, 7999));
        assert!(!traffic.matches(Protocol::UDP, 8500));
    }

    #[test]
    fn test_l4_traffic_range_invalid() {
        assert!(L4Traffic::range(Protocol::TCP, 9000, 8000).is_err());
    }

    #[test]
    fn test_l4_selector_matches() {
        let selector = L4Selector::new(Protocol::TCP, 80);
        let traffic = L4Traffic::new(Protocol::TCP, 80);
        assert!(selector.matches(&traffic));

        let traffic2 = L4Traffic::new(Protocol::UDP, 80);
        assert!(!selector.matches(&traffic2));
    }

    #[test]
    fn test_l4_policy_allow_all() {
        let policy = L4Policy::allow_all();
        assert!(!policy.is_empty());
        assert!(policy.allows(&L4Traffic::new(Protocol::TCP, 80)));
        assert!(policy.allows(&L4Traffic::new(Protocol::UDP, 53)));
    }

    #[test]
    fn test_l4_policy_deny_all() {
        let policy = L4Policy::deny_all();
        assert!(policy.is_empty());
        assert!(!policy.allows(&L4Traffic::new(Protocol::TCP, 80)));
    }

    #[test]
    fn test_l4_policy_add_allowed() {
        let mut policy = L4Policy::deny_all();
        policy.add_allowed(L4Traffic::new(Protocol::TCP, 80));
        assert!(policy.allows(&L4Traffic::new(Protocol::TCP, 80)));
        assert!(!policy.allows(&L4Traffic::new(Protocol::TCP, 443)));
    }

    #[test]
    fn test_l4_policy_proxy_required() {
        let mut policy = L4Policy::new();
        assert!(!policy.requires_proxy());
        policy.proxy_required = true;
        assert!(policy.requires_proxy());
    }
}
