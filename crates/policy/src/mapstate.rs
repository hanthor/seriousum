//! MapState: compiled policy state for eBPF maps
//!
//! MapState represents the per-endpoint policy compiled into a form suitable
//! for eBPF map storage. It tracks allowed/denied identities, ports, and directions.

use std::collections::{HashMap, HashSet};

use crate::{EndpointIdentity, Result};

/// A single entry in the policy map
#[derive(Debug, Clone)]
pub struct MapStateEntry {
    pub verdict: PolicyVerdict,
    pub identity: EndpointIdentity,
    pub port: u16,
    pub protocol: u8, // IPPROTO_TCP, IPPROTO_UDP, etc.
}

impl MapStateEntry {
    pub fn new(verdict: PolicyVerdict, identity: EndpointIdentity, port: u16, protocol: u8) -> Self {
        Self {
            verdict,
            identity,
            port,
            protocol,
        }
    }
}

/// Policy verdict entry type for map state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// Allow traffic
    Allow,
    /// Deny traffic (drop)
    Deny,
    /// Redirect to L7 proxy
    Redirect,
}

impl std::fmt::Display for PolicyVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow => write!(f, "ALLOW"),
            Self::Deny => write!(f, "DENY"),
            Self::Redirect => write!(f, "REDIRECT"),
        }
    }
}

/// Compiled policy state per endpoint
#[derive(Debug, Clone, Default)]
pub struct MapState {
    /// Ingress policies: (identity, port, protocol) -> verdict
    ingress: HashMap<(EndpointIdentity, u16, u8), PolicyVerdict>,
    /// Egress policies: (identity, port, protocol) -> verdict
    egress: HashMap<(EndpointIdentity, u16, u8), PolicyVerdict>,
    /// Identities allowed at ingress
    allowed_ingress_identities: HashSet<EndpointIdentity>,
    /// Identities allowed at egress
    allowed_egress_identities: HashSet<EndpointIdentity>,
}

