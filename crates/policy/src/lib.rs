//! Cilium policy engine — ported from cilium/pkg/policy
//!
//! This module provides the policy distillery: parsing, repository storage, and
//! per-endpoint policy compilation to eBPF maps.

pub mod error;
pub mod l4;
pub mod mapstate;
pub mod repository;
pub mod rule;
pub mod selector;

pub use error::{PolicyError, Result};
pub use l4::{L4Policy, L4Selector, L4Traffic};
pub use mapstate::{MapState, MapStateEntry, PolicyVerdict};
pub use repository::PolicyRepository;
pub use rule::{PolicyRule, RuleOrigin};
pub use selector::{EndpointSelector, Selector};

/// Traffic direction for policy rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficDirection {
    Ingress,
    Egress,
}

impl std::fmt::Display for TrafficDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ingress => write!(f, "ingress"),
            Self::Egress => write!(f, "egress"),
        }
    }
}

/// Policy verdict: allow, deny, or redirect to L7 proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
    Redirect, // to L7 proxy
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow => write!(f, "ALLOW"),
            Self::Deny => write!(f, "DENY"),
            Self::Redirect => write!(f, "REDIRECT"),
        }
    }
}

/// Endpoint identity for policy resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EndpointIdentity {
    pub id: u32,
}

impl EndpointIdentity {
    pub const WORLD: Self = Self { id: 1 };
    pub const HOST: Self = Self { id: 2 };
    pub const UNMANAGED: Self = Self { id: 3 };
    pub const HEALTH: Self = Self { id: 4 };
    pub const INIT: Self = Self { id: 5 };
    pub const LOCAL_NODE: Self = Self { id: 6 };
    pub const REMOTE_NODE: Self = Self { id: 7 };

    pub fn new(id: u32) -> Self {
        Self { id }
    }

    pub fn is_reserved(self) -> bool {
        self.id < 100
    }
}

impl std::fmt::Display for EndpointIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traffic_direction_display() {
        assert_eq!(TrafficDirection::Ingress.to_string(), "ingress");
        assert_eq!(TrafficDirection::Egress.to_string(), "egress");
    }

    #[test]
    fn test_verdict_display() {
        assert_eq!(Verdict::Allow.to_string(), "ALLOW");
        assert_eq!(Verdict::Deny.to_string(), "DENY");
        assert_eq!(Verdict::Redirect.to_string(), "REDIRECT");
    }

    #[test]
    fn test_endpoint_identity_reserved() {
        assert!(EndpointIdentity::WORLD.is_reserved());
        assert!(EndpointIdentity::HOST.is_reserved());
        assert!(!EndpointIdentity::new(200).is_reserved());
    }

    #[test]
    fn test_endpoint_identity_constants() {
        assert_eq!(EndpointIdentity::WORLD.id, 1);
        assert_eq!(EndpointIdentity::HOST.id, 2);
        assert_eq!(EndpointIdentity::LOCAL_NODE.id, 6);
    }
}
