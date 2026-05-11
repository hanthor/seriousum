// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Core types for egress gateway

use std::net::{Ipv4Addr, Ipv6Addr};
use std::collections::HashMap;

/// Special IPv4 addresses used as sentinel values in BPF policy maps
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialIPs;

impl SpecialIPs {
    /// Gateway not found IPv4 (0.0.0.0)
    pub const GATEWAY_NOT_FOUND_IPV4: Ipv4Addr = Ipv4Addr::UNSPECIFIED;

    /// Excluded CIDR IPv4 marker (0.0.0.1)
    pub const EXCLUDED_CIDR_IPV4: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 1);

    /// Egress IP not found IPv4 (0.0.0.0)
    pub const EGRESS_IP_NOT_FOUND_IPV4: Ipv4Addr = Ipv4Addr::UNSPECIFIED;

    /// Gateway not found IPv6 (::)
    pub const GATEWAY_NOT_FOUND_IPV6: Ipv6Addr = Ipv6Addr::UNSPECIFIED;

    /// Egress IP not found IPv6 (::)
    pub const EGRESS_IP_NOT_FOUND_IPV6: Ipv6Addr = Ipv6Addr::UNSPECIFIED;
}

/// Identifies an endpoint by UID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EndpointID(pub u64);

impl EndpointID {
    /// Create a new EndpointID from a UID string (using hash)
    pub fn from_uid(uid: &str) -> Self {
        use fnv::FnvHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = FnvHasher::default();
        uid.hash(&mut hasher);
        EndpointID(hasher.finish())
    }

    /// Get the numeric hash value
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Identifies a policy by name and namespace
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PolicyID {
    /// Policy name
    pub name: String,
    /// Policy namespace
    pub namespace: String,
}

impl PolicyID {
    /// Create a new policy ID
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
        }
    }

    /// Create a policy ID with empty namespace
    pub fn from_name(name: impl Into<String>) -> Self {
        Self::new(name, "")
    }
}

impl std::fmt::Display for PolicyID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.namespace.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}/{}", self.namespace, self.name)
        }
    }
}

/// Event type bitmap for tracking state changes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventBitmap(u32);

impl EventBitmap {
    const K8S_SYNC_DONE: u32 = 1 << 0;
    const ADD_POLICY: u32 = 1 << 1;
    const DELETE_POLICY: u32 = 1 << 2;
    const UPDATE_ENDPOINT: u32 = 1 << 3;
    const DELETE_ENDPOINT: u32 = 1 << 4;
    const UPDATE_NODE: u32 = 1 << 5;
    const DELETE_NODE: u32 = 1 << 6;

    /// Create a new empty bitmap
    pub fn new() -> Self {
        Self(0)
    }

    /// Set K8s sync done event
    pub fn set_k8s_sync_done(&mut self) {
        self.0 |= Self::K8S_SYNC_DONE;
    }

    /// Set add policy event
    pub fn set_add_policy(&mut self) {
        self.0 |= Self::ADD_POLICY;
    }

    /// Set delete policy event
    pub fn set_delete_policy(&mut self) {
        self.0 |= Self::DELETE_POLICY;
    }

    /// Set update endpoint event
    pub fn set_update_endpoint(&mut self) {
        self.0 |= Self::UPDATE_ENDPOINT;
    }

    /// Set delete endpoint event
    pub fn set_delete_endpoint(&mut self) {
        self.0 |= Self::DELETE_ENDPOINT;
    }

    /// Set update node event
    pub fn set_update_node(&mut self) {
        self.0 |= Self::UPDATE_NODE;
    }

    /// Set delete node event
    pub fn set_delete_node(&mut self) {
        self.0 |= Self::DELETE_NODE;
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    /// Check if K8s sync done
    pub fn is_k8s_sync_done(&self) -> bool {
        self.0 & Self::K8S_SYNC_DONE != 0
    }

    /// Check if any policy events
    pub fn has_policy_events(&self) -> bool {
        self.0 & (Self::ADD_POLICY | Self::DELETE_POLICY) != 0
    }

    /// Check if any endpoint events
    pub fn has_endpoint_events(&self) -> bool {
        self.0 & (Self::UPDATE_ENDPOINT | Self::DELETE_ENDPOINT) != 0
    }

    /// Check if any node events
    pub fn has_node_events(&self) -> bool {
        self.0 & (Self::UPDATE_NODE | Self::DELETE_NODE) != 0
    }

    /// Check if needs reconciliation
    pub fn needs_reconciliation(&self) -> bool {
        self.0 != 0
    }
}

impl Default for EventBitmap {
    fn default() -> Self {
        Self::new()
    }
}

/// Labels map for Kubernetes resources
pub type Labels = HashMap<String, String>;

/// Node information for gateway selection
#[derive(Debug, Clone)]
pub struct Node {
    /// Node name
    pub name: String,
    /// Node labels
    pub labels: Labels,
    /// Node IP (IPv4 or IPv6)
    pub ip: std::net::IpAddr,
    /// Whether this is the local node
    pub is_local: bool,
}

impl Node {
    /// Create a new node
    pub fn new(
        name: impl Into<String>,
        labels: Labels,
        ip: std::net::IpAddr,
        is_local: bool,
    ) -> Self {
        Self {
            name: name.into(),
            labels,
            ip,
            is_local,
        }
    }
}

/// Selector for matching labels
#[derive(Debug, Clone)]
pub struct LabelSelector {
    /// Label keys and values to match
    pub match_labels: HashMap<String, String>,
    /// Label selector expressions
    pub match_expressions: Vec<LabelSelectorExpression>,
}

impl LabelSelector {
    /// Create a new label selector
    pub fn new() -> Self {
        Self {
            match_labels: HashMap::new(),
            match_expressions: Vec::new(),
        }
    }

