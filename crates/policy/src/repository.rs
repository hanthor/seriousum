//! Policy repository: stores and manages policies
//!
//! The repository is the main policy engine: it stores rules, compiles them per
//! endpoint, and generates MapState for eBPF consumption.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::{
    EndpointIdentity, MapState, PolicyError, PolicyRule,
    PolicyVerdict, Result, TrafficDirection,
};

/// Policy repository: stores rules and compiles policy per endpoint
pub struct PolicyRepository {
    /// All ingress rules
    ingress_rules: Arc<DashMap<String, PolicyRule>>,
    /// All egress rules
    egress_rules: Arc<DashMap<String, PolicyRule>>,
    /// Compiled policies per identity
    compiled_policies: Arc<RwLock<HashMap<EndpointIdentity, CompiledPolicy>>>,
}

/// Compiled policy for an endpoint
#[derive(Debug, Clone)]
pub struct CompiledPolicy {
    pub identity: EndpointIdentity,
    pub map_state: MapState,
}

impl PolicyRepository {
    pub fn new() -> Self {
        Self {
            ingress_rules: Arc::new(DashMap::new()),
            egress_rules: Arc::new(DashMap::new()),
            compiled_policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an ingress rule
    pub fn add_ingress_rule(&self, rule_id: impl Into<String>, rule: PolicyRule) -> Result<()> {
        if rule.direction != TrafficDirection::Ingress {
            return Err(PolicyError::InvalidRule(
                "cannot add non-ingress rule to ingress set".to_string(),
            ));
        }
        self.ingress_rules.insert(rule_id.into(), rule);
        Ok(())
    }

    /// Add an egress rule
    pub fn add_egress_rule(&self, rule_id: impl Into<String>, rule: PolicyRule) -> Result<()> {
        if rule.direction != TrafficDirection::Egress {
            return Err(PolicyError::InvalidRule(
                "cannot add non-egress rule to egress set".to_string(),
            ));
        }
        self.egress_rules.insert(rule_id.into(), rule);
        Ok(())
    }

    /// Get an ingress rule
    pub fn get_ingress_rule(&self, rule_id: &str) -> Option<PolicyRule> {
        self.ingress_rules.get(rule_id).map(|r| r.clone())
    }

    /// Get an egress rule
    pub fn get_egress_rule(&self, rule_id: &str) -> Option<PolicyRule> {
        self.egress_rules.get(rule_id).map(|r| r.clone())
    }

    /// Delete an ingress rule
    pub fn delete_ingress_rule(&self, rule_id: &str) -> Option<PolicyRule> {
        self.ingress_rules.remove(rule_id).map(|(_, r)| r)
    }

    /// Delete an egress rule
    pub fn delete_egress_rule(&self, rule_id: &str) -> Option<PolicyRule> {
        self.egress_rules.remove(rule_id).map(|(_, r)| r)
    }

    /// Get count of ingress rules
    pub fn ingress_rule_count(&self) -> usize {
        self.ingress_rules.len()
    }

    /// Get count of egress rules
    pub fn egress_rule_count(&self) -> usize {
        self.egress_rules.len()
    }

    /// Distill policy for an endpoint identity
    ///
    /// This is the main policy compilation algorithm: takes all applicable rules
    /// and compiles them into a MapState suitable for eBPF consumption.
    pub fn distill_policy(
        &self,
        endpoint_identity: EndpointIdentity,
        endpoint_labels: &HashMap<String, String>,
    ) -> Result<CompiledPolicy> {
        let mut map_state = MapState::new();

        // Compile ingress policies
        for rule_ref in self.ingress_rules.iter() {
            let rule = rule_ref.value();

            // Check if rule applies to this endpoint
            if !rule.subject_selector.matches(endpoint_labels) {
                continue;
            }

            // For each peer that matches the selector
            Self::compile_rule_to_mapstate(&mut map_state, rule, endpoint_identity, TrafficDirection::Ingress)?;
        }

        // Compile egress policies
        for rule_ref in self.egress_rules.iter() {
            let rule = rule_ref.value();

            // Check if rule applies to this endpoint
            if !rule.subject_selector.matches(endpoint_labels) {
                continue;
            }

            Self::compile_rule_to_mapstate(&mut map_state, rule, endpoint_identity, TrafficDirection::Egress)?;
        }

        Ok(CompiledPolicy {
            identity: endpoint_identity,
            map_state,
        })
    }

    /// Compile a single rule into MapState
    fn compile_rule_to_mapstate(
        map_state: &mut MapState,
        rule: &PolicyRule,
        endpoint_identity: EndpointIdentity,
        direction: TrafficDirection,
    ) -> Result<()> {
        if rule.direction != direction {
            return Ok(());
        }

        // Determine verdict based on rule type (allow or deny)
        // For now: inferred rules are allow, explicit deny rules are deny
        let verdict = if rule.l4_policy.is_empty() {
            PolicyVerdict::Deny
        } else {
            PolicyVerdict::Allow
        };

        // Add entries for each allowed L4 traffic in the rule
        for traffic in &rule.l4_policy.allowed {
            let protocol_num = match traffic.protocol {
                crate::l4::Protocol::TCP => 6,
                crate::l4::Protocol::UDP => 17,
                crate::l4::Protocol::ICMP => 1,
                crate::l4::Protocol::ICMPv6 => 58,
            };

            // Iterate through port range
            for port in traffic.port_start..=traffic.port_end {
                match direction {
                    TrafficDirection::Ingress => {
                        // For ingress, the peer is the remote identity
                        map_state.add_ingress(endpoint_identity, port, protocol_num, verdict)?;
                    }
                    TrafficDirection::Egress => {
                        // For egress, the peer is the remote identity
                        map_state.add_egress(endpoint_identity, port, protocol_num, verdict)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Get compiled policy for an endpoint
    pub async fn get_compiled_policy(&self, identity: EndpointIdentity) -> Result<Option<CompiledPolicy>> {
        let policies = self.compiled_policies.read().await;
        Ok(policies.get(&identity).cloned())
    }

    /// Store compiled policy for an endpoint
    pub async fn set_compiled_policy(&self, identity: EndpointIdentity, policy: CompiledPolicy) -> Result<()> {
        let mut policies = self.compiled_policies.write().await;
        policies.insert(identity, policy);
        Ok(())
    }

    /// Clear compiled policies
    pub async fn clear_compiled_policies(&self) -> Result<()> {
        let mut policies = self.compiled_policies.write().await;
        policies.clear();
        Ok(())
    }

    /// Get count of compiled policies
    pub async fn compiled_policy_count(&self) -> Result<usize> {
        let policies = self.compiled_policies.read().await;
        Ok(policies.len())
    }
}

impl Default for PolicyRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_repository_new() {
        let repo = PolicyRepository::new();
        assert_eq!(repo.ingress_rule_count(), 0);
        assert_eq!(repo.egress_rule_count(), 0);
    }

    #[test]
    fn test_add_ingress_rule() {
        let repo = PolicyRepository::new();
        let rule = PolicyRule::new(TrafficDirection::Ingress);
        repo.add_ingress_rule("rule-1", rule).unwrap();
        assert_eq!(repo.ingress_rule_count(), 1);
    }

    #[test]
    fn test_add_egress_rule() {
        let repo = PolicyRepository::new();
        let rule = PolicyRule::new(TrafficDirection::Egress);
        repo.add_egress_rule("rule-1", rule).unwrap();
        assert_eq!(repo.egress_rule_count(), 1);
    }

    #[test]
    fn test_wrong_direction_ingress() {
        let repo = PolicyRepository::new();
        let rule = PolicyRule::new(TrafficDirection::Egress);
        let result = repo.add_ingress_rule("rule-1", rule);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_direction_egress() {
        let repo = PolicyRepository::new();
        let rule = PolicyRule::new(TrafficDirection::Ingress);
        let result = repo.add_egress_rule("rule-1", rule);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_ingress_rule() {
        let repo = PolicyRepository::new();
        let rule = PolicyRule::new(TrafficDirection::Ingress);
        repo.add_ingress_rule("rule-1", rule).unwrap();

        let retrieved = repo.get_ingress_rule("rule-1");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_delete_ingress_rule() {
        let repo = PolicyRepository::new();
        let rule = PolicyRule::new(TrafficDirection::Ingress);
        repo.add_ingress_rule("rule-1", rule).unwrap();

        let deleted = repo.delete_ingress_rule("rule-1");
        assert!(deleted.is_some());
        assert_eq!(repo.ingress_rule_count(), 0);
    }

    #[tokio::test]
    async fn test_distill_policy() {
        let repo = PolicyRepository::new();
        let identity = EndpointIdentity::new(42);
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        // Distill with no rules (should succeed with empty policy)
        let compiled = repo.distill_policy(identity, &labels).unwrap();
        assert_eq!(compiled.identity, identity);
        assert!(compiled.map_state.is_ingress_empty());
    }

    #[tokio::test]
    async fn test_compiled_policy_storage() {
        let repo = PolicyRepository::new();
        let identity = EndpointIdentity::new(42);
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let compiled = repo.distill_policy(identity, &labels).unwrap();
        repo.set_compiled_policy(identity, compiled).await.unwrap();

        assert_eq!(repo.compiled_policy_count().await.unwrap(), 1);

        let retrieved = repo.get_compiled_policy(identity).await.unwrap();
        assert!(retrieved.is_some());
    }
}
