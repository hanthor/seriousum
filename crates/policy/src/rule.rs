//! Policy rules: the declarative policy specification

use std::collections::HashMap;

use crate::{EndpointSelector, L4Policy, PolicyError, Result, TrafficDirection};

/// Origin of a policy rule (which object created it)
#[derive(Debug, Clone)]
pub struct RuleOrigin {
    pub namespace: String,
    pub name: String,
    pub resource_version: String,
}

impl RuleOrigin {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>, resource_version: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            resource_version: resource_version.into(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.namespace, self.name, self.resource_version)
    }
}

/// A policy rule: selectors + L4 policy for a direction
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub origin: Option<RuleOrigin>,
    pub direction: TrafficDirection,
    /// Subject selector (usually the endpoint being ruled)
    pub subject_selector: EndpointSelector,
    /// Remote endpoint selector (who they're communicating with)
    pub peer_selector: EndpointSelector,
    /// L4 policy (ports, protocols)
    pub l4_policy: L4Policy,
}

impl PolicyRule {
    pub fn new(direction: TrafficDirection) -> Self {
        Self {
            origin: None,
            direction,
            subject_selector: EndpointSelector::empty(),
            peer_selector: EndpointSelector::empty(),
            l4_policy: L4Policy::new(),
        }
    }

    pub fn with_subject_selector(mut self, selector: EndpointSelector) -> Self {
        self.subject_selector = selector;
        self
    }

    pub fn with_peer_selector(mut self, selector: EndpointSelector) -> Self {
        self.peer_selector = selector;
        self
    }

    pub fn with_l4_policy(mut self, policy: L4Policy) -> Self {
        self.l4_policy = policy;
        self
    }

    pub fn with_origin(mut self, origin: RuleOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Parse a rule from a string representation (simplified)
    pub fn parse(input: &str) -> Result<Self> {
        // Simplified parser: "ingress|egress app=web app=backend tcp:80"
        let parts: Vec<&str> = input.split(' ').collect();
        if parts.len() < 2 {
            return Err(PolicyError::InvalidRule(
                "rule must have at least direction and peer selector".to_string(),
            ));
        }

        let direction = match parts[0] {
            "ingress" => TrafficDirection::Ingress,
            "egress" => TrafficDirection::Egress,
            _ => return Err(PolicyError::InvalidRule(format!("unknown direction: {}", parts[0]))),
        };

        let mut rule = Self::new(direction);

        // Parse peer selector labels
        for part in parts.iter().skip(1) {
            if let Some((key, value)) = part.split_once('=') {
                rule.peer_selector.labels.insert(key.to_string(), value.to_string());
            }
        }

        Ok(rule)
    }

    /// Check if this rule applies to the given peer
    pub fn applies_to(&self, peer_labels: &HashMap<String, String>) -> bool {
        self.peer_selector.matches(peer_labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_origin_key() {
        let origin = RuleOrigin::new("default", "policy-1", "v1");
        assert_eq!(origin.key(), "default/policy-1/v1");
    }

    #[test]
    fn test_policy_rule_new_ingress() {
        let rule = PolicyRule::new(TrafficDirection::Ingress);
        assert_eq!(rule.direction, TrafficDirection::Ingress);
        assert!(rule.origin.is_none());
    }

    #[test]
    fn test_policy_rule_new_egress() {
        let rule = PolicyRule::new(TrafficDirection::Egress);
        assert_eq!(rule.direction, TrafficDirection::Egress);
    }

    #[test]
    fn test_policy_rule_fluent() {
        let subject_sel = EndpointSelector::empty().with_label("app", "web");
        let peer_sel = EndpointSelector::empty().with_label("app", "db");

        let rule = PolicyRule::new(TrafficDirection::Ingress)
            .with_subject_selector(subject_sel.clone())
            .with_peer_selector(peer_sel.clone());

        assert_eq!(rule.subject_selector.labels.len(), 1);
        assert_eq!(rule.peer_selector.labels.len(), 1);
    }

    #[test]
    fn test_policy_rule_parse_ingress() {
        let rule = PolicyRule::parse("ingress app=web").unwrap();
        assert_eq!(rule.direction, TrafficDirection::Ingress);
        assert!(rule.peer_selector.labels.contains_key("app"));
    }

    #[test]
    fn test_policy_rule_parse_egress() {
        let rule = PolicyRule::parse("egress app=backend").unwrap();
        assert_eq!(rule.direction, TrafficDirection::Egress);
    }

    #[test]
    fn test_policy_rule_parse_invalid() {
        let result = PolicyRule::parse("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_policy_rule_applies_to() {
        let rule = PolicyRule::parse("ingress app=web").unwrap();
        let mut peer_labels = HashMap::new();
        peer_labels.insert("app".to_string(), "web".to_string());

        assert!(rule.applies_to(&peer_labels));
    }
}
