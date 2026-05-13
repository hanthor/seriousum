//! Cilium policy engine core types ported to Rust.

pub mod cidr;
pub mod error;
pub mod l4;
pub mod mapstate;
pub mod repository;
pub mod rule;
pub mod selector;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use cidr::{CIDRPolicy, CIDRRule, generate_cidr_prefixes};
pub use error::{PolicyError, Result};
pub use l4::{L4Policy, L4Traffic, Protocol};
pub use mapstate::{Key, MapStateEntry, MapStateMap, insert_map_state};
pub use repository::{L4PolicyMap, PolicyRepository, Repository, SelectorPolicy};
pub use rule::{DefaultDenyConfig, EgressRule, IngressRule, PolicyRule, Rule};
pub use selector::{CachedSelector, EndpointSelector, Requirement, RequirementOperator, Selector};

/// A set of endpoint labels.
pub type Labels = HashMap<String, String>;

/// Traffic direction for policy rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrafficDirection {
    /// Ingress traffic headed to a local endpoint.
    Ingress,
    /// Egress traffic leaving a local endpoint.
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

/// Numeric endpoint identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointIdentity {
    /// Numeric security identity value.
    pub id: u32,
}

impl EndpointIdentity {
    /// Reserved world identity.
    pub const WORLD: Self = Self { id: 1 };
    /// Reserved host identity.
    pub const HOST: Self = Self { id: 2 };
    /// Reserved local-node identity.
    pub const LOCAL_NODE: Self = Self { id: 6 };

    /// Creates an endpoint identity from a numeric ID.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self { id }
    }

    /// Returns true when the identity is one of the reserved identities.
    #[must_use]
    pub const fn is_reserved(self) -> bool {
        self.id < 100
    }
}

impl std::fmt::Display for EndpointIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

/// Endpoint identity paired with labels for selector evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabeledIdentity {
    /// Numeric endpoint identity.
    pub identity: EndpointIdentity,
    /// Labels associated with the identity.
    pub labels: Labels,
}

impl LabeledIdentity {
    /// Creates a labeled identity.
    #[must_use]
    pub fn new(id: u32, labels: Labels) -> Self {
        Self {
            identity: EndpointIdentity::new(id),
            labels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_direction_display_matches_expected_strings() {
        assert_eq!(TrafficDirection::Ingress.to_string(), "ingress");
        assert_eq!(TrafficDirection::Egress.to_string(), "egress");
    }

    #[test]
    fn reserved_identity_detection_works() {
        assert!(EndpointIdentity::WORLD.is_reserved());
        assert!(!EndpointIdentity::new(512).is_reserved());
    }
}