    /// Add a match label
    pub fn with_match_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.match_labels.insert(key.into(), value.into());
        self
    }

    /// Check if this selector matches the given labels
    pub fn matches(&self, labels: &Labels) -> bool {
        // Check match labels
        for (key, value) in &self.match_labels {
            if labels.get(key) != Some(value) {
                return false;
            }
        }

        // Check match expressions
        for expr in &self.match_expressions {
            if !expr.matches(labels) {
                return false;
            }
        }

        true
    }
}

impl Default for LabelSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Label selector expression
#[derive(Debug, Clone)]
pub struct LabelSelectorExpression {
    /// Label key
    pub key: String,
    /// Operator (In, NotIn, Exists, DoesNotExist)
    pub operator: LabelSelectorOperator,
    /// Values for comparison
    pub values: Vec<String>,
}

impl LabelSelectorExpression {
    /// Create a new expression
    pub fn new(key: impl Into<String>, operator: LabelSelectorOperator) -> Self {
        Self {
            key: key.into(),
            operator,
            values: Vec::new(),
        }
    }

    /// Add values
    pub fn with_values(mut self, values: Vec<String>) -> Self {
        self.values = values;
        self
    }

    /// Check if this expression matches the given labels
    pub fn matches(&self, labels: &Labels) -> bool {
        match self.operator {
            LabelSelectorOperator::In => {
                if let Some(val) = labels.get(&self.key) {
                    self.values.contains(val)
                } else {
                    false
                }
            }
            LabelSelectorOperator::NotIn => {
                if let Some(val) = labels.get(&self.key) {
                    !self.values.contains(val)
                } else {
                    true
                }
            }
            LabelSelectorOperator::Exists => labels.contains_key(&self.key),
            LabelSelectorOperator::DoesNotExist => !labels.contains_key(&self.key),
        }
    }
}

/// Label selector operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelSelectorOperator {
    /// Value must be in the specified values
    In,
    /// Value must not be in the specified values
    NotIn,
    /// Key must exist
    Exists,
    /// Key must not exist
    DoesNotExist,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_id_from_uid() {
        let uid = "test-uid-123";
        let id1 = EndpointID::from_uid(uid);
        let id2 = EndpointID::from_uid(uid);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_policy_id_display() {
        let id = PolicyID::new("policy1", "namespace1");
        assert_eq!(id.to_string(), "namespace1/policy1");

        let id2 = PolicyID::from_name("policy2");
        assert_eq!(id2.to_string(), "policy2");
    }

    #[test]
    fn test_event_bitmap() {
        let mut bitmap = EventBitmap::new();
        assert!(!bitmap.is_k8s_sync_done());

        bitmap.set_k8s_sync_done();
        assert!(bitmap.is_k8s_sync_done());

        bitmap.set_add_policy();
        assert!(bitmap.has_policy_events());

        bitmap.clear();
        assert!(!bitmap.is_k8s_sync_done());
        assert!(!bitmap.has_policy_events());
    }

    #[test]
    fn test_label_selector_matches() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());
        labels.insert("tier".to_string(), "frontend".to_string());

        let selector = LabelSelector::new()
            .with_match_label("app", "web");

        assert!(selector.matches(&labels));

        let selector2 = LabelSelector::new()
            .with_match_label("app", "db");

        assert!(!selector2.matches(&labels));
    }

    #[test]
    fn test_label_selector_expression() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let expr = LabelSelectorExpression::new("app", LabelSelectorOperator::In)
            .with_values(vec!["web".to_string(), "api".to_string()]);

        assert!(expr.matches(&labels));

        let expr2 = LabelSelectorExpression::new("tier", LabelSelectorOperator::Exists);
        assert!(!expr2.matches(&labels));
    }
}