impl MapState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an ingress policy entry
    pub fn add_ingress(
        &mut self,
        identity: EndpointIdentity,
        port: u16,
        protocol: u8,
        verdict: PolicyVerdict,
    ) -> Result<()> {
        self.ingress.insert((identity, port, protocol), verdict);
        if verdict == PolicyVerdict::Allow {
            self.allowed_ingress_identities.insert(identity);
        }
        Ok(())
    }

    /// Add an egress policy entry
    pub fn add_egress(
        &mut self,
        identity: EndpointIdentity,
        port: u16,
        protocol: u8,
        verdict: PolicyVerdict,
    ) -> Result<()> {
        self.egress.insert((identity, port, protocol), verdict);
        if verdict == PolicyVerdict::Allow {
            self.allowed_egress_identities.insert(identity);
        }
        Ok(())
    }

    /// Look up an ingress policy
    pub fn lookup_ingress(&self, identity: EndpointIdentity, port: u16, protocol: u8) -> Option<PolicyVerdict> {
        self.ingress.get(&(identity, port, protocol)).copied()
    }

    /// Look up an egress policy
    pub fn lookup_egress(&self, identity: EndpointIdentity, port: u16, protocol: u8) -> Option<PolicyVerdict> {
        self.egress.get(&(identity, port, protocol)).copied()
    }

    /// Get all ingress entries
    pub fn ingress_entries(&self) -> Vec<MapStateEntry> {
        self.ingress
            .iter()
            .map(|((identity, port, protocol), verdict)| MapStateEntry {
                verdict: *verdict,
                identity: *identity,
                port: *port,
                protocol: *protocol,
            })
            .collect()
    }

    /// Get all egress entries
    pub fn egress_entries(&self) -> Vec<MapStateEntry> {
        self.egress
            .iter()
            .map(|((identity, port, protocol), verdict)| MapStateEntry {
                verdict: *verdict,
                identity: *identity,
                port: *port,
                protocol: *protocol,
            })
            .collect()
    }

    /// Clear all policies
    pub fn clear(&mut self) {
        self.ingress.clear();
        self.egress.clear();
        self.allowed_ingress_identities.clear();
        self.allowed_egress_identities.clear();
    }

    /// Get count of ingress entries
    pub fn ingress_len(&self) -> usize {
        self.ingress.len()
    }

    /// Get count of egress entries
    pub fn egress_len(&self) -> usize {
        self.egress.len()
    }

    /// Check if ingress is empty
    pub fn is_ingress_empty(&self) -> bool {
        self.ingress.is_empty()
    }

    /// Check if egress is empty
    pub fn is_egress_empty(&self) -> bool {
        self.egress.is_empty()
    }

    /// Get allowed ingress identities
    pub fn allowed_ingress_identities(&self) -> Vec<EndpointIdentity> {
        self.allowed_ingress_identities.iter().copied().collect()
    }

    /// Get allowed egress identities
    pub fn allowed_egress_identities(&self) -> Vec<EndpointIdentity> {
        self.allowed_egress_identities.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_state_new() {
        let ms = MapState::new();
        assert_eq!(ms.ingress_len(), 0);
        assert_eq!(ms.egress_len(), 0);
        assert!(ms.is_ingress_empty());
        assert!(ms.is_egress_empty());
    }

    #[test]
    fn test_map_state_add_ingress() {
        let mut ms = MapState::new();
        let identity = EndpointIdentity::new(42);

        ms.add_ingress(identity, 80, 6, PolicyVerdict::Allow).unwrap();
        assert_eq!(ms.ingress_len(), 1);
        assert_eq!(ms.lookup_ingress(identity, 80, 6), Some(PolicyVerdict::Allow));
        assert_eq!(ms.lookup_ingress(identity, 443, 6), None);
    }

    #[test]
    fn test_map_state_add_egress() {
        let mut ms = MapState::new();
        let identity = EndpointIdentity::new(100);

        ms.add_egress(identity, 53, 17, PolicyVerdict::Allow).unwrap();
        assert_eq!(ms.egress_len(), 1);
        assert_eq!(ms.lookup_egress(identity, 53, 17), Some(PolicyVerdict::Allow));
    }

    #[test]
    fn test_map_state_mixed_ingress_egress() {
        let mut ms = MapState::new();
        let identity = EndpointIdentity::new(42);

        ms.add_ingress(identity, 80, 6, PolicyVerdict::Allow).unwrap();
        ms.add_egress(identity, 443, 6, PolicyVerdict::Allow).unwrap();

        assert_eq!(ms.ingress_len(), 1);
        assert_eq!(ms.egress_len(), 1);
    }

    #[test]
    fn test_map_state_allow_tracking() {
        let mut ms = MapState::new();
        let id1 = EndpointIdentity::new(1);
        let id2 = EndpointIdentity::new(2);

        ms.add_ingress(id1, 80, 6, PolicyVerdict::Allow).unwrap();
        ms.add_ingress(id2, 443, 6, PolicyVerdict::Deny).unwrap();

        let allowed = ms.allowed_ingress_identities();
        assert!(allowed.contains(&id1));
        assert!(!allowed.contains(&id2));
    }

    #[test]
    fn test_map_state_entries() {
        let mut ms = MapState::new();
        let identity = EndpointIdentity::new(42);

        ms.add_ingress(identity, 80, 6, PolicyVerdict::Allow).unwrap();
        ms.add_ingress(identity, 443, 6, PolicyVerdict::Allow).unwrap();

        let entries = ms.ingress_entries();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.identity == identity));
    }

    #[test]
    fn test_map_state_clear() {
        let mut ms = MapState::new();
        let identity = EndpointIdentity::new(42);

        ms.add_ingress(identity, 80, 6, PolicyVerdict::Allow).unwrap();
        ms.add_egress(identity, 443, 6, PolicyVerdict::Allow).unwrap();

        assert_eq!(ms.ingress_len(), 1);
        assert_eq!(ms.egress_len(), 1);

        ms.clear();
        assert_eq!(ms.ingress_len(), 0);
        assert_eq!(ms.egress_len(), 0);
    }

    #[test]
    fn test_policy_verdict_display() {
        assert_eq!(PolicyVerdict::Allow.to_string(), "ALLOW");
        assert_eq!(PolicyVerdict::Deny.to_string(), "DENY");
        assert_eq!(PolicyVerdict::Redirect.to_string(), "REDIRECT");
    }
}
