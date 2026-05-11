//! Network Policy Subsystem - Enforces Kubernetes network policies
//!
//! Implements Issue #49 (P2.1): Policy Subsystem
//!
//! This component:
//! - Loads and parses Kubernetes NetworkPolicy resources
//! - Evaluates policy selectors and rules
//! - Generates eBPF rules for traffic control
//! - Tracks policy state and changes
//! - Integrates with load balancer for access control

use serde::{Deserialize, Serialize};
use seriousum_core::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

// ============================================================================
// Data Structures
// ============================================================================

/// Network policy resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    pub name: String,
    pub namespace: String,
    pub pod_selector: Selector,
    pub ingress_rules: Vec<IngressRule>,
    pub egress_rules: Vec<EgressRule>,
    pub policy_types: Vec<PolicyType>,
}

/// Selector for matching pods/namespaces
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selector {
    labels: HashMap<String, String>,
}

impl Selector {
    pub fn new(labels: HashMap<String, String>) -> Self {
        Self { labels }
    }

    pub fn matches(&self, pod_labels: &HashMap<String, String>) -> bool {
        self.labels
            .iter()
            .all(|(key, value)| pod_labels.get(key) == Some(value))
    }
}

/// Ingress rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressRule {
    pub from: Vec<PolicyPeer>,
    pub ports: Vec<PolicyPort>,
}

/// Egress rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressRule {
    pub to: Vec<PolicyPeer>,
    pub ports: Vec<PolicyPort>,
}

/// Policy peer (from/to)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyPeer {
    pub pod_selector: Option<Selector>,
    pub namespace_selector: Option<Selector>,
}

/// Network port specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyPort {
    pub protocol: String,
    pub port: u16,
}

/// Policy type (Ingress/Egress)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyType {
    Ingress,
    Egress,
}

/// Evaluated policy rule
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub direction: Direction,
    pub source_labels: HashMap<String, String>,
    pub dest_labels: HashMap<String, String>,
    pub ports: Vec<u16>,
    pub protocol: String,
    pub action: Action,
}

/// Traffic direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ingress,
    Egress,
}

/// Traffic action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
}

// ============================================================================
// Policy Cache
// ============================================================================

/// In-memory cache of network policies
pub struct PolicyCache {
    policies: Arc<RwLock<HashMap<String, NetworkPolicy>>>,
}

impl PolicyCache {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add or update a policy
    pub async fn add_policy(&self, policy: NetworkPolicy) {
        let key = format!("{}/{}", policy.namespace, policy.name);
        let mut policies = self.policies.write().await;
        policies.insert(key.clone(), policy);
        debug!("Added policy: {}", key);
    }

    /// Remove a policy
    pub async fn remove_policy(&self, namespace: &str, name: &str) -> Option<NetworkPolicy> {
        let key = format!("{}/{}", namespace, name);
        let mut policies = self.policies.write().await;
        let removed = policies.remove(&key);
        if removed.is_some() {
            debug!("Removed policy: {}", key);
        }
        removed
    }

    /// Get all policies
    pub async fn list_policies(&self) -> Vec<NetworkPolicy> {
        let policies = self.policies.read().await;
        policies.values().cloned().collect()
    }

    /// Get policies matching pod labels
    pub async fn get_policies_for_pod(&self, pod_labels: &HashMap<String, String>) -> Vec<NetworkPolicy> {
        let policies = self.policies.read().await;
        policies
            .values()
            .filter(|p| p.pod_selector.matches(pod_labels))
            .cloned()
            .collect()
    }

    /// Count policies
    pub async fn policy_count(&self) -> usize {
        self.policies.read().await.len()
    }
}

impl Default for PolicyCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Policy Evaluator
// ============================================================================

/// Evaluates policy rules
pub struct PolicyEvaluator;

impl PolicyEvaluator {
    /// Check if a pod matches a selector
    pub fn selector_matches(pod_labels: &HashMap<String, String>, selector: &Selector) -> bool {
        selector.matches(pod_labels)
    }

    /// Evaluate ingress rules for a pod
    pub fn evaluate_ingress(
        policies: &[NetworkPolicy],
    ) -> Vec<IngressRule> {
        policies
            .iter()
            .filter(|p| p.policy_types.contains(&PolicyType::Ingress))
            .flat_map(|p| p.ingress_rules.iter().cloned())
            .collect()
    }

    /// Evaluate egress rules for a pod
    pub fn evaluate_egress(
        policies: &[NetworkPolicy],
    ) -> Vec<EgressRule> {
        policies
            .iter()
            .filter(|p| p.policy_types.contains(&PolicyType::Egress))
            .flat_map(|p| p.egress_rules.iter().cloned())
            .collect()
    }
}

// ============================================================================
// Policy Enforcer
// ============================================================================

/// Applies policies to eBPF maps
pub struct PolicyEnforcer {
    cache: Arc<PolicyCache>,
}

impl PolicyEnforcer {
    pub fn new(cache: Arc<PolicyCache>) -> Self {
        Self { cache }
    }

