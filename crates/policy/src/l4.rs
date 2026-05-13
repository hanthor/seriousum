//! L4 policy helpers.

use serde::{Deserialize, Serialize};

use crate::{PolicyError, Result};

/// Transport or network protocol used by an L4 rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Protocol {
    /// Wildcard protocol.
    Any,
    /// TCP traffic.
    TCP,
    /// UDP traffic.
    UDP,
    /// SCTP traffic.
    SCTP,
    /// ICMP traffic.
    ICMP,
    /// ICMPv6 traffic.
    ICMPv6,
}

impl Protocol {
    /// Converts a protocol name into a protocol value.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "ANY" => Some(Self::Any),
            "TCP" => Some(Self::TCP),
            "UDP" => Some(Self::UDP),
            "SCTP" => Some(Self::SCTP),
            "ICMP" => Some(Self::ICMP),
            "ICMPV6" => Some(Self::ICMPv6),
            _ => None,
        }
    }

    /// Returns the kernel protocol number for the protocol.
    #[must_use]
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Any => 0,
            Self::TCP => 6,
            Self::UDP => 17,
            Self::SCTP => 132,
            Self::ICMP => 1,
            Self::ICMPv6 => 58,
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Any => write!(f, "ANY"),
            Self::TCP => write!(f, "TCP"),
            Self::UDP => write!(f, "UDP"),
            Self::SCTP => write!(f, "SCTP"),
            Self::ICMP => write!(f, "ICMP"),
            Self::ICMPv6 => write!(f, "ICMPv6"),
        }
    }
}

/// A port and protocol range allowed by policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct L4Traffic {
    /// Protocol matched by the rule.
    pub protocol: Protocol,
    /// First allowed port in the range.
    pub port_start: u16,
    /// Last allowed port in the range.
    pub port_end: u16,
}

impl L4Traffic {
    /// Creates a rule for a single port.
    #[must_use]
    pub fn new(protocol: Protocol, port: u16) -> Self {
        Self {
            protocol,
            port_start: port,
            port_end: port,
        }
    }

    /// Creates a wildcard rule matching all protocols and ports.
    #[must_use]
    pub fn any() -> Self {
        Self {
            protocol: Protocol::Any,
            port_start: 0,
            port_end: u16::MAX,
        }
    }

    /// Creates a rule for a port range.
    pub fn range(protocol: Protocol, start: u16, end: u16) -> Result<Self> {
        if start > end {
            return Err(PolicyError::InvalidL4Policy(format!(
                "port range invalid: {start} > {end}"
            )));
        }

        Ok(Self {
            protocol,
            port_start: start,
            port_end: end,
        })
    }

    /// Returns true when this rule covers the provided protocol and port.
    #[must_use]
    pub fn matches(&self, protocol: Protocol, port: u16) -> bool {
        (self.protocol == Protocol::Any || self.protocol == protocol)
            && port >= self.port_start
            && port <= self.port_end
    }

    /// Returns true when this rule represents a wildcard L3-only rule.
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        self.protocol == Protocol::Any && self.port_start == 0 && self.port_end == u16::MAX
    }
}

/// Distilled L4 policy attached to a rule.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct L4Policy {
    /// Allowed protocol and port combinations.
    pub allowed: Vec<L4Traffic>,
    /// Whether matching traffic should be redirected to a proxy.
    pub proxy_required: bool,
    /// Whether matching traffic requires authentication.
    pub authentication_required: bool,
}

impl L4Policy {
    /// Creates an empty L4 policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an allow-all policy.
    #[must_use]
    pub fn allow_all() -> Self {
        Self {
            allowed: vec![L4Traffic::any()],
            proxy_required: false,
            authentication_required: false,
        }
    }

    /// Creates a deny-all policy.
    #[must_use]
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Adds an allowed L4 rule.
    pub fn add_allowed(&mut self, traffic: L4Traffic) {
        self.allowed.push(traffic);
    }

    /// Returns the rule entries, defaulting to a wildcard when no ports are given.
    #[must_use]
    pub fn entries(&self) -> Vec<L4Traffic> {
        if self.allowed.is_empty() {
            vec![L4Traffic::any()]
        } else {
            self.allowed.clone()
        }
    }

    /// Returns true when the policy allows the provided traffic.
    #[must_use]
    pub fn allows(&self, protocol: Protocol, port: u16) -> bool {
        self.entries()
            .iter()
            .any(|allowed| allowed.matches(protocol, port))
    }

    /// Returns true when the policy has no explicit entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_from_name_supports_any() {
        assert_eq!(Protocol::from_name("any"), Some(Protocol::Any));
        assert_eq!(Protocol::from_name("tcp"), Some(Protocol::TCP));
        assert_eq!(Protocol::from_name("bad"), None);
    }

    #[test]
    fn l4_range_rejects_inverted_port_ranges() {
        let result = L4Traffic::range(Protocol::TCP, 81, 80);
        assert!(result.is_err());
    }

    #[test]
    fn wildcard_policy_matches_any_port() {
        let policy = L4Policy::allow_all();
        assert!(policy.allows(Protocol::TCP, 80));
        assert!(policy.allows(Protocol::UDP, 53));
    }
}