    /// Enforce all policies
    pub async fn enforce(&self) -> Result<()> {
        let policies = self.cache.list_policies().await;
        debug!("Enforcing {} policies", policies.len());
        // TODO: Generate eBPF rules and apply to maps
        Ok(())
    }

    /// Update policy enforcement
    pub async fn update_policy(&self, policy: NetworkPolicy) -> Result<()> {
        self.cache.add_policy(policy).await;
        self.enforce().await
    }

    /// Remove policy enforcement
    pub async fn remove_policy(&self, namespace: &str, name: &str) -> Result<()> {
        self.cache.remove_policy(namespace, name).await;
        self.enforce().await
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_matches_exact_labels() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());
        labels.insert("tier".to_string(), "frontend".to_string());

        let mut selector_labels = HashMap::new();
        selector_labels.insert("app".to_string(), "web".to_string());
        let selector = Selector::new(selector_labels);

        assert!(selector.matches(&labels));
    }

    #[test]
    fn selector_fails_on_missing_label() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let mut selector_labels = HashMap::new();
        selector_labels.insert("tier".to_string(), "frontend".to_string());
        let selector = Selector::new(selector_labels);

        assert!(!selector.matches(&labels));
    }

    #[test]
    fn selector_fails_on_wrong_value() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let mut selector_labels = HashMap::new();
        selector_labels.insert("app".to_string(), "api".to_string());
        let selector = Selector::new(selector_labels);

        assert!(!selector.matches(&labels));
    }

    #[test]
    fn empty_selector_matches_all() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let selector = Selector::new(HashMap::new());
        assert!(selector.matches(&labels));
    }

    #[tokio::test]
    async fn policy_cache_add_and_list() {
        let cache = PolicyCache::new();

        let policy = NetworkPolicy {
            name: "test-policy".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(HashMap::new()),
            ingress_rules: vec![],
            egress_rules: vec![],
            policy_types: vec![PolicyType::Ingress],
        };

        cache.add_policy(policy).await;
        assert_eq!(cache.policy_count().await, 1);

        let policies = cache.list_policies().await;
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name, "test-policy");
    }

    #[tokio::test]
    async fn policy_cache_remove() {
        let cache = PolicyCache::new();

        let policy = NetworkPolicy {
            name: "test-policy".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(HashMap::new()),
            ingress_rules: vec![],
            egress_rules: vec![],
            policy_types: vec![PolicyType::Ingress],
        };

        cache.add_policy(policy).await;
        assert_eq!(cache.policy_count().await, 1);

        cache.remove_policy("default", "test-policy").await;
        assert_eq!(cache.policy_count().await, 0);
    }

    #[tokio::test]
    async fn policy_cache_get_for_pod() {
        let cache = PolicyCache::new();

        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let policy = NetworkPolicy {
            name: "web-policy".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(labels.clone()),
            ingress_rules: vec![],
            egress_rules: vec![],
            policy_types: vec![PolicyType::Ingress],
        };

        cache.add_policy(policy).await;

        let policies = cache.get_policies_for_pod(&labels).await;
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name, "web-policy");
    }

    #[test]
    fn policy_evaluator_ingress() {
        let policies = vec![NetworkPolicy {
            name: "policy".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(HashMap::new()),
            ingress_rules: vec![IngressRule {
                from: vec![],
                ports: vec![],
            }],
            egress_rules: vec![],
            policy_types: vec![PolicyType::Ingress],
        }];

        let rules = PolicyEvaluator::evaluate_ingress(&policies);
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn policy_evaluator_egress() {
        let policies = vec![NetworkPolicy {
            name: "policy".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(HashMap::new()),
            ingress_rules: vec![],
            egress_rules: vec![EgressRule {
                to: vec![],
                ports: vec![],
            }],
            policy_types: vec![PolicyType::Egress],
        }];

        let rules = PolicyEvaluator::evaluate_egress(&policies);
        assert_eq!(rules.len(), 1);
    }

    #[tokio::test]
    async fn policy_enforcer_creation() {
        let cache = Arc::new(PolicyCache::new());
        let enforcer = PolicyEnforcer::new(cache);

        let result = enforcer.enforce().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn policy_enforcer_update() {
        let cache = Arc::new(PolicyCache::new());
        let enforcer = PolicyEnforcer::new(cache);

        let policy = NetworkPolicy {
            name: "test".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(HashMap::new()),
            ingress_rules: vec![],
            egress_rules: vec![],
            policy_types: vec![PolicyType::Ingress],
        };

        let result = enforcer.update_policy(policy).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn policy_enforcer_remove() {
        let cache = Arc::new(PolicyCache::new());
        let enforcer = PolicyEnforcer::new(cache.clone());

        let policy = NetworkPolicy {
            name: "test".to_string(),
            namespace: "default".to_string(),
            pod_selector: Selector::new(HashMap::new()),
            ingress_rules: vec![],
            egress_rules: vec![],
            policy_types: vec![PolicyType::Ingress],
        };

        enforcer.update_policy(policy).await.unwrap();
        assert_eq!(cache.policy_count().await, 1);

        enforcer.remove_policy("default", "test").await.unwrap();
        assert_eq!(cache.policy_count().await, 0);
    }
}
